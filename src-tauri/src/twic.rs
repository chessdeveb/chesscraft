use std::io::Read;

/// Edition de reference (verifiee manuellement sur theweekinchess.com) pour
/// convertir une date en numero d'edition approximatif (voir issue_for_date)
/// -- TWIC parait chaque semaine, le numero croit donc d'une unite par
/// semaine ecoulee depuis cette ancre. Pas documente officiellement comme un
/// schema stable (une edition peut sauter/etre retardee), d'ou l'arrondi
/// deliberement conservateur de issue_for_date (voir son commentaire) :
/// l'objectif est juste un POINT DE DEPART raisonnable pour le sondage
/// (probe_latest_issue), jamais une verite exacte.
const ANCHOR_ISSUE: u32 = 1666;
/// Jours ecoules depuis l'epoque Unix jusqu'au 2026-10-12 (date de
/// ANCHOR_ISSUE), calcule une fois ici pour eviter de reimplementer un calcul
/// de calendrier complet juste pour cette ancre.
const ANCHOR_DAYS_SINCE_EPOCH: i64 = 20738;

fn issue_url(issue: u32) -> String {
    format!("https://theweekinchess.com/zips/twic{issue}g.zip")
}

/// Jours ecoules depuis l'epoque Unix pour une date "AAAA.MM.JJ" (tag PGN
/// standard) -- None si la date est absente/partielle ("????.??.??" ou un
/// seul champ inconnu), auquel cas elle ne peut pas servir a estimer une
/// edition. Calcul de calendrier minimal (pas de dependance externe) : exact
/// pour toute date gregorienne valide, ce qui couvre tous les cas reels d'un
/// tag PGN.
pub fn days_since_epoch(date: &str) -> Option<i64> {
    let parts: Vec<&str> = date.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Algorithme days_from_civil (Howard Hinnant) : https://howardhinnant.github.io/date_algorithms.html
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Estime le numero d'edition TWIC en circulation a une date donnee, a
/// partir de l'ancre ANCHOR_ISSUE/ANCHOR_DAYS_SINCE_EPOCH -- arrondi
/// DELIBEREMENT a l'edition d'UNE SEMAINE PLUS TOT que l'estimation brute
/// (saturee a 1) : repartir un cran trop tot ne fait que recouvrir une
/// semaine deja presente (le dedoublonnage existant l'absorbe sans rien
/// dupliquer, voir db::find_duplicate), alors que repartir trop tard
/// laisserait silencieusement un vrai trou. Utilisee pour calculer le point
/// de depart du rattrapage a partir de la date la plus recente deja connue
/// (voir external_updates::infer_start_issue), jamais pour autre chose.
pub fn issue_for_date(date: &str) -> Option<u32> {
    let days = days_since_epoch(date)?;
    let weeks_from_anchor = (days - ANCHOR_DAYS_SINCE_EPOCH).div_euclid(7);
    let estimated = ANCHOR_ISSUE as i64 + weeks_from_anchor - 1;
    Some(estimated.max(1) as u32)
}

/// Verite un numero d'edition TWIC (requete HEAD, pas de telechargement) :
/// Some(taille en octets) si l'edition existe, None sinon (404).
fn issue_exists(issue: u32) -> Result<bool, String> {
    match ureq::head(&issue_url(issue)).call() {
        Ok(_) => Ok(true),
        Err(ureq::Error::Status(404, _)) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// Sonde en avant depuis `start` (inclus) jusqu'a la premiere edition
/// absente (404), retourne la derniere edition confirmee presente -- aucun
/// scraping HTML, repose uniquement sur le format d'URL deja verifie
/// (theweekinchess.com/zips/twicNg.zip) et la semantique HTTP standard
/// 200/404. Pas de borne haute arbitraire : meme apres un an d'absence
/// (~52 sondages), reste rapide (requetes HEAD, pas de telechargement).
pub fn probe_latest_issue(start: u32) -> Result<Option<u32>, String> {
    if !issue_exists(start)? {
        return Ok(None);
    }
    let mut latest = start;
    loop {
        let next = latest + 1;
        if !issue_exists(next)? {
            return Ok(Some(latest));
        }
        latest = next;
    }
}

/// Estimation (non verifiee) du numero d'edition probablement en
/// circulation AUJOURD'HUI -- contrairement a issue_for_date, pas d'arrondi
/// conservateur d'une semaine : sert uniquement a amorcer find_latest_issue,
/// qui verifie reellement par sondage HTTP et corrige si besoin.
fn estimated_current_issue() -> u32 {
    let now_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86_400)
        .unwrap_or(ANCHOR_DAYS_SINCE_EPOCH);
    let weeks_from_anchor = (now_days - ANCHOR_DAYS_SINCE_EPOCH).div_euclid(7);
    (ANCHOR_ISSUE as i64 + weeks_from_anchor).max(1) as u32
}

/// Derniere edition TWIC reellement disponible : amorce une estimation a
/// partir d'aujourd'hui (estimated_current_issue), verifie par sondage HTTP
/// (probe_latest_issue), et recule par petits pas si l'estimation elle-meme
/// n'existe pas encore/plus (edition en retard ce jour-la, ou legere derive
/// du calendrier reel) -- jusqu'a 8 semaines en arriere avant d'abandonner.
pub fn find_latest_issue() -> Result<u32, String> {
    let mut seed = estimated_current_issue();
    for _ in 0..8 {
        if let Some(latest) = probe_latest_issue(seed)? {
            return Ok(latest);
        }
        if seed <= 1 {
            break;
        }
        seed -= 1;
    }
    Err("impossible de déterminer la dernière édition TWIC disponible".to_string())
}

/// Telecharge le zip "games" (PGN, pas la variante ChessBase) d'une edition.
pub fn download_issue(issue: u32) -> Result<Vec<u8>, String> {
    let resp = ureq::get(&issue_url(issue)).call().map_err(|e| format!("téléchargement TWIC {issue} échoué : {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Extrait le contenu du premier fichier ".pgn" trouve dans le zip -- par
/// nom plutot qu'un nom fixe (ex. "twic1666.pgn") : reste correct si TWIC
/// changeait un jour sa convention de nommage interne au zip, seule
/// l'extension compte.
pub fn extract_pgn(zip_bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;
    let name = (0..archive.len())
        .map(|i| archive.by_index(i).map(|f| f.name().to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|n| n.to_ascii_lowercase().ends_with(".pgn"))
        .ok_or_else(|| "archive TWIC inattendue (aucun fichier .pgn)".to_string())?;
    let mut entry = archive.by_name(&name).map_err(|e| e.to_string())?;
    let mut text = String::new();
    entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_for_date_matches_the_verified_anchor() {
        // l'ancre elle-meme : une semaine avant l'ancre par construction
        // (voir le commentaire de issue_for_date sur l'arrondi conservateur).
        assert_eq!(issue_for_date("2026.10.12"), Some(ANCHOR_ISSUE - 1));
    }

    #[test]
    fn issue_for_date_goes_back_one_issue_per_week() {
        assert_eq!(issue_for_date("2026.10.05"), Some(ANCHOR_ISSUE - 2));
        assert_eq!(issue_for_date("2026.09.28"), Some(ANCHOR_ISSUE - 3));
        assert_eq!(issue_for_date("2026.10.19"), Some(ANCHOR_ISSUE));
    }

    #[test]
    fn issue_for_date_never_returns_zero_or_negative() {
        assert_eq!(issue_for_date("1952.01.01"), Some(1));
    }

    #[test]
    fn issue_for_date_rejects_partial_or_unknown_dates() {
        assert_eq!(issue_for_date("????.??.??"), None);
        assert_eq!(issue_for_date("2026.??.??"), None);
        assert_eq!(issue_for_date("not a date"), None);
    }

    #[test]
    fn days_since_epoch_matches_known_reference_points() {
        // 1970-01-01 est le jour 0 par definition de l'epoque Unix.
        assert_eq!(days_since_epoch("1970.01.01"), Some(0));
        // 2000-03-01, point de controle usuel pour verifier civil_from_days
        // (bissextile correctement geree juste avant).
        assert_eq!(days_since_epoch("2000.03.01"), Some(11017));
    }
}
