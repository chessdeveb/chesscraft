use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;

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

/// Agrege les coups joues apres la sequence `moves` (prefixe de la ligne
/// principale, en SAN). Le comptage se fait entierement en SQL (GROUP BY sur
/// le coup suivant, extrait par sous-chaine) plutot qu'en rapatriant chaque
/// partie en Rust pour la comparer une a une : sur une base de plusieurs
/// millions de parties, la difference est de l'ordre de la seconde contre
/// plusieurs dizaines de secondes. Le filtrage par prefixe (LIKE 'prefixe %')
/// s'appuie sur idx_games_mainline pour eviter un balayage complet des que
/// des coups sont deja joues.
pub fn explore(conn: &Connection, moves: &[String]) -> Result<Vec<OpeningMoveStat>, String> {
    let prefix = moves.join(" ");
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
}
