use crate::db;
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

/// Parcourt toutes les parties de la base et agrege les coups joues apres
/// la sequence `moves` (prefixe de la ligne principale, en SAN). Compare la
/// liste de coups mise en cache (colonne mainline_sans) plutot que de
/// reparser chaque partie coup par coup : indispensable pour rester rapide
/// sur une base de plusieurs centaines de milliers de parties.
pub fn explore(conn: &Connection, moves: &[String]) -> Result<Vec<OpeningMoveStat>, String> {
    let games = db::all_games_mainline(conn)?;
    let mut stats: BTreeMap<String, Tally> = BTreeMap::new();

    for (mainline_sans, result) in games {
        let mainline: Vec<&str> = if mainline_sans.is_empty() {
            Vec::new()
        } else {
            mainline_sans.split(' ').collect()
        };
        if mainline.len() <= moves.len() || !mainline.iter().zip(moves.iter()).all(|(a, b)| *a == b.as_str()) {
            continue;
        }

        let entry = stats.entry(mainline[moves.len()].to_string()).or_default();
        entry.games += 1;
        match result.as_deref() {
            Some("1-0") => entry.white_wins += 1,
            Some("1/2-1/2") => entry.draws += 1,
            Some("0-1") => entry.black_wins += 1,
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
