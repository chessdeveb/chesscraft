use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

// Referentiel public lichess/chess-openings (CC0), fusionne en un seul
// fichier -- voir resources/eco.README.md pour la provenance et le format.
// Inclus dans le binaire (3810 entrees, quelques centaines de Ko) : jamais
// mis a jour a l'usage (contrairement a la liste FIDE), pas besoin d'un
// fichier externe/telechargeable pour une donnee qui ne bouge quasiment pas.
const ECO_TSV: &str = include_str!("../resources/eco.tsv");

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EcoInfo {
    pub eco: String,
    pub name: String,
}

/// Retire les numeros de coup ("1.", "2.", jamais "1..." dans ce referentiel
/// -- ligne principale seulement, pas de variantes) et le suffixe d'echec/mat
/// ("+"/"#") d'une ligne au format PGN classique ("1. e4 e6 2. d4 d5"), pour
/// obtenir la meme forme que mainline_sans/le SAN interne de l'appli (voir
/// pgn::san, game_tree::legal_moves) : sans numeros, sans suffixe.
fn normalize_pgn_moves(pgn: &str) -> Vec<String> {
    pgn.split_whitespace()
        .filter(|tok| !tok.starts_with(|c: char| c.is_ascii_digit()))
        .map(|tok| tok.trim_end_matches(['+', '#']).to_string())
        .collect()
}

/// Table (sequence de coups -> code+nom ECO), construite une seule fois a
/// partir du referentiel embarque. Cle = les coups joints par des espaces,
/// meme convention que `moves.join(" ")` utilise par explorer::explore.
fn eco_table() -> &'static HashMap<String, EcoInfo> {
    static TABLE: OnceLock<HashMap<String, EcoInfo>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for line in ECO_TSV.lines().skip(1) {
            let mut parts = line.splitn(3, '\t');
            let (Some(eco), Some(name), Some(pgn)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let moves = normalize_pgn_moves(pgn);
            if moves.is_empty() {
                continue;
            }
            map.insert(moves.join(" "), EcoInfo { eco: eco.to_string(), name: name.to_string() });
        }
        map
    })
}

/// Classifie la position atteinte par `moves` (SAN sans suffixe, meme
/// convention que mainline_sans) selon le referentiel ECO : cherche la
/// correspondance exacte la plus longue en reculant coup par coup, comme
/// suggere par la source elle-meme (chaque ouverture nommee y a une ligne la
/// plus courte unique -- voir resources/eco.README.md). None si aucun
/// prefixe de `moves` ne correspond a une ouverture nommee (position de
/// depart, ou ligne trop rare/exotique pour figurer dans le referentiel).
pub fn classify(moves: &[String]) -> Option<EcoInfo> {
    let table = eco_table();
    for len in (1..=moves.len()).rev() {
        if let Some(info) = table.get(&moves[..len].join(" ")) {
            return Some(info.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sans(moves: &[&str]) -> Vec<String> {
        moves.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classifies_the_first_move() {
        let info = classify(&sans(&["e4"])).unwrap();
        assert_eq!(info.eco, "B00");
        assert_eq!(info.name, "King's Pawn Game");
    }

    #[test]
    fn classifies_a_deeper_position_distinctly_from_a_prefix() {
        // "1. e4" (B00) et "1. e4 e5" (C20) sont deux entrees distinctes du
        // referentiel : la classification doit prendre la plus longue qui
        // correspond exactement, pas s'arreter au premier coup.
        let info = classify(&sans(&["e4", "e5"])).unwrap();
        assert_eq!(info.eco, "C20");
    }

    #[test]
    fn falls_back_to_the_longest_matching_prefix() {
        // Aucune entree exacte pour "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 Zz9" (coup
        // invente) : doit retomber sur le plus long prefixe reel qui, lui,
        // correspond a une ouverture nommee, pas renvoyer None.
        let moves = sans(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6", "Zz9"]);
        let info = classify(&moves).unwrap();
        assert!(!info.eco.is_empty());
    }

    #[test]
    fn returns_none_for_the_starting_position() {
        assert_eq!(classify(&[]), None);
    }

    #[test]
    fn normalize_strips_move_numbers_and_check_suffixes() {
        assert_eq!(normalize_pgn_moves("1. e4 e6 2. d4 d5"), vec!["e4", "e6", "d4", "d5"]);
        assert_eq!(normalize_pgn_moves("1. e4 e5 2. Qh5 Nc6 3. Qxf7#"), vec!["e4", "e5", "Qh5", "Nc6", "Qxf7"]);
    }
}
