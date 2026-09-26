use crate::db;
use pgn_reader::{Nag, Outcome, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
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
    pub skipped_invalid: u32,
    pub failed: u32,
}

enum GameOutcome {
    Imported,
    Duplicate,
    /// Blancs ou Noirs manquant/vide/"?" (convention PGN pour "inconnu") :
    /// partie inexploitable, on ne la garde pas.
    InvalidNames,
}

fn has_name(tags: &BTreeMap<String, String>, tag: &str) -> bool {
    tags.get(tag).is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && v != "?"
    })
}

/// Visiteur "leger" : ne valide pas les coups (pas de simulation d'echiquier),
/// se contente de recopier le texte SAN tel que fourni par le parseur.
/// `san_tokens` (utilise pour mainline_sans, l'index de position et la
/// detection de doublon) ne doit contenir QUE la ligne principale -- `depth`
/// suit la profondeur d'imbrication des variantes ( ... ) pour ne pousser
/// dans san_tokens/numeroter les coups qu'a profondeur 0, tout en recopiant
/// quand meme le texte SAN des variantes dans `movetext` (profondeur > 0)
/// pour qu'elles survivent dans le PGN stocke. Les numeros de coup ne sont
/// pas ecrits a l'interieur des variantes : le parseur (ici comme a la
/// reouverture, voir pgn.rs) ne s'appuie que sur la sequence SAN, jamais sur
/// les numeros, qui ne sont que decoratifs.
struct BulkGameVisitor {
    tags: BTreeMap<String, String>,
    movetext: String,
    san_tokens: Vec<String>,
    ply: u32,
    depth: u32,
}

