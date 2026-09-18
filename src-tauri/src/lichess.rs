/// Cadences Lichess proposees par le dialogue d'import (voir
/// lib.rs::import_lichess_games) -- valeurs exactes attendues par le
/// parametre perfType de l'API Lichess
/// (https://lichess.org/api#tag/Games/operation/apiGamesUser).
pub const PERF_TYPES: [&str; 3] = ["bullet", "blitz", "rapid"];

/// Un pseudo Lichess n'autorise que lettres/chiffres/tiret/underscore --
/// rejette tout le reste ici plutot que de laisser une valeur inattendue
/// (espace, "/", "..") finir directement dans une URL.
pub fn valid_username(username: &str) -> bool {
    !username.is_empty() && username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Construit l'URL d'export PGN Lichess (API publique, aucun jeton requis)
/// pour un pseudo, une periode (millisecondes epoch UTC, bornes incluses) et
/// un sous-ensemble de cadences. moves/tags/clocks/evals/opening actives :
/// recupere tout ce que l'appli sait deja exploiter a l'import (horloge
/// "[%clk]", evaluation "[%eval]" -- voir apply_game_report -- ouverture ECO).
pub fn export_url(username: &str, since_ms: Option<i64>, until_ms: Option<i64>, perf_types: &[String]) -> String {
    let mut url =
        format!("https://lichess.org/api/games/user/{username}?moves=true&tags=true&clocks=true&evals=true&opening=true");
    if !perf_types.is_empty() {
        url.push_str("&perfType=");
        url.push_str(&perf_types.join(","));
    }
    if let Some(since) = since_ms {
        url.push_str(&format!("&since={since}"));
    }
    if let Some(until) = until_ms {
        url.push_str(&format!("&until={until}"));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_username_accepts_letters_digits_dash_underscore() {
        assert!(valid_username("DrNykterstein"));
        assert!(valid_username("foo_bar-42"));
    }

    #[test]
    fn valid_username_rejects_empty_or_unsafe_characters() {
        assert!(!valid_username(""));
        assert!(!valid_username("foo/bar"));
        assert!(!valid_username("foo bar"));
        assert!(!valid_username("../../etc"));
    }

    #[test]
    fn export_url_includes_perf_types_and_period_when_given() {
        let url = export_url(
            "DrNykterstein",
            Some(1_700_000_000_000),
            Some(1_705_000_000_000),
            &["bullet".to_string(), "blitz".to_string()],
        );
        assert!(url.starts_with("https://lichess.org/api/games/user/DrNykterstein?"));
        assert!(url.contains("perfType=bullet,blitz"));
        assert!(url.contains("since=1700000000000"));
        assert!(url.contains("until=1705000000000"));
    }

    #[test]
    fn export_url_omits_absent_period_bounds_and_perf_filter() {
        let url = export_url("foo", None, None, &[]);
        assert!(!url.contains("since="));
        assert!(!url.contains("until="));
        assert!(!url.contains("perfType="));
    }
}
