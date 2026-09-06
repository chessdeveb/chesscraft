use rusqlite::{params, Connection};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const DOWNLOAD_URL: &str = "https://ratings.fide.com/download/players_list.zip";
const ARCHIVE_ENTRY_NAME: &str = "players_list_foa.txt";

/// Positions (debut de colonne) du format a largeur fixe publie par la FIDE,
/// determinees depuis la ligne d'en-tete du fichier reel (chaque ligne fait
/// 162 caracteres + CRLF) :
/// ID Number | Name | Fed | Sex | Tit | WTit | OTit | FOA | SRtng | SGm | SK
/// | RRtng | RGm | Rk | BRtng | BGm | BK | B-day | Flag
/// Pas de format documente officiellement stable : si la FIDE change un jour
/// la mise en page, `parse_line` echouera silencieusement ligne par ligne
/// (voir la boucle dans `refresh`) plutot que de faire planter tout l'import.
const COL_STARTS: [usize; 19] = [0, 15, 76, 80, 84, 89, 94, 109, 113, 119, 123, 126, 132, 136, 139, 145, 149, 152, 158];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FidePlayer {
    pub fide_id: String,
    pub name: String,
    pub fed: String,
    pub sex: String,
    pub title: String,
    pub w_title: String,
    pub o_title: String,
    pub std_rating: Option<i32>,
    pub rapid_rating: Option<i32>,
    pub blitz_rating: Option<i32>,
    pub birth_year: Option<i32>,
    pub flag: String,
}

fn col(line: &str, i: usize) -> &str {
    let start = COL_STARTS[i];
    let end = COL_STARTS.get(i + 1).copied().unwrap_or(line.len());
    if start >= line.len() {
        return "";
    }
    line.get(start..end.min(line.len())).unwrap_or("").trim()
}

fn parse_rating(s: &str) -> Option<i32> {
    let n: i32 = s.parse().ok()?;
    if n == 0 {
        None
    } else {
        Some(n)
    }
}

/// Une ligne du fichier FIDE vers un FidePlayer, ou None si la ligne est
/// trop courte / mal formee (voir COL_STARTS) plutot que de paniquer.
fn parse_line(line: &str) -> Option<FidePlayer> {
    if line.len() < COL_STARTS[1] {
        return None;
    }
    let fide_id = col(line, 0).to_string();
    if fide_id.is_empty() {
        return None;
    }
    Some(FidePlayer {
        fide_id,
        name: col(line, 1).to_string(),
        fed: col(line, 2).to_string(),
        sex: col(line, 3).to_string(),
        title: col(line, 4).to_string(),
        w_title: col(line, 5).to_string(),
        o_title: col(line, 6).to_string(),
        std_rating: parse_rating(col(line, 8)),
        rapid_rating: parse_rating(col(line, 11)),
        blitz_rating: parse_rating(col(line, 14)),
        birth_year: col(line, 17).parse().ok(),
        flag: col(line, 18).to_string(),
    })
}

fn db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("fide_players.sqlite")
}

pub fn open(app_data_dir: &Path) -> Result<Connection, String> {
    open_at(&db_path(app_data_dir))
}