impl BulkGameVisitor {
    fn new() -> Self {
        BulkGameVisitor {
            tags: BTreeMap::new(),
            movetext: String::new(),
            san_tokens: Vec::new(),
            ply: 0,
            depth: 0,
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
        if self.depth == 0 {
            if self.ply % 2 == 0 {
                self.movetext.push_str(&format!("{}. ", self.ply / 2 + 1));
            }
            // san_plus.to_string() garde le "+"/"#" de fin de coup (echec/mat)
            // du PGN source -- san_tokens (mainline_sans) doit rester sans ce
            // suffixe, seule forme utilisee partout ailleurs dans l'appli
            // (arbre de partie, legal_moves, moteur, voir pgn::san), sinon
            // l'explorateur d'ouvertures ne peut plus faire correspondre un
            // coup qui donne echec (voir migrate_strip_check_suffixes, db.rs).
            self.san_tokens.push(san_plus.san.to_string());
            self.ply += 1;
        }
        self.movetext.push_str(&text);
        self.movetext.push(' ');
        ControlFlow::Continue(())
    }

    fn begin_variation(&mut self, _movetext: &mut Self::Movetext) -> ControlFlow<Self::Output, Skip> {
        self.movetext.push_str("( ");
        self.depth += 1;
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, _movetext: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        self.depth = self.depth.saturating_sub(1);
        self.movetext.push_str(") ");
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
fn process_one_game(
    tx: &rusqlite::Transaction,
    visitor: &BulkGameVisitor,
    white_fide_id: Option<&str>,
    black_fide_id: Option<&str>,
) -> Result<GameOutcome, String> {
    if !has_name(&visitor.tags, "White") || !has_name(&visitor.tags, "Black") {
        return Ok(GameOutcome::InvalidNames);
    }
    match db::find_duplicate(tx, &visitor.tags, &visitor.san_tokens)? {
        Some(_) => Ok(GameOutcome::Duplicate),
        None => {
            let pgn = assemble_pgn(&visitor.tags, &visitor.movetext);
            let moves_str = visitor.san_tokens.join(" ");
            let id = db::save_game(tx, &pgn, &visitor.tags, &moves_str)?;
            if white_fide_id.is_some() || black_fide_id.is_some() {
                db::set_game_fide_ids(tx, id, white_fide_id, black_fide_id)?;
            }
            Ok(GameOutcome::Imported)
        }
    }
}

/// Nombre de parties traitees entre deux validations (COMMIT) de la
/// transaction en cours. Limite les degats en cas d'arret brutal (plantage,
/// panique) : seul le lot en cours est perdu, pas tout l'import.
const BATCH_SIZE: u32 = 2000;

/// Importe toutes les parties d'une source PGN dans la base, par lots
/// valides regulierement. `on_progress` est appele regulierement (pas a
/// chaque partie) avec les compteurs courants.
///
/// `pgn_source` est generique sur `Read` (plutot qu'un `&str` deja charge en
/// memoire) expres : certaines bases source (LumbrasGigaBase...) depassent
/// les 8 Go, et `std::fs::read_to_string` les chargeait entierement avant
/// meme de commencer -- gros pic memoire, et un fichier de cette taille
/// pouvait epuiser la memoire disponible en cours de route, interrompant
/// l'import a mi-chemin (voir l'appelant, qui passe maintenant un
/// `BufReader<File>` : lecture en flux, memoire proportionnelle a une seule
/// partie a la fois, pas au fichier entier).
pub fn bulk_import(
    conn: &mut Connection,
    pgn_source: impl std::io::Read,
    mut on_progress: impl FnMut(&BulkImportSummary),
    mut should_cancel: impl FnMut() -> bool,
    mut canonicalize_name: impl FnMut(&str, Option<i32>) -> Option<(String, String)>,
) -> Result<BulkImportSummary, String> {
    let mut summary = BulkImportSummary::default();
    let mut tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut since_commit = 0u32;

    let mut reader = Reader::new(pgn_source);
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

        // Reecrit Blancs/Noirs vers leur forme canonique FIDE quand elle est
        // resolue sans ambiguite (voir fide::canonicalize) -- avant la
        // detection de doublon ET la sauvegarde, pour que "Giri,A" et
        // "Giri, Anish" importes dans le meme lot finissent avec la meme
        // chaine (dedoublonnage) au lieu de fragmenter le joueur en deux
        // entrees. WhiteElo/BlackElo (s'ils sont presents) servent d'indice
        // pour departager un nom de famille + initiale encore ambigus (ex.
        // "Giri, Anish" vs "Giri, Andrea"). L'identifiant FIDE resolu en meme
        // temps que le nom est conserve a part (pas dans visitor.tags, qui
        // finit tel quel dans le texte PGN assemble par assemble_pgn) pour
        // etre pose sur games.white_fide_id/black_fide_id apres coup (voir
        // process_one_game) -- absent (None) des qu'aucun candidat FIDE
        // unique et non ambigu n'a ete trouve, meme apres depart par Elo.
        let mut white_fide_id: Option<String> = None;
        let mut black_fide_id: Option<String> = None;
        for (tag, elo_tag) in [("White", "WhiteElo"), ("Black", "BlackElo")] {
            if let Some(raw) = visitor.tags.get(tag) {
                let elo_hint = visitor.tags.get(elo_tag).and_then(|s| s.trim().parse::<i32>().ok());
                if let Some((canonical, fide_id)) = canonicalize_name(raw, elo_hint) {
                    visitor.tags.insert(tag.to_string(), canonical);
                    if tag == "White" {
                        white_fide_id = Some(fide_id);
                    } else {
                        black_fide_id = Some(fide_id);
                    }
                }
            }
        }

        if visitor.tags.is_empty() && visitor.san_tokens.is_empty() {
            summary.failed += 1;
        } else {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process_one_game(&tx, &visitor, white_fide_id.as_deref(), black_fide_id.as_deref())
            }));
            match outcome {
                Ok(Ok(GameOutcome::Imported)) => summary.imported += 1,
                Ok(Ok(GameOutcome::Duplicate)) => summary.skipped_duplicates += 1,
                Ok(Ok(GameOutcome::InvalidNames)) => summary.skipped_invalid += 1,
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
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT, start_fen TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();

        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |_, _| None).unwrap();
        println!(
            "total={} imported={} dup={} failed={}",
            summary.total, summary.imported, summary.skipped_duplicates, summary.failed
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.imported, 2);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn preserves_nested_variations_in_stored_pgn_without_polluting_mainline() {
        let pgn = r#"[Event "Test Event"]
[Site "Test Site"]
[Date "2024.01.01"]
[Round "1"]
[White "Alice"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Nf3 (2. Bc4 Bc5 (2... Nf6 3. d3) 3. Qh5) 2... Nc6 3. Bb5 1-0
"#;
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT, start_fen TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();

        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |_, _| None).unwrap();
        assert_eq!(summary.imported, 1);

        let (pgn_moves, mainline_sans): (String, String) = conn
            .query_row("SELECT pgn_moves, mainline_sans FROM games LIMIT 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();

        // la ligne principale ne doit contenir que les coups joues, pas ceux
        // des variantes (Bc4, Qh5, d3 ne doivent jamais y figurer)
        assert_eq!(mainline_sans, "e4 e5 Nf3 Nc6 Bb5");

        // mais le PGN stocke doit conserver les variantes imbriquees pour la
        // reouverture de la partie (voir pgn.rs)
        assert!(pgn_moves.contains("Bc4"), "variation move Bc4 missing: {pgn_moves}");
        assert!(pgn_moves.contains("Qh5"), "variation move Qh5 missing: {pgn_moves}");
        assert!(pgn_moves.contains("Nf6"), "nested variation move Nf6 missing: {pgn_moves}");
        assert!(pgn_moves.contains('('), "opening paren missing: {pgn_moves}");
        assert!(pgn_moves.contains(')'), "closing paren missing: {pgn_moves}");
    }

    // Cas reel qui a revele le bug : un coup qui donne echec/mat ("Qxf7#")
    // gardait son suffixe dans mainline_sans, alors que le reste de l'appli
    // (arbre de partie, legal_moves) ne l'a jamais -- rendait ce coup
    // impossible a cliquer depuis l'explorateur et cassait la recherche de
    // parties une fois rejoue manuellement (voir migrate_strip_check_suffixes,
    // db.rs).
    #[test]
    fn strips_check_and_mate_suffixes_from_stored_mainline() {
        let pgn = r#"[Event "Test Event"]
[Site "Test Site"]
[Date "2024.01.01"]
[White "Alice"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Qh5 Nc6 3. Qxf7# 1-0
"#;
        let mut conn = fresh_conn();
        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |_, _| None).unwrap();
        assert_eq!(summary.imported, 1);

        let (pgn_moves, mainline_sans): (String, String) = conn
            .query_row("SELECT pgn_moves, mainline_sans FROM games LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();

        // mainline_sans (utilise pour la recherche de position/l'explorateur)
        // sans le suffixe, coherent avec le SAN stocke partout ailleurs
        assert_eq!(mainline_sans, "e4 e5 Qh5 Nc6 Qxf7");
        // le PGN reellement stocke, lui, garde l'annotation d'echec/mat
        // (correct/attendu quand on relit ce texte dans un autre logiciel)
        assert!(pgn_moves.contains("Qxf7#"), "check/mate suffix stripped from stored PGN text: {pgn_moves}");
    }

    #[test]
    fn rejects_games_with_a_missing_or_unknown_player_name() {
        let pgn = r#"[Event "Test Event"]
[Site "Test Site"]
[Date "2024.01.01"]
[Round "1"]
[White ""]
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

1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 5. e3 O-O 0-1

[Event "Test Event 3"]
[Site "Test Site"]
[Date "2024.01.03"]
[Round "3"]
[White "Eve"]
[Black "?"]
[Result "1/2-1/2"]

1. e4 e5 2. Nf3 1/2-1/2
"#;
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT, start_fen TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();

        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |_, _| None).unwrap();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped_invalid, 2);
        assert_eq!(summary.failed, 0);

        let actual: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        assert_eq!(actual, 1);
    }

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT, start_fen TEXT,
                white_fide_id TEXT, black_fide_id TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();
        conn
    }

