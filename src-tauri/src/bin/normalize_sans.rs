// Outil ponctuel, hors de l'appli Tauri elle-meme : re-derive un SAN
// pleinement canonique (desambiguisation minimale, exactement celle que
// produirait le moteur de l'appli, voir game_tree::legal_moves) pour
// mainline_sans sur toutes les parties d'une base, en rejouant chaque
// partie depuis le debut via shakmaty.
//
// Corrige les parties dont la source PGN d'origine portait une
// desambiguisation superflue (ex. "Ngf6" alors qu'un seul cavalier peut
// legalement atteindre f6, l'autre etant cloue au roi -- cas reel rapporte
// par l'utilisateur) : mainline_sans recopie a l'import le texte SAN du
// fichier source tel quel (voir bulk_import::BulkGameVisitor::san), sans
// jamais le regenerer via le moteur. Le reste de l'appli (explorateur,
// clic sur un coup suggere, recherche "parties dans cette position")
// compare pourtant ce texte litteralement : le moindre ecart avec le SAN
// que shakmaty produirait lui-meme casse la correspondance -- le coup
// suggere devient alors injouable au clic, et le rejouer manuellement (sous
// sa forme correcte, plus courte) ne retrouve plus aucune partie.
//
// Volontairement SEPARE de l'appli et de ses migrations automatiques (voir
// db::strip_check_suffixes, qui ne fait que retirer "+"/"#", executee en
// tache de fond au premier chargement d'une base) : ce traitement est plus
// lourd (rejoue integralement chaque partie, pas un simple retrait de
// caracteres) et n'a vocation a tourner qu'une fois sur une base existante
// deja affectee -- pas a etre integre a un cycle de migration permanent.
//
// Usage :
//   cargo run --release --bin normalize_sans -- <chemin-vers-base.sqlite> [--dry-run]
//
// --dry-run : affiche ce qui serait modifie sans rien ecrire.

use chesscraft_lib::db;
use chesscraft_lib::explorer;
use rusqlite::params;
use shakmaty::san::San;
use shakmaty::{Chess, Position};
use std::path::Path;

const PAGE_SIZE: i64 = 2000;

/// Rejoue une ligne de coups SAN (sans suffixe "+"/"#", voir la convention
/// du reste de l'appli -- game_tree/legal_moves/mainline_sans une fois
/// nettoye par strip_check_suffixes n'en portent jamais) depuis la position
/// de depart, et renvoie la meme ligne reecrite avec le SAN minimal que
/// shakmaty produirait pour chaque coup. None des qu'un coup ne se rejoue
/// pas (donnee corrompue, notation non standard...) : mieux vaut laisser
/// la partie intacte que d'ecrire une ligne partiellement reecrite.
fn canonicalize_mainline(mainline: &str) -> Option<Vec<String>> {
    let mut position = Chess::default();
    let mut out = Vec::new();
    for token in mainline.split(' ').filter(|s| !s.is_empty()) {
        let clean = token.trim_end_matches(['+', '#']);
        let san: San = clean.parse().ok()?;
        let mv = san.to_move(&position).ok()?;
        out.push(San::from_move(&position, mv.clone()).to_string());
        position = position.play(mv).ok()?;
    }
    Some(out)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1).cloned() else {
        eprintln!("usage: normalize_sans <chemin-vers-base.sqlite> [--dry-run]");
        std::process::exit(1);
    };
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let mut conn = db::open(Path::new(&path)).expect("ouverture de la base");

    let mut examined = 0u64;
    let mut changed = 0u64;
    let mut failed = 0u64;
    let mut last_id = 0i64;

    loop {
        let page: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, mainline_sans FROM games WHERE id > ?1 ORDER BY id LIMIT ?2")
                .expect("prepare select");
            let rows = stmt
                .query_map(params![last_id, PAGE_SIZE], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default()))
                })
                .expect("query select");
            rows.collect::<Result<Vec<_>, _>>().expect("collect rows")
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let mut updates: Vec<(i64, String)> = Vec::new();
        for (id, mainline) in &page {
            examined += 1;
            if mainline.is_empty() {
                continue;
            }
            match canonicalize_mainline(mainline) {
                Some(tokens) => {
                    let rewritten = tokens.join(" ");
                    if &rewritten != mainline {
                        updates.push((*id, rewritten));
                    }
                }
                None => failed += 1,
            }
        }

        if !dry_run && !updates.is_empty() {
            let tx = conn.transaction().expect("begin transaction");
            {
                let mut update = tx.prepare("UPDATE games SET mainline_sans = ?1 WHERE id = ?2").expect("prepare update");
                for (id, rewritten) in &updates {
                    update.execute(params![rewritten, id]).expect("update row");
                }
            }
            tx.commit().expect("commit transaction");
        }
        changed += updates.len() as u64;

        println!("examinees: {examined}, reecrites: {changed}, non rejouables: {failed}");
    }

    if !dry_run && changed > 0 {
        println!("recalcul des stats de l'explorateur...");
        explorer::rebuild_position_index(&mut conn, |progress| match progress {
            explorer::RebuildProgress::Reading { examined } => println!("  position_index: lecture, {examined} parties examinees"),
            explorer::RebuildProgress::Writing { written, total } => println!("  position_index: ecriture, {written} / {total} lignes"),
        }, || false)
        .expect("rebuild_position_index");
        db::rebuild_player_stats(&mut conn, |progress| match progress {
            explorer::RebuildProgress::Reading { examined } => println!("  player_stats: lecture, {examined} parties examinees"),
            explorer::RebuildProgress::Writing { written, total } => println!("  player_stats: ecriture, {written} / {total} lignes"),
        })
        .expect("rebuild_player_stats");
    }

    println!(
        "termine. {examined} parties examinees, {changed} reecrites, {failed} non rejouables (laissees telles quelles){}.",
        if dry_run { " [dry-run, rien n'a ete ecrit]" } else { "" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cas reel rapporte par l'utilisateur : Bb5+ puis Nd7 cloue le cavalier
    // en d7 au roi noir (diagonale b5-c6-d7-e8, c6 vide) -- seul le cavalier
    // de g8 peut alors atteindre f6, donc "Nf6" est la seule notation SAN
    // correcte (minimale), sans "g". Une source PGN ayant tout de meme
    // enregistre "Ngf6" (desambiguisation superflue) doit etre corrigee.
    #[test]
    fn corrects_unnecessary_disambiguation_caused_by_a_pinned_piece() {
        let original = "e4 c5 Nf3 d6 Bb5+ Nd7 O-O Ngf6";
        let expected = vec!["e4", "c5", "Nf3", "d6", "Bb5", "Nd7", "O-O", "Nf6"];
        assert_eq!(canonicalize_mainline(original), Some(expected.into_iter().map(String::from).collect()));
    }

    #[test]
    fn leaves_an_already_canonical_line_unchanged() {
        let original = "e4 e5 Nf3 Nc6 Bb5";
        let expected: Vec<String> = original.split(' ').map(String::from).collect();
        assert_eq!(canonicalize_mainline(original), Some(expected));
    }

    #[test]
    fn gives_up_on_a_line_that_does_not_replay() {
        // "e4" comme second demi-coup (Noirs) ne correspond a aucun coup
        // legal : le pion e7 ne peut pas atteindre e4 en un seul coup.
        assert_eq!(canonicalize_mainline("e4 e4"), None);
        // jeton illisible (ni SAN ni un role d'echecs connu)
        assert_eq!(canonicalize_mainline("e4 e5 Zz9"), None);
    }
}
