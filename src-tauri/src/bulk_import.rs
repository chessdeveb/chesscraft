use crate::db;
use pgn_reader::{Nag, Outcome, RawComment, RawTag, Reader, SanPlus, Visitor};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::ControlFlow;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportSummary {
    pub total: u32,
    pub imported: u32,
    pub skipped_duplicates: u32,
    pub failed: u32,
}

/// Visiteur "leger" : ne valide pas les coups (pas de simulation d'echiquier),
/// se contente de recopier le texte SAN tel que fourni par le parseur. Les
/// variantes imbriquees sont ignorees (comportement par defaut de pgn-reader)
/// pour rester rapide sur de tres gros fichiers ; la ligne principale, les
/// commentaires et les NAG sont conserves.
struct BulkGameVisitor {
    tags: BTreeMap<String, String>,
    movetext: String,
    san_tokens: Vec<String>,
    ply: u32,
}

impl BulkGameVisitor {
    fn new() -> Self {
        BulkGameVisitor {
            tags: BTreeMap::new(),
            movetext: String::new(),
            san_tokens: Vec::new(),
            ply: 0,
        }
    }
}

impl Visitor for BulkGameVisitor {
    type Tags = ();
    type Movetext = ();
    type Output = ();

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(())
    }

    fn tag(&mut self, _tags: &mut Self::Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<Self::Output> {
        if let Ok(name) = std::str::from_utf8(name) {
            self.tags.insert(name.to_string(), value.decode_utf8_lossy().into_owned());
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        let text = san_plus.to_string();
        if self.ply % 2 == 0 {
            self.movetext.push_str(&format!("{}. ", self.ply / 2 + 1));
        }
        self.movetext.push_str(&text);
        self.movetext.push(' ');
        self.san_tokens.push(text);
        self.ply += 1;
        ControlFlow::Continue(())
    }

    fn nag(&mut self, _movetext: &mut Self::Movetext, nag: Nag) -> ControlFlow<Self::Output> {
        self.movetext.push_str(&format!("${} ", nag.0));
        ControlFlow::Continue(())
    }

    fn comment(&mut self, _movetext: &mut Self::Movetext, comment: RawComment<'_>) -> ControlFlow<Self::Output> {
        // un commentaire source contenant litteralement { ou } casserait le
        // texte PGN reconstruit (fermeture prematuree, coups restants
        // interpretes comme du movetext) : on neutralise ces caracteres.
        let text = String::from_utf8_lossy(comment.as_bytes()).replace(['{', '}'], "");
        self.movetext.push_str("{ ");
        self.movetext.push_str(&text);
        self.movetext.push_str(" } ");
        ControlFlow::Continue(())
    }

    fn outcome(&mut self, _movetext: &mut Self::Movetext, outcome: Outcome) -> ControlFlow<Self::Output> {
        self.movetext.push_str(&outcome.to_string());
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {}
}

fn assemble_pgn(tags: &BTreeMap<String, String>, movetext: &str) -> String {
    let mut out = String::new();
    for (key, value) in tags {
        out.push_str(&format!(
            "[{key} \"{}\"]\n",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    out.push('\n');
    out.push_str(movetext.trim());
    out.push('\n');
    out
}

/// Traite une partie (detection de doublon + sauvegarde). Isole dans sa
/// propre fonction pour pouvoir l'entourer d'un catch_unwind : une partie
/// malformee qui ferait paniquer le parseur de doublon ne doit pas perdre
/// tout le lot en cours.
fn process_one_game(tx: &rusqlite::Transaction, visitor: &BulkGameVisitor) -> Result<bool, String> {
    match db::find_duplicate(tx, &visitor.tags, &visitor.san_tokens)? {
        Some(_) => Ok(false),
        None => {
            let pgn = assemble_pgn(&visitor.tags, &visitor.movetext);
            let moves_str = visitor.san_tokens.join(" ");
            db::save_game(tx, &pgn, &visitor.tags, &moves_str)?;
            Ok(true)
        }
    }
}

/// Nombre de parties traitees entre deux validations (COMMIT) de la
/// transaction en cours. Limite les degats en cas d'arret brutal (plantage,
/// panique) : seul le lot en cours est perdu, pas tout l'import.
const BATCH_SIZE: u32 = 2000;

/// Importe toutes les parties d'un texte PGN dans la base, par lots valides
/// regulierement. `on_progress` est appele regulierement (pas a chaque
/// partie) avec les compteurs courants.
pub fn bulk_import(
    conn: &mut Connection,
    pgn_text: &str,
    mut on_progress: impl FnMut(&BulkImportSummary),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<BulkImportSummary, String> {
    let mut summary = BulkImportSummary::default();
    let mut tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut since_commit = 0u32;

    let mut reader = Reader::new(pgn_text.as_bytes());
    loop {
        if summary.total % 200 == 0 && should_cancel() {
            break;
        }

        let mut visitor = BulkGameVisitor::new();
        let read_result = reader.read_game(&mut visitor);
        match read_result {
            Ok(Some(())) => {}
            Ok(None) => break,
            // une erreur de lecture peut laisser le lecteur bloque a la meme
            // position : on arrete plutot que de boucler indefiniment.
            Err(_) => {
                summary.failed += 1;
                break;
            }
        }
        summary.total += 1;

        if visitor.tags.is_empty() && visitor.san_tokens.is_empty() {
            summary.failed += 1;
        } else {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process_one_game(&tx, &visitor)
            }));
            match outcome {
                Ok(Ok(true)) => summary.imported += 1,
                Ok(Ok(false)) => summary.skipped_duplicates += 1,
                Ok(Err(_)) | Err(_) => summary.failed += 1,
            }
        }

        since_commit += 1;
        if since_commit >= BATCH_SIZE {
            tx.commit().map_err(|e| e.to_string())?;
            tx = conn.transaction().map_err(|e| e.to_string())?;
            since_commit = 0;
        }

        if summary.total % 500 == 0 {
            on_progress(&summary);
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    on_progress(&summary);
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_two_games() {
        let pgn = r#"[Event "Test Event"]
[Site "Test Site"]
[Date "2024.01.01"]
[Round "1"]
[White "Alice"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 1-0

[Event "Test Event 2"]
[Site "Test Site"]
[Date "2024.01.02"]
[Round "2"]
[White "Carol"]
[Black "Dave"]
[Result "0-1"]

1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 {comment here} 5. e3 O-O 0-1
"#;
        let mut conn = Connection::open_in_memory().unwrap();
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

        let summary = bulk_import(&mut conn, pgn, |_| {}, || false).unwrap();
        println!(
            "total={} imported={} dup={} failed={}",
            summary.total, summary.imported, summary.skipped_duplicates, summary.failed
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.imported, 2);
        assert_eq!(summary.failed, 0);
    }

    fn fresh_conn() -> Connection {
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
        conn
    }

    #[test]
    fn survives_multiple_commit_batches() {
        // plus de parties que BATCH_SIZE, pour verifier que les commits
        // intermediaires ne perdent ni ne dupliquent rien.
        let count = BATCH_SIZE + 500;
        let mut pgn = String::new();
        for i in 0..count {
            pgn.push_str(&format!(
                "[Event \"E\"]\n[Site \"S\"]\n[Date \"2024.01.01\"]\n[Round \"{i}\"]\n[White \"W{i}\"]\n[Black \"B{i}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n"
            ));
        }

        let mut conn = fresh_conn();
        let summary = bulk_import(&mut conn, &pgn, |_| {}, || false).unwrap();
        assert_eq!(summary.total, count);
        assert_eq!(summary.imported, count);
        assert_eq!(summary.failed, 0);

        let actual: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        assert_eq!(actual, count as i64);
    }

    #[test]
    fn cancellation_keeps_already_committed_batches() {
        let count = BATCH_SIZE * 2;
        let mut pgn = String::new();
        for i in 0..count {
            pgn.push_str(&format!(
                "[Event \"E\"]\n[Site \"S\"]\n[Date \"2024.01.01\"]\n[Round \"{i}\"]\n[White \"W{i}\"]\n[Black \"B{i}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n"
            ));
        }

        let mut conn = fresh_conn();
        // laisse le temps d'entamer le deuxieme lot avant d'annuler
        let mut checks = 0u32;
        let summary = bulk_import(
            &mut conn,
            &pgn,
            |_| {},
            move || {
                checks += 1;
                checks > 15
            },
        )
        .unwrap();
        assert!(summary.total > BATCH_SIZE, "devrait avoir depasse le premier lot");
        assert!(summary.total < count, "devrait s'etre arrete avant la fin");

        let actual: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        assert_eq!(actual, summary.imported as i64);
    }
}
