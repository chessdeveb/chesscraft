use serde::Deserialize;
use serde::de::DeserializeOwned;

/// ureq ne lit du JSON que si la feature "json" est activee ; plutot que
/// d'etendre la dependance ureq pour ca, on passe par serde_json (deja une
/// dependance directe du projet) sur le corps texte de la reponse.
fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let body = ureq::get(url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// Sous-ensemble des champs Lichess utilises ici -- serde ignore
/// silencieusement tout champ present dans la reponse mais absent de ces
/// structs (pas de deny_unknown_fields), donc ces types restent valides meme
/// si Lichess ajoute des champs par la suite.
#[derive(Deserialize, Clone)]
pub struct BroadcastTour {
    pub id: String,
    pub name: String,
    /// [debut, fin] en millisecondes epoque UTC -- absent pour un tournoi
    /// sans dates renseignees (rare), voir date_range_overlaps.
    #[serde(default)]
    pub dates: Option<[i64; 2]>,
}

#[derive(Deserialize, Clone)]
pub struct BroadcastSearchResult {
    pub tour: BroadcastTour,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BroadcastSearchPage {
    current_page_results: Vec<BroadcastSearchResult>,
    next_page: Option<u32>,
}

/// Recherche de tournois par mot-cle, y compris anciens/termines (verifie
/// manuellement : contrairement a /api/broadcast/top -- non utilise ici --
/// couvre aussi des tournois vieux de plusieurs mois) -- pagine (24
/// resultats/page cote Lichess), on s'arrete des qu'il n'y a plus de page
/// suivante ou a `max_pages` (garde-fou : une recherche trop generique ne
/// doit pas remonter des centaines de pages).
pub fn search(query: &str, max_pages: u32) -> Result<Vec<BroadcastSearchResult>, String> {
    let mut results = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("https://lichess.org/api/broadcast/search?q={}&page={page}", urlencoding(query));
        let resp: BroadcastSearchPage = get_json(&url)?;
        let has_next = resp.next_page.is_some();
        results.extend(resp.current_page_results);
        if !has_next || page >= max_pages {
            break;
        }
        page += 1;
    }
    Ok(results)
}

/// Export PGN complet d'un tournoi -- TOUTES ses rondes en une seule requete
/// (verifie manuellement : contrairement a ce que suggere le reste de cette
/// API, il n'y a pas besoin de lister les rondes puis d'exporter chacune
/// separement, voir l'ancienne tournament_rounds()/round_pgn() par ronde,
/// retiree -- plusieurs rondes par requete au lieu d'une seule, important
/// pour un rattrapage de plusieurs semaines ou chaque TWIC distinct peut
/// recouper des dizaines de tournois). Parties terminees ou non -- le tri
/// "non terminees" se fait cote appelant sur le texte PGN obtenu, comme pour
/// TWIC, voir external_updates.rs.
pub fn tournament_pgn(tournament_id: &str) -> Result<String, String> {
    let url = format!("https://lichess.org/api/broadcast/{tournament_id}.pgn");
    ureq::get(&url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
}

/// Vrai si deux plages [debut, fin] (meme unite, ici des millisecondes
/// epoque) se recoupent -- utilise pour ne retenir, parmi les resultats
/// d'une recherche Lichess par nom, que ceux dont la periode correspond
/// reellement a celle du tournoi TWIC compare (le nom seul est un signal
/// trop faible, voir external_updates.rs).
pub fn date_range_overlaps(a: [i64; 2], b: [i64; 2]) -> bool {
    a[0] <= b[1] && b[0] <= a[1]
}

/// `encodeURIComponent`-like minimal (le crate `urlencoding` n'est pas une
/// dependance existante, un nom de tournoi reste du texte simple -- espaces
/// et quelques signes a percent-encoder suffisent ici, pas besoin d'une
/// implementation RFC 3986 complete pour un parametre de recherche texte).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_range_overlaps_detects_overlap_and_disjoint_ranges() {
        assert!(date_range_overlaps([100, 200], [150, 250]));
        assert!(date_range_overlaps([100, 200], [50, 120]));
        assert!(date_range_overlaps([100, 200], [100, 200]));
        assert!(!date_range_overlaps([100, 200], [201, 300]));
        assert!(!date_range_overlaps([201, 300], [100, 200]));
    }

    #[test]
    fn urlencoding_escapes_spaces_and_keeps_safe_characters() {
        assert_eq!(urlencoding("fide candidates 2026"), "fide%20candidates%202026");
        assert_eq!(urlencoding("abc-123_XYZ.~"), "abc-123_XYZ.~");
    }
}