/// Separee de `open` pour rester testable avec le sentinel SQLite
/// `:memory:` tel quel (open() le concatenerait avec app_data_dir en un
/// chemin de fichier invalide, `:memory:\fide_players.sqlite`). pub(crate)
/// (pas juste prive) pour etre aussi utilisable depuis les tests de
/// bulk_import.rs, qui ont besoin d'une base FIDE en memoire pour verifier
/// la canonicalisation de bout en bout.
pub(crate) fn open_at(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    // meme raisonnement que db::open : WAL laisse la table se lire pendant
    // qu'un rafraichissement ecrit (~1,9M lignes, plusieurs secondes), et le
    // busy_timeout evite un "database is locked" immediat en cas de bref
    // chevauchement.
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS fide_players (
            fide_id TEXT PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE,
            fed TEXT NOT NULL,
            sex TEXT NOT NULL,
            title TEXT NOT NULL,
            w_title TEXT NOT NULL,
            o_title TEXT NOT NULL,
            std_rating INTEGER,
            rapid_rating INTEGER,
            blitz_rating INTEGER,
            birth_year INTEGER,
            flag TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_fide_players_name ON fide_players(name);
        CREATE TABLE IF NOT EXISTS fide_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

pub fn last_updated(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT value FROM fide_meta WHERE key = 'last_updated'", [], |row| row.get(0))
        .ok()
}

struct NameCandidate {
    name: String,
    std_rating: Option<i32>,
    rapid_rating: Option<i32>,
    blitz_rating: Option<i32>,
}

impl NameCandidate {
    /// Classement le plus representatif disponible pour departager par Elo
    /// (voir disambiguate_by_elo) : standard d'abord, sinon rapide/blitz --
    /// mieux vaut une estimation approximative que rien pour un joueur qui
    /// ne joue pas (ou plus) en cadence standard.
    fn best_rating(&self) -> Option<i32> {
        self.std_rating.or(self.rapid_rating).or(self.blitz_rating)
    }
}

/// Ecart de classement tolere pour departager par Elo un nom de famille +
/// initiale ambigus (voir disambiguate_by_elo). Choix necessairement
/// arbitraire : assez large pour absorber la derive naturelle d'un
/// classement entre la date de la partie importee (WhiteElo/BlackElo du PGN)
/// et le classement FIDE actuel, assez etroit pour qu'il soit tres
/// improbable que deux joueurs distincts partageant deja nom de famille ET
/// initiale tombent tous les deux dedans par coincidence.
const ELO_MATCH_TOLERANCE: i32 = 150;

/// Departage un ensemble de candidats encore ambigus (meme nom de famille,
/// meme initiale) a l'aide du classement indique dans la partie importee
/// (WhiteElo/BlackElo du PGN) : ne tranche que si un seul candidat a un
/// classement FIDE a moins de ELO_MATCH_TOLERANCE de cette valeur -- comme
/// pour le reste de canonicalize_name, en cas de doute (aucun ou plusieurs
/// candidats suffisamment proches) on ne tranche pas plutot que de deviner.
fn disambiguate_by_elo(candidates: &[&NameCandidate], elo_hint: Option<i32>) -> Option<String> {
    let elo = elo_hint?;
    let mut close = candidates.iter().filter(|c| c.best_rating().is_some_and(|r| (r - elo).abs() <= ELO_MATCH_TOLERANCE));
    let first = close.next()?;
    if close.next().is_some() {
        return None;
    }
    Some(first.name.clone())
}

/// Tente de faire correspondre un nom importe (Blancs/Noirs d'un PGN, format
/// variable : "Lastname, Firstname", "Lastname,F", "Lastname, F.", ou meme
/// une abreviation de plusieurs lettres comme "Lastname, Iv") a un joueur
/// FIDE unique et non ambigu, pour le reecrire vers sa forme canonique (le
/// nom FIDE complet, tel que stocke dans fide_players) -- resout par exemple
/// "Giri,A" et "Giri, Anish" vers la meme chaine, ce qui evite a la fois un
/// doublon manque (voir db::find_duplicate, comparaison stricte sur le nom)
/// et une fragmentation du joueur en deux entrees distinctes dans
/// player_stats/l'explorateur.
///
/// `elo_hint` (WhiteElo/BlackElo de la partie importee, si present) sert
/// uniquement a departager le nom de famille + initiale quand plusieurs
/// joueurs FIDE le partagent (ex. "Giri, Anish" ~2750 vs "Giri, Andrea" tres
/// en dessous, avec "Giri,A" seul dans le PGN) -- voir disambiguate_by_elo.
///
/// Ne renvoie Some(...) que si un seul candidat FIDE correspond, au nom
/// (+ initiale) seul ou grace a l'Elo -- jamais en cas d'ambiguite
/// persistante : mieux vaut laisser deux entrees distinctes que fusionner
/// deux joueurs differents a tort.
pub fn canonicalize_name(conn: &Connection, raw_name: &str, elo_hint: Option<i32>) -> Option<String> {
    let raw_name = raw_name.trim();
    let (last, first) = raw_name.split_once(',').unwrap_or((raw_name, ""));
    let last = last.trim();
    let first = first.trim().trim_end_matches('.').trim();
    if last.is_empty() {
        return None;
    }

    // Prefixe exact sur "Lastname," (virgule incluse) : cible precisement ce
    // nom de famille, sans ramener un autre nom de famille qui partagerait
    // juste un prefixe commun (ex. "Gir" ne doit pas matcher "Girinath").
    // name est COLLATE NOCASE (voir open) : la comparaison est deja
    // insensible a la casse sans effort supplementaire ici.
    let lo = format!("{last},");
    let hi = format!("{lo}\u{ffff}");
    let mut stmt = conn
        .prepare("SELECT name, std_rating, rapid_rating, blitz_rating FROM fide_players WHERE name >= ?1 AND name < ?2")
        .ok()?;
    let candidates: Vec<NameCandidate> = stmt
        .query_map(params![lo, hi], |row| {
            Ok(NameCandidate {
                name: row.get(0)?,
                std_rating: row.get(1)?,
                rapid_rating: row.get(2)?,
                blitz_rating: row.get(3)?,
            })
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Prenom importe absent (juste "Lastname" ou "Lastname,") : n'accepte
    // que si un seul joueur FIDE porte ce nom de famille, sinon tente l'Elo.
    if first.is_empty() {
        return match candidates.len() {
            1 => Some(candidates[0].name.clone()),
            _ => disambiguate_by_elo(&candidates.iter().collect::<Vec<_>>(), elo_hint),
        };
    }

    fn fide_first_name(candidate: &str) -> Option<&str> {
        candidate.split_once(", ").map(|(_, f)| f)
    }

    // Prefixe de prenom (insensible a la casse), pas seulement une initiale
    // a une lettre ou un prenom complet exact : certaines sources PGN
    // abregent a 2-3 lettres plutot qu'a une seule (ex. "Saric, Iv" pour
    // "Ivan") -- un prenom complet reste couvert, un prefixe de sa longueur
    // totale etant trivialement lui-meme.
    let first_lower = first.to_ascii_lowercase();
    let matches: Vec<&NameCandidate> = candidates
        .iter()
        .filter(|c| fide_first_name(&c.name).is_some_and(|f| f.to_ascii_lowercase().starts_with(&first_lower)))
        .collect();

    match matches.len() {
        1 => Some(matches[0].name.clone()),
        _ => disambiguate_by_elo(&matches, elo_hint),
    }
}

fn prefix_bounds(value: &str) -> (String, String) {
    (value.to_string(), format!("{value}\u{ffff}"))
}

pub fn count_fide_players(conn: &Connection, search: &str) -> Result<u32, String> {
    let count: i64 = if search.is_empty() {
        conn.query_row("SELECT COUNT(*) FROM fide_players", [], |row| row.get(0))
    } else {
        let (lo, hi) = prefix_bounds(search);
        conn.query_row(
            "SELECT COUNT(*) FROM fide_players WHERE name >= ?1 AND name < ?2",
            params![lo, hi],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

fn map_fide_player_row(row: &rusqlite::Row) -> rusqlite::Result<FidePlayer> {
    Ok(FidePlayer {
        fide_id: row.get(0)?,
        name: row.get(1)?,
        fed: row.get(2)?,
        sex: row.get(3)?,
        title: row.get(4)?,
        w_title: row.get(5)?,
        o_title: row.get(6)?,
        std_rating: row.get(7)?,
        rapid_rating: row.get(8)?,
        blitz_rating: row.get(9)?,
        birth_year: row.get(10)?,
        flag: row.get(11)?,
    })
}

fn order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir == "desc";
    match (sort_by, desc) {
        ("name", true) => "ORDER BY name DESC",
        ("name", false) => "ORDER BY name ASC",
        ("fed", true) => "ORDER BY fed DESC, name ASC",
        ("fed", false) => "ORDER BY fed ASC, name ASC",
        ("std", true) => "ORDER BY std_rating DESC, name ASC",
        ("std", false) => "ORDER BY std_rating ASC, name ASC",
        ("rapid", true) => "ORDER BY rapid_rating DESC, name ASC",
        ("rapid", false) => "ORDER BY rapid_rating ASC, name ASC",
        ("blitz", true) => "ORDER BY blitz_rating DESC, name ASC",
        ("blitz", false) => "ORDER BY blitz_rating ASC, name ASC",
        ("birthYear", true) => "ORDER BY birth_year DESC, name ASC",
        ("birthYear", false) => "ORDER BY birth_year ASC, name ASC",
        _ => "ORDER BY std_rating DESC, name ASC",
    }
}

const SELECT_COLUMNS: &str =
    "fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag";

pub fn list_fide_players_page(
    conn: &Connection,
    search: &str,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<FidePlayer>, String> {
    let order_by = order_by_clause(sort_by, sort_dir);
    if search.is_empty() {
        let sql = format!("SELECT {SELECT_COLUMNS} FROM fide_players {order_by} LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![limit, offset], map_fide_player_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    } else {
        let (lo, hi) = prefix_bounds(search);
        let sql = format!("SELECT {SELECT_COLUMNS} FROM fide_players WHERE name >= ?1 AND name < ?2 {order_by} LIMIT ?3 OFFSET ?4");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![lo, hi, limit, offset], map_fide_player_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
}

/// Telecharge la liste FIDE (combinee Standard+Rapide+Blitz), la decompresse
/// et remplace entierement fide_players. `on_progress` est appele tous les
/// 20000 joueurs traites (pas a chaque ligne -- ~1,9M lignes au total, l'appel
/// IPC vers le frontend a un cout non negligeable a cette frequence).
/// En flux (BufReader sur l'entree du zip), jamais tout le texte (~310 Mo) en
/// memoire d'un coup -- meme raisonnement que pour l'import PGN en masse (voir
/// bulk_import::bulk_import) : la taille de la source ne doit pas dependre de
/// la RAM disponible.
pub fn refresh(app_data_dir: &Path, mut on_progress: impl FnMut(u32)) -> Result<u32, String> {
    let resp = ureq::get(DOWNLOAD_URL)
        .call()
        .map_err(|e| format!("Téléchargement de la liste FIDE échoué : {e}"))?;
    let mut zip_bytes = Vec::new();
    resp.into_reader().read_to_end(&mut zip_bytes).map_err(|e| e.to_string())?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;
    let entry = archive
        .by_name(ARCHIVE_ENTRY_NAME)
        .map_err(|e| format!("Archive FIDE inattendue (entrée {ARCHIVE_ENTRY_NAME} introuvable) : {e}"))?;
    let reader = BufReader::new(entry);

    let mut conn = open(app_data_dir)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM fide_players", []).map_err(|e| e.to_string())?;
    let mut count = 0u32;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO fide_players
                 (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(|e| e.to_string())?;
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| e.to_string())?;
            if i == 0 {
                continue; // en-tete
            }
            let Some(p) = parse_line(&line) else { continue };
            stmt.execute(params![
                p.fide_id,
                p.name,
                p.fed,
                p.sex,
                p.title,
                p.w_title,
                p.o_title,
                p.std_rating,
                p.rapid_rating,
                p.blitz_rating,
                p.birth_year,
                p.flag,
            ])
            .map_err(|e| e.to_string())?;
            count += 1;
            if count % 20000 == 0 {
                on_progress(count);
            }
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    tx.execute(
        "INSERT INTO fide_meta (key, value) VALUES ('last_updated', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![now.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    on_progress(count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_line() -> String {
        // reconstruit une ligne au bon alignement a partir des offsets reels,
        // avec les valeurs verifiees sur le vrai fichier (Magnus Carlsen)
        let mut line = " ".repeat(162);
        let put = |line: &mut String, start: usize, text: &str| {
            line.replace_range(start..start + text.len(), text);
        };
        put(&mut line, 0, "1503014");
        put(&mut line, 15, "Carlsen, Magnus");
        put(&mut line, 76, "NOR");
        put(&mut line, 80, "M");
        put(&mut line, 84, "GM");
        put(&mut line, 113, "2823");
        put(&mut line, 126, "2803");
        put(&mut line, 139, "2860");
        put(&mut line, 152, "1990");
        line
    }

    #[test]
    fn parse_line_reads_all_fields_at_the_right_offsets() {
        let p = parse_line(&sample_line()).unwrap();
        assert_eq!(p.fide_id, "1503014");
        assert_eq!(p.name, "Carlsen, Magnus");
        assert_eq!(p.fed, "NOR");
        assert_eq!(p.sex, "M");
        assert_eq!(p.title, "GM");
        assert_eq!(p.std_rating, Some(2823));
        assert_eq!(p.rapid_rating, Some(2803));
        assert_eq!(p.blitz_rating, Some(2860));
        assert_eq!(p.birth_year, Some(1990));
    }

    #[test]
    fn parse_line_treats_zero_rating_as_unrated() {
        let mut line = " ".repeat(162);
        line.replace_range(0..7, "1234567");
        line.replace_range(15..19, "Test");
        line.replace_range(113..114, "0");
        let p = parse_line(&line).unwrap();
        assert_eq!(p.std_rating, None);
    }

    #[test]
    fn parse_line_rejects_a_line_without_an_id() {
        let line = " ".repeat(162);
        assert!(parse_line(&line).is_none());
    }

    #[test]
    fn list_fide_players_page_filters_sorts_and_paginates() {
        let conn = open_at(Path::new(":memory:")).unwrap();
        let insert = |id: &str, name: &str, std: Option<i32>| {
            conn.execute(
                "INSERT INTO fide_players (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, 'FRA', 'M', '', '', '', ?3, NULL, NULL, NULL, '')",
                params![id, name, std],
            )
            .unwrap();
        };
        insert("1", "Alice", Some(2400));
        insert("2", "Bob", Some(2600));
        insert("3", "Carol", None);

        assert_eq!(count_fide_players(&conn, "").unwrap(), 3);
        let by_rating = list_fide_players_page(&conn, "", "std", "desc", 10, 0).unwrap();
        assert_eq!(by_rating[0].name, "Bob");

        let filtered = list_fide_players_page(&conn, "Ali", "", "", 10, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Alice");

        let page1 = list_fide_players_page(&conn, "", "name", "asc", 2, 0).unwrap();
        assert_eq!(page1.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob"]);
        let page2 = list_fide_players_page(&conn, "", "name", "asc", 2, 2).unwrap();
        assert_eq!(page2.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Carol"]);
    }

    fn conn_with_players(names: &[&str]) -> Connection {
        conn_with_rated_players(&names.iter().map(|n| (*n, None)).collect::<Vec<_>>())
    }

    fn conn_with_rated_players(players: &[(&str, Option<i32>)]) -> Connection {
        let conn = open_at(Path::new(":memory:")).unwrap();
        for (i, (name, std_rating)) in players.iter().enumerate() {
            conn.execute(
                "INSERT INTO fide_players (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, 'NED', 'M', '', '', '', ?3, NULL, NULL, NULL, '')",
                params![i.to_string(), name, std_rating],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn canonicalize_name_resolves_initial_to_full_fide_name() {
        let conn = conn_with_players(&["Giri, Anish"]);
        assert_eq!(canonicalize_name(&conn, "Giri,A", None), Some("Giri, Anish".to_string()));
        assert_eq!(canonicalize_name(&conn, "Giri, A.", None).as_deref(), Some("Giri, Anish"));
        // deja sous forme canonique : resolution idempotente
        assert_eq!(canonicalize_name(&conn, "Giri, Anish", None).as_deref(), Some("Giri, Anish"));
        // insensible a la casse sur le nom de famille
        assert_eq!(canonicalize_name(&conn, "giri,a", None).as_deref(), Some("Giri, Anish"));
    }

    // Cas reel qui a revele que seule une initiale a une lettre ou un prenom
    // complet exact etait gere -- "Saric, Iv" (abreviation a deux lettres,
    // certaines sources PGN font ca) ne matchait ni l'un ni l'autre et
    // restait tel quel malgre "Ivan" comme seul candidat compatible.
    #[test]
    fn canonicalize_name_resolves_multi_letter_prefix_to_full_fide_name() {
        let conn = conn_with_players(&["Saric, Ivan"]);
        assert_eq!(canonicalize_name(&conn, "Saric, Iv", None), Some("Saric, Ivan".to_string()));
        assert_eq!(canonicalize_name(&conn, "Saric,Iv", None).as_deref(), Some("Saric, Ivan"));
    }

    #[test]
    fn canonicalize_name_resolves_sole_candidate_when_first_name_missing() {
        let conn = conn_with_players(&["Giri, Anish"]);
        assert_eq!(canonicalize_name(&conn, "Giri", None), Some("Giri, Anish".to_string()));
    }

    #[test]
    fn canonicalize_name_refuses_ambiguous_initial() {
        // deux joueurs FIDE partagent le meme nom de famille ET la meme
        // initiale : ne doit jamais fusionner a tort (sans indice Elo).
        let conn = conn_with_players(&["Nguyen, Anh", "Nguyen, Anton"]);
        assert_eq!(canonicalize_name(&conn, "Nguyen,A", None), None);
        // prenom complet leve l'ambiguite
        assert_eq!(canonicalize_name(&conn, "Nguyen, Anh", None), Some("Nguyen, Anh".to_string()));
    }

    #[test]
    fn canonicalize_name_refuses_ambiguous_missing_first_name() {
        let conn = conn_with_players(&["Nguyen, Anh", "Nguyen, Anton"]);
        assert_eq!(canonicalize_name(&conn, "Nguyen", None), None);
    }

    #[test]
    fn canonicalize_name_returns_none_when_no_match() {
        let conn = conn_with_players(&["Giri, Anish"]);
        assert_eq!(canonicalize_name(&conn, "Doe, John", None), None);
        // prefixe de nom de famille partiel : ne doit pas matcher a tort
        // (ex. "Gir" ne doit pas ramener "Girinath")
        let conn2 = conn_with_players(&["Girinath, Someone"]);
        assert_eq!(canonicalize_name(&conn2, "Giri,A", None), None);
    }

    // Cas reel qui a motive l'ajout de l'indice Elo : "Giri,A" est ambigu
    // entre "Giri, Anish" (~2750) et "Giri, Andrea" (bien plus bas) -- le
    // WhiteElo/BlackElo de la partie importee permet de trancher sans avoir
    // a deviner sur le nom seul.
    #[test]
    fn canonicalize_name_disambiguates_by_elo_when_name_alone_is_ambiguous() {
        let conn = conn_with_rated_players(&[("Giri, Anish", Some(2750)), ("Giri, Andrea", Some(2100))]);
        assert_eq!(canonicalize_name(&conn, "Giri,A", None), None);
        assert_eq!(canonicalize_name(&conn, "Giri,A", Some(2748)), Some("Giri, Anish".to_string()));
        assert_eq!(canonicalize_name(&conn, "Giri,A", Some(2095)), Some("Giri, Andrea".to_string()));
        // trop loin des deux (hors tolerance) : reste ambigu, pas de choix par defaut
        assert_eq!(canonicalize_name(&conn, "Giri,A", Some(2400)), None);
    }

    #[test]
    fn canonicalize_name_elo_hint_does_not_force_a_match_when_still_ambiguous() {
        // les deux candidats ont un classement proche de l'indice : toujours
        // ambigu, ne doit pas choisir au hasard.
        let conn = conn_with_rated_players(&[("Nguyen, Anh", Some(2400)), ("Nguyen, Anton", Some(2420))]);
        assert_eq!(canonicalize_name(&conn, "Nguyen,A", Some(2410)), None);
    }
}
