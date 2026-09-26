use serde::Serialize;

/// Une partie telle que necessaire au calcul de performance.
pub struct PerfGame {
    pub white: String,
    pub black: String,
    pub white_elo: Option<i32>,
    pub black_elo: Option<i32>,
    pub result: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    pub player: String,
    /// Parties prises en compte (resultat connu, Elo adverse renseigne).
    pub games: u32,
    /// Parties de la selection, y compris celles ignorees.
    pub selected: u32,
    pub points: f64,
    pub average_opponent: i32,
    pub performance: i32,
}

// Table de conversion FIDE (Handbook B.02, 8.1a) : ecart de performance dp
// pour un pourcentage de points p arrondi au centieme, de 50 % (dp = 0) a
// 100 % (dp = 800). Sous 50 %, dp est l'oppose de celui de 100 % - p.
const DP_TABLE: [i32; 51] = [
    0, 7, 14, 21, 29, 36, 43, 50, 57, 65, 72, 80, 87, 95, 102, 110, 117, 125, 133, 141, 149, 158, 166, 175, 184, 193, 202, 211, 220, 230, 240,
    251, 262, 273, 284, 296, 309, 322, 336, 351, 366, 383, 401, 422, 444, 470, 501, 538, 589, 677, 800,
];

fn rating_difference(score: f64) -> i32 {
    let percent = (score * 100.0).round().clamp(0.0, 100.0) as i32;
    if percent >= 50 {
        DP_TABLE[(percent - 50) as usize]
    } else {
        -DP_TABLE[(50 - percent) as usize]
    }
}

fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Performance Elo (methode FIDE : Elo moyen des adversaires + dp) du joueur
/// le plus present dans `games` (a egalite, le premier rencontre). Les parties
/// sans resultat decisif/nul ou sans Elo adverse sont ignorees. None si aucune
/// partie n'est exploitable.
pub fn compute(games: &[PerfGame]) -> Option<Performance> {
    let mut counts: Vec<(String, String, u32)> = Vec::new(); // (cle, nom affiche, occurrences)
    for game in games {
        for name in [&game.white, &game.black] {
            let key = normalize(name);
            if key.is_empty() {
                continue;
            }
            match counts.iter_mut().find(|(k, _, _)| *k == key) {
                Some(entry) => entry.2 += 1,
                None => counts.push((key, name.trim().to_string(), 1)),
            }
        }
    }
    let (key, display, _) = counts.iter().fold(None::<&(String, String, u32)>, |best, entry| match best {
        Some(b) if b.2 >= entry.2 => Some(b),
        _ => Some(entry),
    })?;

    let mut counted = 0u32;
    let mut points = 0.0;
    let mut opponent_sum = 0i64;
    for game in games {
        let (is_white, opponent_elo) = match (normalize(&game.white) == *key, normalize(&game.black) == *key) {
            (true, false) => (true, game.black_elo),
            (false, true) => (false, game.white_elo),
            _ => continue,
        };
        let Some(opponent_elo) = opponent_elo.filter(|e| *e > 0) else { continue };
        let score = match (game.result.as_str(), is_white) {
            ("1-0", true) | ("0-1", false) => 1.0,
            ("0-1", true) | ("1-0", false) => 0.0,
            ("1/2-1/2", _) => 0.5,
            _ => continue,
        };
        counted += 1;
        points += score;
        opponent_sum += opponent_elo as i64;
    }
    if counted == 0 {
        return None;
    }
    let average = (opponent_sum as f64 / counted as f64).round() as i32;
    Some(Performance {
        player: display.clone(),
        games: counted,
        selected: games.len() as u32,
        points,
        average_opponent: average,
        performance: average + rating_difference(points / counted as f64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(white: &str, black: &str, white_elo: Option<i32>, black_elo: Option<i32>, result: &str) -> PerfGame {
        PerfGame { white: white.to_string(), black: black.to_string(), white_elo, black_elo, result: result.to_string() }
    }

    #[test]
    fn rating_difference_matches_the_fide_table() {
        assert_eq!(rating_difference(0.5), 0);
        assert_eq!(rating_difference(0.75), 193);
        assert_eq!(rating_difference(0.25), -193);
        assert_eq!(rating_difference(1.0), 800);
        assert_eq!(rating_difference(0.0), -800);
    }

    #[test]
    fn computes_performance_for_the_most_frequent_player_on_both_colors() {
        let games = vec![
            game("Alice", "Bob", Some(2500), Some(2400), "1-0"),
            game("Carol", "Alice", Some(2600), Some(2500), "1/2-1/2"),
            game("Dave", "Alice", Some(2200), Some(2500), "0-1"),
            game("Alice", "Erin", Some(2500), Some(2000), "0-1"),
        ];
        let perf = compute(&games).unwrap();
        assert_eq!(perf.player, "Alice");
        assert_eq!(perf.games, 4);
        assert_eq!(perf.points, 2.5);
        // (2400 + 2600 + 2200 + 2000) / 4 = 2300, 62,5 % -> arrondi a 63 % : +95
        assert_eq!(perf.average_opponent, 2300);
        assert_eq!(perf.performance, 2395);
    }

    #[test]
    fn ignores_games_without_result_or_opponent_rating() {
        let games = vec![
            game("Alice", "Bob", Some(2500), Some(2400), "1-0"),
            game("Alice", "Carol", Some(2500), None, "1-0"),
            game("Alice", "Dave", Some(2500), Some(2300), "*"),
        ];
        let perf = compute(&games).unwrap();
        assert_eq!((perf.games, perf.selected), (1, 3));
        assert_eq!(perf.performance, 2400 + 800);
    }

    #[test]
    fn returns_none_when_nothing_is_usable() {
        assert_eq!(compute(&[]), None);
        assert_eq!(compute(&[game("Alice", "Bob", None, None, "1-0")]), None);
    }
}
