use crate::{db, fide, lichess_broadcast, pgn, twic};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

/// Fenetre, en semaines (~4 mois), au-dela de laquelle on ne tente plus de
/// consolider avec Lichess (recherche + recoupement de dates par tournoi,
/// potentiellement couteux si beaucoup de semaines sont en retard) -- les
/// editions TWIC plus anciennes que ca dans le rattrapage sont importees
/// telles quelles, sans enrichissement Lichess. Choix explicite de
/// l'utilisateur (pas une estimation), voir la discussion qui a mene a cette
/// fonctionnalite.
const LICHESS_LOOKBACK_WEEKS: u32 = 17;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreviewTournament {
    pub event: String,
    pub games: u32,
    /// "twic" | "lichess" | "both" -- "both" des que ce tournoi (par nom
    /// d'Event) a ete rencontre dans les deux sources, meme si certaines de
    /// ses parties Lichess etaient prioritaires sur leur equivalent TWIC
    /// (voir compute_update_preview).
    pub source: String,
    /// Date de la premiere/derniere partie retenue pour ce tournoi (tag Date,
    /// "AAAA.MM.JJ") -- None si aucune partie retenue n'a de date exploitable
    /// (voir db::is_known_date).
    pub date_start: Option<String>,
    pub date_end: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreview {
    pub tournaments: Vec<UpdatePreviewTournament>,
    pub total_games: u32,
    pub from_issue: u32,
    pub to_issue: u32,
    pub weeks_behind: u32,
    /// Chemin d'un fichier PGN temporaire deja consolide/dedoublonne, pret a
    /// etre passe tel quel a la commande bulk_import_pgn existante -- aucune
    /// nouvelle logique d'ecriture en base n'est introduite par cette
    /// fonctionnalite.
    pub pgn_path: String,
}

/// Cle de correspondance entre une partie TWIC et son eventuel equivalent
/// Lichess : (blancs canonises, noirs canonises, date, resultat). Pas besoin
/// de comparer les coups eux-memes ici -- un faux negatif (deux parties
/// distinctes partageant par coincidence ces quatre champs) se contenterait
/// de laisser les deux cote a cote, rattrape ensuite sans degat par le vrai
/// dedoublonnage (plus strict, coups compris) qu'execute deja bulk_import au
/// moment de l'import reel.
type GameKey = (String, String, String, String);

fn canon_name(fide_conn: Option<&Connection>, raw: &str) -> String {
    fide_conn.and_then(|c| fide::canonicalize(c, raw, None)).map(|(name, _)| name).unwrap_or_else(|| raw.trim().to_string())
}

fn game_key(fide_conn: Option<&Connection>, g: &pgn::PgnGameSummary) -> GameKey {
    (canon_name(fide_conn, &g.white), canon_name(fide_conn, &g.black), g.date.clone(), g.result.clone())
}

// Petit adaptateur local : rusqlite::Result -> Result<Option<T>, String>,
// traitant QueryReturnedNoRows comme None plutot que comme une erreur (voir
// most_recent_known_date, seul appelant).
trait OptionalOrString<T> {
    fn optional_or_string(self) -> Result<Option<T>, String>;
}
impl<T> OptionalOrString<T> for rusqlite::Result<T> {
    fn optional_or_string(self) -> Result<Option<T>, String> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Date de partie la plus recente et exploitable (annee reelle, pas
/// "????.??.??") d'une base -- sert a deduire par ou reprendre le
/// rattrapage (voir infer_start_issue). GLOB (pas LIKE) sur une annee a 4
/// chiffres commencant par 1 ou 2 : exclut les dates entierement ou
/// partiellement inconnues sans dependre d'un format de date particulier
/// au-dela du tag PGN standard "AAAA.MM.JJ".
fn most_recent_known_date(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT date_played FROM games WHERE date_played GLOB '[12][0-9][0-9][0-9].*' ORDER BY date_played DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional_or_string()
}

/// Edition TWIC par laquelle reprendre le rattrapage : deduite de la partie
/// la plus recente deja connue dans la base de reference (voir
/// twic::issue_for_date, deliberement conservateur). A defaut de date
/// exploitable (base vide, ou uniquement des dates inconnues), repli sur
/// "une seule semaine en arriere" -- ni tout l'historique TWIC, ni rien.
fn infer_start_issue(reference_db_path: &str, latest_issue: u32) -> Result<u32, String> {
    let conn = db::open(Path::new(reference_db_path))?;
    let start = most_recent_known_date(&conn)?.and_then(|d| twic::issue_for_date(&d)).unwrap_or_else(|| latest_issue.saturating_sub(1));
    Ok(start.min(latest_issue))
}

/// Parties "terminees" seulement : un resultat "*" (encore en cours, surtout
/// pertinent pour Lichess, TWIC ne publie que du termine) ne doit jamais
/// etre propose -- voir la discussion qui a mene a cette fonctionnalite.
fn is_finished(result: &str) -> bool {
    result != "*"
}

/// Un PGN deja parse (list_games), pret a etre compare a une autre source
/// (voir compute_update_preview) et, pour les parties retenues, reextrait en
/// texte complet (voir append_game) a partir du MEME buffer d'origine.
struct ParsedSource {
    bytes: Vec<u8>,
    games: Vec<pgn::PgnGameSummary>,
}

fn parse_source(bytes: Vec<u8>) -> Result<ParsedSource, String> {
    let games = pgn::list_games(&bytes)?.into_iter().filter(|g| is_finished(&g.result)).collect();
    Ok(ParsedSource { bytes, games })
}

/// Reextrait une partie par son index dans son buffer source et l'ajoute au
/// fichier de sortie -- reutilise parse_game_at/write_game (deja testees
/// pour tout le reste de l'appli) plutot qu'un nouveau chemin de
/// (de)serialisation PGN specifique a cette fonctionnalite.
fn append_game(out: &mut impl Write, source: &ParsedSource, index: u32) -> Result<(), String> {
    let tree = pgn::parse_game_at(&source.bytes, index)?;
    writeln!(out, "{}\n", pgn::write_game(&tree)).map_err(|e| e.to_string())
}

#[derive(Default)]
struct TournamentTally {
    games_written: u32,
    from_twic: bool,
    from_lichess: bool,
    date_start: Option<String>,
    date_end: Option<String>,
}

/// Ecrit les parties d'une source dans le fichier de sortie, en sautant
/// celles deja ecrites par une source prioritaire (voir `written`) -- mais
/// note `from_twic`/`from_lichess` dans tous les cas, meme pour une partie
/// sautee : un tournoi couvert par les deux sources doit se signaler "both"
/// a l'utilisateur, pas seulement "lichess" parce que ses parties TWIC se
/// sont toutes averees des doublons.
fn write_source(
    out: &mut impl Write,
    source: &ParsedSource,
    fide_conn: Option<&Connection>,
    is_lichess: bool,
    written: &mut BTreeSet<GameKey>,
    per_tournament: &mut BTreeMap<String, TournamentTally>,
) -> Result<u32, String> {
    let mut added = 0u32;
    for (i, g) in source.games.iter().enumerate() {
        let tally = per_tournament.entry(g.event.clone()).or_default();
        if is_lichess {
            tally.from_lichess = true;
        } else {
            tally.from_twic = true;
        }
        let key = game_key(fide_conn, g);
        if written.insert(key) {
            append_game(out, source, i as u32)?;
            tally.games_written += 1;
            added += 1;
            if db::is_known_date(&g.date) {
                if tally.date_start.as_deref().is_none_or(|start| g.date.as_str() < start) {
                    tally.date_start = Some(g.date.clone());
                }
                if tally.date_end.as_deref().is_none_or(|end| g.date.as_str() > end) {
                    tally.date_end = Some(g.date.clone());
                }
            }
        }
    }
    Ok(added)
}

/// Plage d'editions TWIC a rattraper, sans rien telecharger -- juste assez
/// pour decider s'il y a du nouveau et, le cas echeant, avertir
/// explicitement l'utilisateur AVANT de lancer le gros du travail si le
/// retard est important (voir LARGE_CATCHUP_WARNING_WEEKS). Rapide : un
/// sondage HTTP (HEAD, pas de telechargement) par semaine d'ecart entre la
/// derniere edition connue et aujourd'hui, plus une requete SQL locale.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatchupRange {
    pub from_issue: u32,
    pub to_issue: u32,
    pub weeks_behind: u32,
}

fn compute_catchup_range(reference_db_path: &str) -> Result<Option<CatchupRange>, String> {
    let latest_issue = twic::find_latest_issue()?;
    let start_issue = infer_start_issue(reference_db_path, latest_issue)?;
    if start_issue >= latest_issue {
        return Ok(None);
    }
    Ok(Some(CatchupRange { from_issue: start_issue + 1, to_issue: latest_issue, weeks_behind: latest_issue - start_issue }))
}

/// Etape legere appelee en premier (voir check_external_updates_estimate,
/// lib.rs) : seulement de quoi afficher "X semaines de retard" et, le cas
/// echeant, demander confirmation avant le calcul complet -- celui-ci
/// (compute_update_preview) telecharge potentiellement des dizaines
/// d'editions et interroge Lichess par tournoi, trop couteux pour servir de
/// simple verification.
pub fn estimate_catchup(reference_db_path: &str) -> Result<Option<CatchupRange>, String> {
    compute_catchup_range(reference_db_path)
}

/// Coeur de la fonctionnalite : rattrape toutes les editions TWIC manquees
/// depuis la derniere partie connue de `reference_db_path`, les consolide
/// avec les tournois Lichess correspondants (recherche + recoupement de
/// dates, Lichess prioritaire en cas de doublon) pour les semaines recentes
/// uniquement (voir LICHESS_LOOKBACK_WEEKS), et ecrit le resultat dans
/// `output_pgn_path`. None si deja a jour. `fide_conn`, s'il est fourni,
/// ameliore la detection de doublons entre sources (memes joueurs ecrits
/// differemment) mais n'est pas requis. A appeler apres estimate_catchup
/// (et, si le retard est important, apres confirmation explicite de
/// l'utilisateur) -- pas de nouvel avertissement ici, cette fonction fait
/// tout le travail sans s'arreter en cours de route.
pub fn compute_update_preview(
    reference_db_path: &str,
    fide_conn: Option<&Connection>,
    output_pgn_path: &Path,
    mut on_progress: impl FnMut(String),
) -> Result<Option<UpdatePreview>, String> {
    on_progress("Recherche de la dernière édition TWIC disponible...".to_string());
    let Some(range) = compute_catchup_range(reference_db_path)? else { return Ok(None) };
    let (start_issue, latest_issue) = (range.from_issue - 1, range.to_issue);

    let mut out = std::io::BufWriter::new(std::fs::File::create(output_pgn_path).map_err(|e| e.to_string())?);
    let mut written: BTreeSet<GameKey> = BTreeSet::new();
    let mut per_tournament: BTreeMap<String, TournamentTally> = BTreeMap::new();
    let mut total_games = 0u32;

    for issue in (start_issue + 1)..=latest_issue {
        on_progress(format!("Édition TWIC {issue}..."));
        let zip_bytes = twic::download_issue(issue)?;
        let pgn_text = twic::extract_pgn(&zip_bytes)?;
        let twic_source = parse_source(pgn_text.into_bytes())?;

        let weeks_from_latest = latest_issue - issue;
        if weeks_from_latest <= LICHESS_LOOKBACK_WEEKS {
            on_progress(format!("Recherche des tournois Lichess correspondant à l'édition {issue}..."));
            if let Some(lichess_source) = fetch_matching_lichess_games(&twic_source, &mut on_progress)? {
                // Lichess prioritaire : ecrit en premier, TWIC ne comble
                // ensuite que ce qui manque (choix explicite de l'utilisateur).
                total_games += write_source(&mut out, &lichess_source, fide_conn, true, &mut written, &mut per_tournament)?;
            }
        }
        total_games += write_source(&mut out, &twic_source, fide_conn, false, &mut written, &mut per_tournament)?;
    }
    out.flush().map_err(|e| e.to_string())?;

    let tournaments = per_tournament
        .into_iter()
        .filter(|(_, tally)| tally.games_written > 0)
        .map(|(event, tally)| UpdatePreviewTournament {
            event,
            games: tally.games_written,
            source: match (tally.from_twic, tally.from_lichess) {
                (true, true) => "both",
                (false, true) => "lichess",
                _ => "twic",
            }
            .to_string(),
            date_start: tally.date_start,
            date_end: tally.date_end,
        })
        .collect();

    Ok(Some(UpdatePreview {
        tournaments,
        total_games,
        from_issue: start_issue + 1,
        to_issue: latest_issue,
        weeks_behind: latest_issue - start_issue,
        pgn_path: output_pgn_path.to_string_lossy().into_owned(),
    }))
}

/// Reecrit `pgn_path` (le fichier produit par compute_update_preview) sans
/// les parties dont le tournoi (tag Event) figure dans `excluded_events` --
/// permet a l'utilisateur de decocher des tournois dans la revue (voir
/// ExternalUpdatesReviewDialog.tsx) juste avant l'import, sans refaire tout
/// le rattrapage (telechargement TWIC + recoupement Lichess). Ecrit dans un
/// nouveau fichier plutot que d'ecraser `pgn_path` : si l'utilisateur rouvre
/// la revue sans repasser par une nouvelle verification (voir la
/// restauration apres veille cote frontend), le fichier d'origine doit
/// toujours contenir la totalite des parties.
pub fn filter_pgn_by_excluded_events(pgn_path: &Path, excluded_events: &BTreeSet<String>) -> Result<String, String> {
    let bytes = std::fs::read(pgn_path).map_err(|e| e.to_string())?;
    let source = parse_source(bytes)?;
    let out_path = pgn_path.with_extension("filtered.pgn");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&out_path).map_err(|e| e.to_string())?);
    for (i, g) in source.games.iter().enumerate() {
        if excluded_events.contains(&g.event) {
            continue;
        }
        append_game(&mut out, &source, i as u32)?;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(out_path.to_string_lossy().into_owned())
}

/// Pour chaque tournoi distinct d'une edition TWIC, cherche le tournoi
/// Lichess correspondant (nom + recoupement de dates, voir
/// lichess_broadcast::date_range_overlaps) et rapporte ses parties
/// terminees -- repli silencieux sur "pas d'equivalent Lichess pour ce
/// tournoi" en cas d'ambiguite ou d'echec reseau ponctuel, jamais une erreur
/// qui interromprait tout le rattrapage pour une recherche optionnelle.
fn fetch_matching_lichess_games(twic_source: &ParsedSource, on_progress: &mut impl FnMut(String)) -> Result<Option<ParsedSource>, String> {
    let mut events: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for g in &twic_source.games {
        let Some(days) = twic::days_since_epoch(&g.date) else { continue };
        let ms = days * 86_400_000;
        let entry = events.entry(g.event.clone()).or_insert((ms, ms));
        entry.0 = entry.0.min(ms);
        entry.1 = entry.1.max(ms);
    }

    let mut combined_pgn = String::new();
    let mut any_match = false;
    for (event, range) in events {
        // Marge d'un jour de part et d'autre : les fuseaux horaires/l'heure
        // de debut de ronde peuvent decaler de quelques heures la date cote
        // Lichess par rapport au tag Date (jour seul) cote TWIC.
        let padded = [range.0 - 86_400_000, range.1 + 86_400_000];
        let Ok(results) = lichess_broadcast::search(&event, 2) else { continue };
        let candidates: Vec<_> =
            results.into_iter().filter(|r| r.tour.dates.is_some_and(|d| lichess_broadcast::date_range_overlaps(d, padded))).collect();
        // Un seul candidat sans ambiguite : repli sur TWIC seul sinon (voir
        // le commentaire de la fonction).
        let [candidate] = candidates.as_slice() else { continue };
        on_progress(format!("Récupération de « {} » sur Lichess...", candidate.tour.name));
        // Une seule requete pour TOUTES les rondes du tournoi (voir
        // lichess_broadcast::tournament_pgn) -- une version anterieure
        // listait les rondes puis exportait chacune separement, ce qui
        // multipliait les requetes HTTP par le nombre de rondes et rendait le
        // rattrapage tres lent sur plusieurs semaines (rapporte par
        // l'utilisateur : "ça a été très long").
        let Ok(tournament_pgn) = lichess_broadcast::tournament_pgn(&candidate.tour.id) else { continue };
        // Le PGN exporte par Lichess pour une partie de broadcast met dans
        // [Event] un texte PAR PARTIE ("Round 6: Blancs - Noirs"), pas le nom
        // du tournoi (verifie manuellement sur l'API) -- laisse tel quel,
        // chaque partie Lichess devenait son propre "tournoi" dans
        // per_tournament (rapporte par l'utilisateur : une ligne par
        // ronde/partie au lieu d'une par tournoi). On le remplace ici par le
        // nom TWIC de ce tournoi (`event`, meme boucle) plutot que
        // `candidate.tour.name` : les deux sources doivent partager
        // exactement la meme cle pour que write_source detecte "both", et
        // c'est aussi le nom qui finira dans la base apres import.
        combined_pgn.push_str(&rewrite_event_tag(&tournament_pgn, &event));
        combined_pgn.push('\n');
        any_match = true;
    }

    if !any_match {
        return Ok(None);
    }
    Ok(Some(parse_source(combined_pgn.into_bytes())?))
}

/// Remplace le tag [Event "..."] de chaque partie d'un PGN par une valeur
/// fixe -- voir fetch_matching_lichess_games, seul appelant, pour pourquoi
/// c'est necessaire sur les parties Lichess. Remplacement ligne a ligne (pas
/// de parsing PGN complet) : le tag Event est toujours seul sur sa ligne,
/// avant le movetext, aussi bien dans les exports Lichess que TWIC.
fn rewrite_event_tag(pgn_text: &str, event: &str) -> String {
    let escaped = event.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = String::with_capacity(pgn_text.len());
    for line in pgn_text.lines() {
        if line.starts_with("[Event \"") {
            out.push_str(&format!("[Event \"{escaped}\"]"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_finished_rejects_only_the_star_placeholder() {
        assert!(is_finished("1-0"));
        assert!(is_finished("0-1"));
        assert!(is_finished("1/2-1/2"));
        assert!(!is_finished("*"));
    }

    #[test]
    fn most_recent_known_date_ignores_unknown_year_placeholders() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, date_played TEXT, pgn_moves TEXT NOT NULL,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL);
             INSERT INTO games (date_played, pgn_moves, created_at, last_modified) VALUES
                ('2026.09.21', 'e4', 0, 0),
                ('2026.10.05', 'e4', 0, 0),
                ('????.??.??', 'e4', 0, 0);",
        )
        .unwrap();
        let most_recent = most_recent_known_date(&conn).unwrap();
        assert_eq!(most_recent.as_deref(), Some("2026.10.05"));
    }

    #[test]
    fn most_recent_known_date_is_none_when_every_date_is_unknown() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, date_played TEXT, pgn_moves TEXT NOT NULL,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL);
             INSERT INTO games (date_played, pgn_moves, created_at, last_modified) VALUES ('????.??.??', 'e4', 0, 0);",
        )
        .unwrap();
        assert_eq!(most_recent_known_date(&conn).unwrap(), None);
    }

    #[test]
    fn rewrite_event_tag_replaces_only_the_event_line_of_each_game() {
        let pgn = "[Event \"Round 6: Alice - Bob\"]\n[Site \"https://lichess.org\"]\n[White \"Alice\"]\n\n1. e4 *\n\n\
                    [Event \"Round 6: Carol - Dan\"]\n[White \"Carol\"]\n\n1. d4 *\n";
        let rewritten = rewrite_event_tag(pgn, "4th Sinquefield Cup 2024");
        assert_eq!(rewritten.matches("[Event \"4th Sinquefield Cup 2024\"]").count(), 2);
        assert!(!rewritten.contains("Alice - Bob"));
        assert!(rewritten.contains("[White \"Alice\"]"));
        assert!(rewritten.contains("[White \"Carol\"]"));
    }

    #[test]
    fn rewrite_event_tag_escapes_quotes_and_backslashes() {
        let pgn = "[Event \"Round 1: A - B\"]\n\n1. e4 *\n";
        let rewritten = rewrite_event_tag(pgn, "Tournoi \"Spécial\" \\ 2026");
        assert!(rewritten.contains("[Event \"Tournoi \\\"Spécial\\\" \\\\ 2026\"]"));
    }

    #[test]
    fn filter_pgn_by_excluded_events_keeps_only_non_excluded_tournaments() {
        let dir = std::env::temp_dir().join(format!("chesscraft_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pgn_path = dir.join("input.pgn");
        std::fs::write(
            &pgn_path,
            "[Event \"Tournoi A\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2026.01.01\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n\
             [Event \"Tournoi B\"]\n[White \"Carol\"]\n[Black \"Dan\"]\n[Date \"2026.01.02\"]\n[Result \"0-1\"]\n\n1. d4 0-1\n",
        )
        .unwrap();

        let excluded: BTreeSet<String> = ["Tournoi A".to_string()].into_iter().collect();
        let out_path = filter_pgn_by_excluded_events(&pgn_path, &excluded).unwrap();
        let out_text = std::fs::read_to_string(&out_path).unwrap();
        assert!(!out_text.contains("Tournoi A"));
        assert!(out_text.contains("Tournoi B"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
