use crate::db;
use crate::pgn;
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
/// la sequence `moves` (prefixe de la ligne principale, en SAN).
pub fn explore(conn: &Connection, moves: &[String]) -> Result<Vec<OpeningMoveStat>, String> {
    let games = db::all_games_pgn(conn)?;
    let mut stats: BTreeMap<String, Tally> = BTreeMap::new();

    for (pgn_text, result) in games {
        let Ok(tree) = pgn::parse_game(&pgn_text) else {
            continue;
        };
        let mainline = tree.mainline_sans();
        if mainline.len() <= moves.len() || mainline[..moves.len()] != moves[..] {
            continue;
        }

        let entry = stats.entry(mainline[moves.len()].clone()).or_default();
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
