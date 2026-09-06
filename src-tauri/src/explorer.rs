use rusqlite::{Connection, ToSql};
use serde::Serialize;
use shakmaty::san::San;
use shakmaty::{Chess, Position};
use std::collections::{BTreeMap, HashMap};

/// Restreint l'explorateur aux parties d'un seul joueur, dans une seule
/// couleur (preparation d'adversaire : voir lib.rs::open_player_prep_tab).
/// Un filtre actif desactive le cache precalcule opening_prefix_stats, qui
/// couvre la base entiere, pas un joueur en particulier -- sans consequence
/// sur les temps de reponse, les parties d'un seul joueur restant une
/// fraction infime de la base meme sur une reference de plusieurs millions
/// de parties.
#[derive(Clone)]
pub struct PlayerFilter {
    pub name: String,
    pub white: bool,
    /// true (preparation d'adversaire, voir lib.rs::open_player_prep_tab) :
    /// egalite exacte, `name` vient d'un joueur deja resolu dans player_stats.
    /// false (filtre ad-hoc du panneau explorateur, saisi librement au
    /// clavier) : prefixe insensible a la casse, comme partout ailleurs dans
    /// l'appli ou l'utilisateur tape un nom (recherche de parties, champs
    /// Blancs/Noirs...) -- une egalite exacte y renvoyait "aucune partie"
    /// pour a peu pres n'importe quelle saisie (le nom stocke est rarement
    /// tape caractere pour caractere en entier).
    pub exact: bool,
}

impl PlayerFilter {
    /// Fragment de condition SQL pour ce filtre (egalite exacte ou prefixe
    /// insensible a la casse, voir `exact`), poussant sur `params` les valeurs
    /// necessaires (une pour l'egalite, deux -- bornes -- pour le prefixe) et
    /// referencant leurs `?N` d'apres la position qu'elles y prennent.
    /// Partagee par explorer::explore_raw et db::position_from_and_where,
    /// qui construisent chacune leur propre requete autour.
    pub fn where_clause(&self, params: &mut Vec<Box<dyn ToSql>>) -> String {
        let column = if self.white { "white" } else { "black" };
        if self.exact {
            params.push(Box::new(self.name.clone()));
            format!("{column} = ?{}", params.len())
        } else {
            let lo = self.name.clone();
            let hi = format!("{lo}\u{ffff}");
            params.push(Box::new(lo));
            params.push(Box::new(hi));
            format!("{column} COLLATE NOCASE >= ?{} AND {column} COLLATE NOCASE < ?{}", params.len() - 1, params.len())
        }
    }
}

/// Filtres combinables de l'explorateur d'ouvertures (panneau principal, pas
/// la preparation d'adversaire qui a son propre PlayerFilter dedie -- voir
/// plus haut -- mais peut aussi en fournir un ici si l'onglet est en mode
/// preparation, voir lib.rs::opening_explorer). Un filtre actif desactive le
/// cache precalcule opening_prefix_stats (qui couvre la base entiere, pas un
/// sous-ensemble filtre) et retombe sur le calcul a la volee -- voir
/// explore_raw pour le choix de l'index force selon la condition la plus
/// selective.
#[derive(Clone, Default)]
pub struct ExplorerFilters {
    pub player: Option<PlayerFilter>,
    /// Classement (Elo standard) : ne garde que les parties ou LES DEUX
    /// joueurs sont dans l'intervalle -- une partie ou un seul camp est
    /// classe dans la fourchette demandee n'est pas ce qu'on cherche quand on
    /// etudie "les parties entre joueurs de ce niveau".
    pub rating_range: Option<(i32, i32)>,
    /// Annee de la partie (date_played, format "AAAA.MM.JJ" ou partiellement
    /// inconnu, ex. "2015.??.??") : la comparaison lexicographique existante
    /// sur cette colonne (voir idx_games_date) gere deja correctement les
    /// dates partiellement inconnues (une annee connue avec mois/jour "??"
    /// reste incluse ; une date entierement inconnue "????.??.??" trie apres
    /// toute annee reelle et est donc naturellement exclue de tout intervalle).
    pub year_range: Option<(i32, i32)>,
}

