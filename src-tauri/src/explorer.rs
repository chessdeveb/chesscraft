use rusqlite::{Connection, ToSql};
use serde::Serialize;
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

fn tally_rows_to_stats(rows: impl Iterator<Item = rusqlite::Result<(String, Option<String>, u32)>>) -> Result<Vec<OpeningMoveStat>, String> {
    let mut stats: BTreeMap<String, Tally> = BTreeMap::new();
    for row in rows {
        let (san, result, cnt) = row.map_err(|e| e.to_string())?;
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
const PRECOMPUTED_MAX_DEPTH: usize = 2;

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

/// Agrege les coups joues apres la sequence `moves` (prefixe de la ligne
/// principale, en SAN). En dessous de PRECOMPUTED_MAX_DEPTH, lit directement
/// la table precalculee opening_prefix_stats (instantane, voir
/// rebuild_prefix_stats) ; au-dela, ou si la table n'est pas disponible,
/// calcule a la volee. Le comptage a la volee se fait entierement en SQL
/// (GROUP BY sur le coup suivant, extrait par sous-chaine) plutot qu'en
/// rapatriant chaque partie en Rust pour la comparer une a une : sur une base
/// de plusieurs millions de parties, la difference est de l'ordre de la
/// seconde contre plusieurs dizaines de secondes. Le filtrage par prefixe
/// (LIKE 'prefixe %') s'appuie sur idx_games_mainline pour eviter un
/// balayage complet des que des coups sont deja joues.
pub fn explore(conn: &Connection, moves: &[String], filter: Option<&PlayerFilter>) -> Result<Vec<OpeningMoveStat>, String> {
    let prefix = moves.join(" ");
    if filter.is_none() && moves.len() <= PRECOMPUTED_MAX_DEPTH {
        if let Ok(stats) = precomputed_stats(conn, &prefix) {
            return Ok(stats);
        }
    }
    // Intervalle explicite plutot que LIKE 'prefixe %' : sur cette requete
    // (sous-requete + GROUP BY), l'optimisation LIKE-vers-intervalle de
    // SQLite ne se declenche pas et force un scan complet de l'index a
    // chaque appel. Un >= / < explicite, lui, est toujours converti en
    // recherche d'intervalle sur idx_games_mainline.
    let (rest_start, lo, hi) = if prefix.is_empty() {
        (1i64, String::new(), "\u{ffff}".to_string())
    } else {
        (
            prefix.len() as i64 + 2,
            format!("{prefix} "),
            format!("{prefix} ") + "\u{ffff}",
        )
    };

    // Avec un filtre joueur, le prefixe de coups (souvent vide ou tres court
    // dans un onglet de preparation qui vient de s'ouvrir) n'est en general
    // pas selectif : mainline_sans >= '' AND < '￿' matche presque toute
    // la base. La condition qui compte alors, c'est l'egalite sur white/black
    // (un seul joueur = une fraction infime de la base), d'ou l'index force
    // sur idx_games_white/idx_games_black plutot que idx_games_mainline --
    // sans ca, SQLite scanne la table entiere avant de filtrer par joueur
    // (observe en pratique : explorateur bloque plusieurs minutes sur une
    // base de reference de 10M parties).
    let (from, mut params): (String, Vec<Box<dyn ToSql>>) = match filter {
        Some(f) => {
            let index = if f.white { "idx_games_white" } else { "idx_games_black" };
            let column = if f.white { "white" } else { "black" };
            (
                format!("games INDEXED BY {index} WHERE {column} = ?1 AND"),
                vec![Box::new(f.name.clone())],
            )
        }
        None => ("games WHERE".to_string(), vec![]),
    };
    let rest_start_ph = params.len() + 1;
    let lo_ph = params.len() + 2;
    let hi_ph = params.len() + 3;
    params.push(Box::new(rest_start));
    params.push(Box::new(lo));
    params.push(Box::new(hi));

    let sql = format!(
        "SELECT
            CASE WHEN INSTR(rest, ' ') = 0 THEN rest ELSE SUBSTR(rest, 1, INSTR(rest, ' ') - 1) END AS next_move,
            result,
            COUNT(*) AS cnt
         FROM (SELECT SUBSTR(mainline_sans, ?{rest_start_ph}) AS rest, result FROM {from} mainline_sans >= ?{lo_ph} AND mainline_sans < ?{hi_ph})
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

        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], None).unwrap();
        assert_eq!(stats.len(), 2);

        let nf3 = stats.iter().find(|s| s.san == "Nf3").unwrap();
        assert_eq!(nf3.games, 2);
        assert_eq!(nf3.white_wins, 1);
        assert_eq!(nf3.black_wins, 1);

        let bc4 = stats.iter().find(|s| s.san == "Bc4").unwrap();
        assert_eq!(bc4.games, 1);
        assert_eq!(bc4.draws, 1);
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

        let filter = PlayerFilter { name: "Alice".to_string(), white: true };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], Some(&filter)).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");

        // meme prefixe precalcule (profondeur 2) mais sans filtre : les deux parties
        let all = explore(&conn, &["e4".to_string(), "e5".to_string()], None).unwrap();
        assert_eq!(all.len(), 2);
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
        let opening = explore(&conn, &[], None).unwrap();
        assert_eq!(opening.len(), 2);
        let e4 = opening.iter().find(|s| s.san == "e4").unwrap();
        assert_eq!(e4.games, 3);
        let d4 = opening.iter().find(|s| s.san == "d4").unwrap();
        assert_eq!(d4.games, 1);

        // profondeur 1 : precalcule
        let after_e4 = explore(&conn, &["e4".to_string()], None).unwrap();
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