    fn fide_conn_with(players: &[&str]) -> Connection {
        let conn = crate::fide::open_at(std::path::Path::new(":memory:")).unwrap();
        for (i, name) in players.iter().enumerate() {
            conn.execute(
                "INSERT INTO fide_players (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, 'NED', 'M', '', '', '', NULL, NULL, NULL, NULL, '')",
                rusqlite::params![i.to_string(), name],
            )
            .unwrap();
        }
        conn
    }

    // Verifie bulk_import de bout en bout (pas juste fide::canonicalize_name
    // isolement) : deux parties DIFFERENTES du meme joueur, orthographiees
    // differemment ("Giri,A" / "Giri, Anish") dans le fichier source --
    // doivent toutes les deux s'importer (pas de faux doublon) mais sous la
    // MEME forme canonique, pour ne pas fragmenter le joueur en deux entrees
    // dans player_stats/l'explorateur.
    #[test]
    fn bulk_import_canonicalizes_names_without_merging_distinct_games() {
        let pgn = r#"[Event "E1"]
[Site "S"]
[Date "2024.01.01"]
[White "Giri,A"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0

[Event "E2"]
[Site "S"]
[Date "2024.01.02"]
[White "Giri, Anish"]
[Black "Carol"]
[Result "0-1"]

1. d4 d5 2. c4 e6 0-1
"#;
        let mut conn = fresh_conn();
        let fide_conn = fide_conn_with(&["Giri, Anish"]);
        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |raw, elo| crate::fide::canonicalize(&fide_conn, raw, elo)).unwrap();
        assert_eq!(summary.imported, 2, "deux parties distinctes, aucune ne doit etre prise pour un doublon de l'autre");

        let whites: Vec<String> = conn
            .prepare("SELECT white FROM games ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(whites, vec!["Giri, Anish".to_string(), "Giri, Anish".to_string()]);
    }

    // Meme principe, mais avec la MEME partie (mêmes coups/resultat/date)
    // presente deux fois sous deux orthographes differentes du meme joueur --
    // sans canonicalisation, find_duplicate (comparaison stricte sur le nom)
    // laisserait passer un doublon.
    #[test]
    fn bulk_import_canonicalization_lets_duplicate_detection_catch_name_variants() {
        let pgn = r#"[Event "E1"]
[Site "S"]
[Date "2024.01.01"]
[White "Giri,A"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0

[Event "E2"]
[Site "S"]
[Date "2024.01.01"]
[White "Giri, Anish"]
[Black "Bob"]
[Result "1-0"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0
"#;
        let mut conn = fresh_conn();
        let fide_conn = fide_conn_with(&["Giri, Anish"]);
        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |raw, elo| crate::fide::canonicalize(&fide_conn, raw, elo)).unwrap();
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped_duplicates, 1);
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
        let summary = bulk_import(&mut conn, pgn.as_bytes(), |_| {}, || false, |_, _| None).unwrap();
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
            pgn.as_bytes(),
            |_| {},
            move || {
                checks += 1;
                checks > 15
            },
            |_, _| None,
        )
        .unwrap();
        assert!(summary.total > BATCH_SIZE, "devrait avoir depasse le premier lot");
        assert!(summary.total < count, "devrait s'etre arrete avant la fin");

        let actual: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        assert_eq!(actual, summary.imported as i64);
    }
}