impl ExplorerFilters {
    pub fn is_empty(&self) -> bool {
        self.player.is_none() && self.rating_range.is_none() && self.year_range.is_none()
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpeningMoveStat {
    pub san: String,
    pub games: u32,
    pub white_wins: u32,
    pub draws: u32,
    pub black_wins: u32,
}

#[derive(Default)]
struct Tally {
    games: u32,
    white_wins: u32,
    draws: u32,
    black_wins: u32,
}

/// Le "+"/"#" de fin de coup (echec/mat), quand il est present dans la
/// donnee stockee, est retire ici avant de grouper : tant que le nettoyage
/// db::strip_check_suffixes n'est pas passe sur toute la base, une meme
/// suite de coups peut coexister sous deux formes ("Qxf7#" et "Qxf7") selon
/// que la partie source a deja ete nettoyee -- sans ce retrait elles
/// compteraient comme deux coups distincts. Le suffixe correct est
/// recalcule ensuite dans explore() en rejouant reellement la position
/// (voir annotate_check_suffixes), plutot que de faire confiance a celui
/// (potentiellement absent/incoherent) de la donnee source.
fn tally_rows_to_stats(rows: impl Iterator<Item = rusqlite::Result<(String, Option<String>, u32)>>) -> Result<Vec<OpeningMoveStat>, String> {
    let mut stats: BTreeMap<String, Tally> = BTreeMap::new();
    for row in rows {
        let (san, result, cnt) = row.map_err(|e| e.to_string())?;
        let san = san.trim_end_matches(['+', '#']).to_string();
        if san.is_empty() {
            continue;
        }
        let entry = stats.entry(san).or_default();
        entry.games += cnt;
        match result.as_deref() {
            Some("1-0") => entry.white_wins += cnt,
            Some("1/2-1/2") => entry.draws += cnt,
            Some("0-1") => entry.black_wins += cnt,
            _ => {}
        }
    }
    let mut out: Vec<OpeningMoveStat> = stats
        .into_iter()
        .map(|(san, tally)| OpeningMoveStat {
            san,
            games: tally.games,
            white_wins: tally.white_wins,
            draws: tally.draws,
            black_wins: tally.black_wins,
        })
        .collect();
    out.sort_by(|a, b| b.games.cmp(&a.games));
    Ok(out)
}

/// Profondeur (en demi-coups) jusqu'a laquelle les stats sont precalculees
/// dans opening_prefix_stats. Au-dela, une partie de plus en plus petite de
/// la base correspond au prefixe : la requete a la volee (voir plus bas)
/// reste rapide sans avoir besoin d'etre precalculee.
pub(crate) const PRECOMPUTED_MAX_DEPTH: usize = 2;

/// "*" pour un resultat manquant/vide : opening_prefix_stats.result est NOT
/// NULL (necessaire pour la contrainte d'unicite utilisee par les mises a
/// jour incrementales, voir increment_prefix_stats) -- NULL n'est jamais
/// egal a NULL dans un index unique SQLite, deux parties sans resultat y
/// creeraient donc chacune leur propre ligne au lieu de partager le meme
/// compteur.
fn normalize_result(result: Option<&str>) -> String {
    match result {
        Some(r) if !r.is_empty() => r.to_string(),
        _ => "*".to_string(),
    }
}

/// (prefixe, coup suivant) pour chaque profondeur precalculee (0 a
/// PRECOMPUTED_MAX_DEPTH inclus) presente dans cette ligne principale.
fn prefixes_for_mainline(mainline_sans: &str) -> Vec<(String, String)> {
    let tokens: Vec<&str> = if mainline_sans.is_empty() { Vec::new() } else { mainline_sans.split(' ').collect() };
    let mut out = Vec::new();
    for depth in 0..=PRECOMPUTED_MAX_DEPTH {
        if tokens.len() <= depth {
            break;
        }
        out.push((tokens[..depth].join(" "), tokens[depth].to_string()));
    }
    out
}

/// A appeler juste apres l'ajout d'une partie (save_game) : ajuste
/// opening_prefix_stats sans tout recalculer, pour que la table reste a jour
/// meme en dehors d'un import en masse (qui, lui, refait un
/// rebuild_prefix_stats complet).
pub fn increment_prefix_stats(conn: &Connection, mainline_sans: &str, result: Option<&str>) -> Result<(), String> {
    let result = normalize_result(result);
    for (prefix, next_move) in prefixes_for_mainline(mainline_sans) {
        conn.execute(
            "INSERT INTO opening_prefix_stats (prefix, next_move, result, cnt) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(prefix, next_move, result) DO UPDATE SET cnt = cnt + 1",
            rusqlite::params![prefix, next_move, result],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A appeler juste avant la suppression d'une partie, ou juste avant de
/// remplacer son contenu (update_game) -- symetrique de
/// increment_prefix_stats. Les lignes qui retombent a zero sont supprimees
/// plutot que laissees a 0, pour eviter d'accumuler des lignes mortes au fil
/// des suppressions.
pub fn decrement_prefix_stats(conn: &Connection, mainline_sans: &str, result: Option<&str>) -> Result<(), String> {
    let result = normalize_result(result);
    for (prefix, next_move) in prefixes_for_mainline(mainline_sans) {
        conn.execute(
            "UPDATE opening_prefix_stats SET cnt = cnt - 1 WHERE prefix = ?1 AND next_move = ?2 AND result = ?3 AND cnt > 0",
            rusqlite::params![prefix, next_move, result],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute("DELETE FROM opening_prefix_stats WHERE cnt <= 0", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Lit les stats precalculees pour un prefixe donne. Erreur (table absente,
/// base pas encore migree) plutot que resultat vide "authentique" : la
/// distinction est geree par l'appelant, qui retombe alors sur la requete a
/// la volee.
fn precomputed_stats(conn: &Connection, prefix: &str) -> Result<Vec<OpeningMoveStat>, String> {
    let mut stmt = conn
        .prepare("SELECT next_move, result, cnt FROM opening_prefix_stats WHERE prefix = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![prefix], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, u32>(2)?))
    });
    match rows {
        Ok(rows) => tally_rows_to_stats(rows),
        Err(e) => Err(e.to_string()),
    }
}

/// Reconstruit entierement la table opening_prefix_stats a partir des
/// parties actuelles : necessaire apres un import en masse, ou pour la
/// premiere ouverture d'une base importee avant l'ajout de cette table (voir
/// migrate_prefix_stats cote db.rs). Un seul passage sur mainline_sans,
/// tokenise en Rust (pas de re-parsing des coups), meme cout qu'un COUNT(*)
/// sur la base -- de l'ordre de la seconde a quelques secondes sur plusieurs
/// millions de parties, contre 10 a 20s par requete a la volee sur les tout
/// premiers coups.
pub fn rebuild_prefix_stats(conn: &mut Connection) -> Result<(), String> {
    let tally = {
        let mut stmt = conn
            .prepare("SELECT mainline_sans, result FROM games WHERE mainline_sans IS NOT NULL AND mainline_sans <> ''")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))
            .map_err(|e| e.to_string())?;

        let mut tally: HashMap<(String, String, String), u64> = HashMap::new();
        for row in rows {
            let (mainline, result) = row.map_err(|e| e.to_string())?;
            let result = normalize_result(result.as_deref());
            for (prefix, next_move) in prefixes_for_mainline(&mainline) {
                *tally.entry((prefix, next_move, result.clone())).or_insert(0) += 1;
            }
        }
        tally
    };

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM opening_prefix_stats", []).map_err(|e| e.to_string())?;
    {
        let mut insert = tx
            .prepare("INSERT INTO opening_prefix_stats (prefix, next_move, result, cnt) VALUES (?1, ?2, ?3, ?4)")
            .map_err(|e| e.to_string())?;
        for ((prefix, next_move, result), cnt) in &tally {
            insert
                .execute(rusqlite::params![prefix, next_move, result, *cnt as i64])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// Position atteinte en rejouant `moves` (SAN sans suffixe, tel qu'envoye
/// par le frontend) depuis la position de depart -- sert a recalculer
/// fiablement le "+"/"#" de chaque coup suivant (voir annotate_check_suffixes)
/// plutot que de faire confiance a celui, potentiellement absent/incoherent,
/// de mainline_sans. None si un coup ne se rejoue pas (ne devrait pas arriver
/// avec des donnees internes a l'appli, mais mieux vaut ne pas annoter du
/// tout que de planter l'explorateur pour ca).
fn replay_position(moves: &[String]) -> Option<Chess> {
    let mut position = Chess::default();
    for m in moves {
        let san: San = m.parse().ok()?;
        let mv = san.to_move(&position).ok()?;
        position = position.play(mv).ok()?;
    }
    Some(position)
}

/// Recalcule le "+"/"#" de chaque coup suivant en le rejouant reellement
/// depuis `position`, plutot que de dependre de mainline_sans (voir
/// tally_rows_to_stats, qui l'a deja retire a ce stade) -- fiable quel que
/// soit l'etat du nettoyage db::strip_check_suffixes sur la base source.
/// Ignore silencieusement un coup qui ne se rejoue pas (voir replay_position).
fn annotate_check_suffixes(stats: &mut [OpeningMoveStat], position: &Chess) {
    for stat in stats.iter_mut() {
        let Ok(san) = stat.san.parse::<San>() else { continue };
        let Ok(mv) = san.to_move(position) else { continue };
        let Ok(new_position) = position.clone().play(mv) else { continue };
        if new_position.is_checkmate() {
            stat.san.push('#');
        } else if new_position.is_check() {
            stat.san.push('+');
        }
    }
}

/// Agrege les coups joues apres la sequence `moves` (prefixe de la ligne
/// principale, en SAN, sans suffixe) et resout leur "+"/"#" par recalcul
/// (voir replay_position/annotate_check_suffixes) plutot que par la donnee
/// stockee, potentiellement incoherente/absente selon l'etat du nettoyage
/// db::strip_check_suffixes sur la base source.
pub fn explore(conn: &Connection, moves: &[String], filters: &ExplorerFilters) -> Result<Vec<OpeningMoveStat>, String> {
    let mut stats = explore_raw(conn, moves, filters)?;
    if let Some(position) = replay_position(moves) {
        annotate_check_suffixes(&mut stats, &position);
    }
    Ok(stats)
}

/// En dessous de PRECOMPUTED_MAX_DEPTH et sans filtre, lit directement la
/// table precalculee opening_prefix_stats (instantane, voir
/// rebuild_prefix_stats) ; au-dela, ou des qu'un filtre est actif, ou si la
/// table n'est pas disponible, calcule a la volee. Le comptage a la volee se
/// fait entierement en SQL (GROUP BY sur le coup suivant, extrait par
/// sous-chaine) plutot qu'en rapatriant chaque partie en Rust pour la
/// comparer une a une : sur une base de plusieurs millions de parties, la
/// difference est de l'ordre de la seconde contre plusieurs dizaines de
/// secondes.
fn explore_raw(conn: &Connection, moves: &[String], filters: &ExplorerFilters) -> Result<Vec<OpeningMoveStat>, String> {
    let prefix = moves.join(" ");
    if filters.is_empty() && moves.len() <= PRECOMPUTED_MAX_DEPTH {
        if let Ok(stats) = precomputed_stats(conn, &prefix) {
            return Ok(stats);
        }
    }
    // Intervalle explicite plutot que LIKE 'prefixe %' : sur cette requete
    // (sous-requete + GROUP BY), l'optimisation LIKE-vers-intervalle de
    // SQLite ne se declenche pas et force un scan complet de l'index a
    // chaque appel. Un >= / < explicite, lui, est toujours converti en
    // recherche d'intervalle sur idx_games_mainline.
    //
    // Pas d'espace impose juste apres le prefixe (contrairement a une version
    // anterieure) : une partie de reference dont le dernier coup du prefixe
    // donnait echec/mat peut encore avoir "+"/"# " colle juste apres ce coup
    // dans mainline_sans (import anterieur a la correction de bulk_import, ou
    // nettoyage db::strip_check_suffixes pas encore passe sur cette ligne) --
    // l'exiger ecartait ces parties des stats alors que games_at_position
    // (db.rs), moins strict, les trouvait deja. Meme raisonnement que
    // position_from_and_where (db.rs) : "+"/"#" ne suivent jamais rien
    // d'autre qu'un coup complet en SAN, donc aucun risque de faux positif a
    // etre tolerant ici.
    let (rest_start, lo, hi) = if prefix.is_empty() {
        (1i64, String::new(), "\u{ffff}".to_string())
    } else {
        (prefix.len() as i64 + 1, prefix.clone(), format!("{prefix}\u{ffff}"))
    };

    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();
    params.push(Box::new(lo));
    params.push(Box::new(hi));
    clauses.push(format!("mainline_sans >= ?{} AND mainline_sans < ?{}", params.len() - 1, params.len()));

    if let Some(p) = &filters.player {
        clauses.push(p.where_clause(&mut params));
    }
    if let Some((min, max)) = filters.rating_range {
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("white_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("black_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
    }
    if let Some((year_min, year_max)) = filters.year_range {
        params.push(Box::new(format!("{year_min:04}")));
        params.push(Box::new(format!("{year_max:04}\u{ffff}")));
        clauses.push(format!("date_played >= ?{} AND date_played < ?{}", params.len() - 1, params.len()));
    }

    // Choix de l'index a forcer : SEUL le filtre joueur en force un. Un
    // joueur reste une fraction infime de la base meme sur plusieurs millions
    // de parties (sans forcer idx_games_white/black, SQLite scannerait
    // idx_games_mainline en entier avant de filtrer -- observe en pratique,
    // voir plus haut dans l'historique de ce fichier). Classement et annee,
    // eux, NE sont PAS forcement selectifs : un intervalle de 2-3 ans sur une
    // base couvrant plusieurs decennies peut representer des centaines de
    // milliers de parties -- forcer idx_games_date dans ce cas (essaye dans
    // une version anterieure) obligeait SQLite a aller relire mainline_sans
    // ligne par ligne pour chacune, mesure catastrophiquement lent en
    // pratique. Ils restent donc de simples conditions residuelles, evaluees
    // sur les lignes deja retenues par l'index choisi pour une autre raison
    // (joueur, ou a defaut mainline_sans) plutot que de piloter eux-memes le
    // choix d'index.
    let index = match &filters.player {
        Some(p) => Some(if p.white { "idx_games_white" } else { "idx_games_black" }),
        None => None,
    };
    let from = match index {
        Some(index) => format!("games INDEXED BY {index}"),
        None => "games".to_string(),
    };

    let rest_start_ph = params.len() + 1;
    params.push(Box::new(rest_start));

    let where_sql = clauses.join(" AND ");

    // Le "+"/"#" eventuel du coup retourne n'est pas retire ici : voir
    // tally_rows_to_stats (regroupement) et annotate_check_suffixes
    // (recalcul fiable), qui s'en chargent cote Rust une fois les lignes
    // rapatriees.
    let sql = format!(
        "SELECT
            CASE WHEN INSTR(rest, ' ') = 0 THEN rest ELSE SUBSTR(rest, 1, INSTR(rest, ' ') - 1) END AS next_move,
            result,
            COUNT(*) AS cnt
         FROM (SELECT LTRIM(SUBSTR(mainline_sans, ?{rest_start_ph}), '+# ') AS rest, result FROM {from} WHERE {where_sql})
         GROUP BY next_move, result"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            let next_move: String = row.get(0)?;
            let result: Option<String> = row.get(1)?;
            let cnt: u32 = row.get(2)?;
            Ok((next_move, result, cnt))
        })
        .map_err(|e| e.to_string())?;

    tally_rows_to_stats(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn base_tags_for_explorer(white: &str, black: &str, date: &str, result: &str) -> BTreeMap<String, String> {
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), white.to_string());
        tags.insert("Black".to_string(), black.to_string());
        tags.insert("Date".to_string(), date.to_string());
        tags.insert("Result".to_string(), result.to_string());
        tags
    }

    fn conn_with_games(games: &[(&str, &str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );",
        )
        .unwrap();
        for (white, mainline, result) in games {
            let mut tags = BTreeMap::new();
            tags.insert("White".to_string(), white.to_string());
            tags.insert("Result".to_string(), result.to_string());
            db::save_game(&conn, "1. e4 *", &tags, mainline).unwrap();
        }
        conn
    }

    #[test]
    fn aggregates_next_move_after_prefix() {
        let conn = conn_with_games(&[
            ("Alice", "e4 e5 Nf3", "1-0"),
            ("Bob", "e4 e5 Nf3", "0-1"),
            ("Carol", "e4 e5 Bc4", "1/2-1/2"),
            ("Dave", "d4 d5", "1-0"), // ne correspond pas au prefixe
        ]);

        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &ExplorerFilters::default()).unwrap();
        assert_eq!(stats.len(), 2);

        let nf3 = stats.iter().find(|s| s.san == "Nf3").unwrap();
        assert_eq!(nf3.games, 2);
        assert_eq!(nf3.white_wins, 1);
        assert_eq!(nf3.black_wins, 1);

        let bc4 = stats.iter().find(|s| s.san == "Bc4").unwrap();
        assert_eq!(bc4.games, 1);
        assert_eq!(bc4.draws, 1);
    }

    // Cas reel rapporte par l'utilisateur : apres "e4 c5 Nf3 d6 Bb5+", les
    // reponses Nc6/Bd7 manquaient completement du tableau de stats (tout en
    // restant trouvables via games_at_position, moins strict) des que la
    // partie de reference gardait encore le "+" de Bb5+ dans mainline_sans
    // (import anterieur a la correction de bulk_import, ou nettoyage
    // db::strip_check_suffixes pas encore passe sur cette ligne).
    #[test]
    fn finds_continuations_when_the_last_prefix_move_still_has_a_check_suffix() {
        let conn = conn_with_games(&[
            ("Alice", "e4 c5 Nf3 d6 Bb5+ Nd7", "1-0"), // deja nettoyee
            ("Bob", "e4 c5 Nf3 d6 Bb5+ Nc6", "0-1"),   // pas encore nettoyee
            ("Carol", "e4 c5 Nf3 d6 Bb5+ Bd7", "1/2-1/2"), // pas encore nettoyee
        ]);

        let stats = explore(
            &conn,
            &["e4".to_string(), "c5".to_string(), "Nf3".to_string(), "d6".to_string(), "Bb5".to_string()],
            &ExplorerFilters::default(),
        )
        .unwrap();

        assert_eq!(stats.iter().map(|s| s.san.as_str()).collect::<std::collections::HashSet<_>>(), ["Nd7", "Nc6", "Bd7"].into());
    }

    // Meme scenario que ci-dessus, mais sur le coup RETOURNE (pas celui du
    // prefixe deja joue) : deux parties qui continuent par le meme coup
    // Qxf7, l'une encore annotee "Qxf7#" (pas nettoyee -- "#" fictif ici,
    // juste pour simuler une annotation source perimee/incoherente, cette
    // position n'est pas reellement mat), l'autre deja "Qxf7" (nettoyee), ne
    // doivent pas apparaitre comme deux coups distincts dans le tableau --
    // sans quoi le nombre de parties/l'ordre d'affichage deviennent
    // incoherents tant que le nettoyage n'est pas termine partout. Le "+"
    // affiche est recalcule (voir annotate_check_suffixes), pas copie de la
    // donnee source -- Qxf7 met bien le roi noir en echec ici, sans etre mat.
    #[test]
    fn merges_check_and_clean_variants_of_the_same_next_move() {
        let conn = conn_with_games(&[
            ("Alice", "e4 e5 Qh5 Nc6 Qxf7#", "1-0"), // pas encore nettoyee
            ("Bob", "e4 e5 Qh5 Nc6 Qxf7", "1-0"),    // deja nettoyee
        ]);

        let stats =
            explore(&conn, &["e4".to_string(), "e5".to_string(), "Qh5".to_string(), "Nc6".to_string()], &ExplorerFilters::default()).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qxf7+");
        assert_eq!(stats[0].games, 2);
    }

    // Position reellement mat (le "Fool's Mate"), pour verifier que le
    // recalcul distingue bien "#" de "+" plutot que d'apposer bêtement un
    // suffixe generique.
    #[test]
    fn annotates_real_checkmate_with_hash_not_plus() {
        let conn = conn_with_games(&[("Alice", "f3 e5 g4 Qh4", "0-1")]);

        let stats = explore(&conn, &["f3".to_string(), "e5".to_string(), "g4".to_string()], &ExplorerFilters::default()).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qh4#");
    }

    #[test]
    fn explore_with_player_filter_only_considers_that_player() {
        // db::open (pas conn_with_games) : le filtre joueur force
        // idx_games_white/idx_games_black, absents du schema minimal utilise
        // par les autres tests de ce module.
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        let filters = ExplorerFilters { player: Some(PlayerFilter { name: "Alice".to_string(), white: true, exact: true }), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");

        // meme prefixe precalcule (profondeur 2) mais sans filtre : les deux parties
        let all = explore(&conn, &["e4".to_string(), "e5".to_string()], &ExplorerFilters::default()).unwrap();
        assert_eq!(all.len(), 2);
    }

    // Cas reel rapporte par l'utilisateur : le filtre joueur ad-hoc du
    // panneau explorateur (exact: false) est tape librement au clavier, pas
    // choisi dans une liste deja resolue comme pour la preparation
    // d'adversaire (exact: true) -- une egalite exacte n'y matchait donc
    // presque jamais rien ("n'importe quel nom -> aucune partie").
    #[test]
    fn explore_with_non_exact_player_filter_matches_by_case_insensitive_prefix() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Carlsen, Magnus".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        // recherche partielle, casse differente : doit tout de meme trouver
        // "Carlsen, Magnus" (exact: true echouerait ici).
        let filters =
            ExplorerFilters { player: Some(PlayerFilter { name: "carlsen".to_string(), white: true, exact: false }), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");
    }

    #[test]
    fn explore_with_rating_range_only_considers_games_where_both_players_are_in_range() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "2450".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        // un seul camp dans l'intervalle demande : ne doit pas compter
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "1500".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        let filters = ExplorerFilters { rating_range: Some((2300, 2500)), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");
    }

    #[test]
    fn explore_with_year_range_only_considers_games_in_that_range() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags_for_explorer("Alice", "Carol", "2015.06.01", "1-0");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        tags = base_tags_for_explorer("Bob", "Dave", "1990.01.01", "0-1");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();
        // date partiellement inconnue mais annee connue : doit rester incluse
        tags = base_tags_for_explorer("Eve", "Frank", "2018.??.??", "1/2-1/2");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Qh5").unwrap();

        let filters = ExplorerFilters { year_range: Some((2010, 2020)), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters).unwrap();
        assert_eq!(stats.iter().map(|s| s.san.as_str()).collect::<std::collections::HashSet<_>>(), ["Nf3", "Qh5"].into());
    }

    #[test]
    fn rebuild_prefix_stats_matches_on_the_fly_computation() {
        let mut conn = conn_with_games(&[
            ("Alice", "e4 e5 Nf3", "1-0"),
            ("Bob", "e4 e5 Nf3", "0-1"),
            ("Carol", "e4 c5 Nf3", "1/2-1/2"),
            ("Dave", "d4 d5", "1-0"),
        ]);
        conn.execute_batch(
            "CREATE TABLE opening_prefix_stats (
                prefix TEXT NOT NULL, next_move TEXT NOT NULL, result TEXT, cnt INTEGER NOT NULL
            );",
        )
        .unwrap();

        rebuild_prefix_stats(&mut conn).unwrap();

        // profondeur 0 (position de depart) : precalcule
        let opening = explore(&conn, &[], &ExplorerFilters::default()).unwrap();
        assert_eq!(opening.len(), 2);
        let e4 = opening.iter().find(|s| s.san == "e4").unwrap();
        assert_eq!(e4.games, 3);
        let d4 = opening.iter().find(|s| s.san == "d4").unwrap();
        assert_eq!(d4.games, 1);

        // profondeur 1 : precalcule
        let after_e4 = explore(&conn, &["e4".to_string()], &ExplorerFilters::default()).unwrap();
        assert_eq!(after_e4.len(), 2);
        let e5 = after_e4.iter().find(|s| s.san == "e5").unwrap();
        assert_eq!(e5.games, 2);
        assert_eq!(e5.white_wins, 1);
        assert_eq!(e5.black_wins, 1);
        let c5 = after_e4.iter().find(|s| s.san == "c5").unwrap();
        assert_eq!(c5.games, 1);
        assert_eq!(c5.draws, 1);
    }
}
