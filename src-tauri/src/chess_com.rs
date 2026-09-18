use serde_json::Value;

// Cote cadences, chess.com classe deja chaque partie sous un champ
// "time_class" qui vaut litteralement "bullet" / "blitz" / "rapid" / "daily"
// (voir download_pgn) -- exactement les valeurs de lichess::PERF_TYPES, donc
// pas de constante equivalente ici : lib.rs::get_lichess_perf_types (seule
// commande de cadences exposee au frontend) sert deja les deux dialogues.

/// Meme regle que pour un pseudo Lichess : uniquement lettres/chiffres/tiret/
/// underscore, pour ne jamais laisser une valeur inattendue finir dans une URL.
pub fn valid_username(username: &str) -> bool {
    !username.is_empty() && username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// chess.com demande un User-Agent identifiable (sinon reponses degradees/bloquees) --
// voir https://www.chess.com/news/view/published-data-api etiquette.
const USER_AGENT: &str = "chesscraft (+https://github.com/chessdeveb/chesscraft)";

fn get_json(url: &str) -> Result<Value, String> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("chess.com : {e}"))?;
    serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())
}

/// URLs des archives mensuelles d'un joueur
/// ("https://api.chess.com/pub/player/USER/games/YYYY/MM"), dans l'ordre
/// chronologique fourni par l'API.
fn archive_urls(username: &str) -> Result<Vec<String>, String> {
    let url = format!("https://api.chess.com/pub/player/{}/games/archives", username.to_lowercase());
    let json = get_json(&url)?;
    let archives = json
        .get("archives")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "réponse chess.com inattendue (archives)".to_string())?;
    Ok(archives.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
}

/// Annee/mois extraits de la fin d'une URL d'archive ("(...)/YYYY/MM").
fn archive_year_month(url: &str) -> Option<(i64, u32)> {
    let mut parts = url.rsplit('/');
    let month: u32 = parts.next()?.parse().ok()?;
    let year: i64 = parts.next()?.parse().ok()?;
    Some((year, month))
}

/// Jours ecoules depuis l'epoque Unix (1970-01-01) pour une date civile
/// donnee -- algorithme de Howard Hinnant (domaine public,
/// http://howardhinnant.github.io/date_algorithms.html), pour eviter une
/// dependance a un crate date/heure pour ce seul besoin : comparer le mois
/// d'une archive chess.com a la periode demandee (voir month_overlaps_range).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_start_epoch_ms(year: i64, month: u32) -> i64 {
    days_from_civil(year, month, 1) * 86_400_000
}

/// Vrai si le mois (annee, mois) peut contenir des parties dans
/// [since_ms, until_ms] (bornes optionnelles, incluses) -- evite de
/// telecharger des mois hors periode.
fn month_overlaps_range(year: i64, month: u32, since_ms: Option<i64>, until_ms: Option<i64>) -> bool {
    let month_start = month_start_epoch_ms(year, month);
    let next_month_start = if month == 12 {
        month_start_epoch_ms(year + 1, 1)
    } else {
        month_start_epoch_ms(year, month + 1)
    };
    if let Some(until) = until_ms {
        if month_start > until {
            return false;
        }
    }
    if let Some(since) = since_ms {
        if next_month_start <= since {
            return false;
        }
    }
    true
}

/// Telecharge et concatene en un seul texte PGN les parties d'un joueur
/// chess.com correspondant a la periode et aux cadences donnees -- pret a
/// etre passe a bulk_import::bulk_import (voir lib.rs::import_chesscom_games),
/// exactement comme le PGN telecharge depuis Lichess.
pub fn download_pgn(username: &str, since_ms: Option<i64>, until_ms: Option<i64>, perf_types: &[String]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for archive_url in archive_urls(username)? {
        let Some((year, month)) = archive_year_month(&archive_url) else { continue };
        if !month_overlaps_range(year, month, since_ms, until_ms) {
            continue;
        }
        let json = get_json(&archive_url)?;
        let Some(games) = json.get("games").and_then(|v| v.as_array()) else { continue };
        for game in games {
            let Some(time_class) = game.get("time_class").and_then(|v| v.as_str()) else { continue };
            if !perf_types.iter().any(|p| p == time_class) {
                continue;
            }
            if let Some(end_time) = game.get("end_time").and_then(|v| v.as_i64()) {
                let end_ms = end_time * 1000;
                if until_ms.is_some_and(|until| end_ms > until) {
                    continue;
                }
                if since_ms.is_some_and(|since| end_ms < since) {
                    continue;
                }
            }
            if let Some(pgn) = game.get("pgn").and_then(|v| v.as_str()) {
                out.extend_from_slice(pgn.as_bytes());
                out.push(b'\n');
                out.push(b'\n');
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_username_accepts_letters_digits_dash_underscore() {
        assert!(valid_username("Hikaru"));
        assert!(valid_username("foo_bar-42"));
    }

    #[test]
    fn valid_username_rejects_empty_or_unsafe_characters() {
        assert!(!valid_username(""));
        assert!(!valid_username("foo/bar"));
        assert!(!valid_username("../../etc"));
    }

    #[test]
    fn archive_year_month_parses_the_trailing_year_and_month() {
        assert_eq!(archive_year_month("https://api.chess.com/pub/player/foo/games/2024/03"), Some((2024, 3)));
        assert_eq!(archive_year_month("not a url"), None);
    }

    #[test]
    fn days_from_civil_matches_known_epoch_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2024, 3, 1), 19783);
    }

    #[test]
    fn month_overlaps_range_keeps_months_touching_the_period() {
        // periode : 2024-02-15 .. 2024-03-10 (en ms epoch)
        let since = month_start_epoch_ms(2024, 2) + 14 * 86_400_000;
        let until = month_start_epoch_ms(2024, 3) + 9 * 86_400_000;
        assert!(!month_overlaps_range(2024, 1, Some(since), Some(until)));
        assert!(month_overlaps_range(2024, 2, Some(since), Some(until)));
        assert!(month_overlaps_range(2024, 3, Some(since), Some(until)));
        assert!(!month_overlaps_range(2024, 4, Some(since), Some(until)));
    }

    #[test]
    fn month_overlaps_range_accepts_everything_when_no_bounds_given() {
        assert!(month_overlaps_range(1999, 1, None, None));
        assert!(month_overlaps_range(2030, 12, None, None));
    }
}
