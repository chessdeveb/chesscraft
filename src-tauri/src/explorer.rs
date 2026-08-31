use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

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
pub fn explore(conn: &Connection, moves: &[String]) -> Result<Vec<OpeningMoveStat>, String> {
    let prefix = moves.join(" ");
    if moves.len() <= PRECOMPUTED_MAX_DEPTH {
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

    let mut stmt = conn
        .prepare(
            "SELECT
                CASE WHEN INSTR(rest, ' ') = 0 THEN rest ELSE SUBSTR(rest, 1, INSTR(rest, ' ') - 1) END AS next_move,
                result,
                COUNT(*) AS cnt
             FROM (SELECT SUBSTR(mainline_sans, ?1) AS rest, result FROM games WHERE mainline_sans >= ?2 AND mainline_sans < ?3)
             GROUP BY next_move, result",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(rusqlite::params![rest_start, lo, hi], |row| {
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

        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()]).unwrap();
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
        let opening = explore(&conn, &[]).unwrap();
        assert_eq!(opening.len(), 2);
        let e4 = opening.iter().find(|s| s.san == "e4").unwrap();
        assert_eq!(e4.games, 3);
        let d4 = opening.iter().find(|s| s.san == "d4").unwrap();
        assert_eq!(d4.games, 1);

        // profondeur 1 : precalcule
        let after_e4 = explore(&conn, &["e4".to_string()]).unwrap();
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
