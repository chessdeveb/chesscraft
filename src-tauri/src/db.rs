use crate::explorer;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveResult {
    Saved { id: i64 },
    Duplicate { existing: GameSummary },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub event: String,
    pub time_control: String,
    pub white_elo: Option<i32>,
    pub black_elo: Option<i32>,
}

/// Comme GameSummary, avec les classements Elo -- propre a la liste "parties
/// dans cette position" de l'explorateur d'ouvertures (voir games_at_zobrist),
/// pas au panneau Parties principal, qui n'en a pas besoin. Struct separee
/// plutot que d'ajouter ces deux champs a GameSummary : celui-ci est partage
/// par plusieurs requetes (list_games_page...) qu'il aurait fallu toutes
/// modifier pour peupler des colonnes dont elles n'ont pas besoin.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerGameSummary {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub event: String,
    pub time_control: String,
    pub white_elo: Option<i32>,
    pub black_elo: Option<i32>,
}

fn map_explorer_game_summary_row(row: &rusqlite::Row) -> rusqlite::Result<ExplorerGameSummary> {
    Ok(ExplorerGameSummary {
        id: row.get(0)?,
        white: or_default(row.get(1)?, "?"),
        black: or_default(row.get(2)?, "?"),
        date: or_default(row.get(3)?, "????.??.??"),
        result: or_default(row.get(4)?, "*"),
        event: or_default(row.get(5)?, "?"),
        time_control: or_default(row.get(6)?, "?"),
        white_elo: row.get(7)?,
        black_elo: row.get(8)?,
    })
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
    // le mode WAL permet a d'autres connexions de lire pendant qu'un import de
    // masse ecrit ; le busy_timeout evite un echec immediat ("database is
    // locked") en cas de bref chevauchement d'ecriture.
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS games (
            id INTEGER PRIMARY KEY,
            white TEXT,
            black TEXT,
            white_elo INTEGER,
            black_elo INTEGER,
            result TEXT,
            event TEXT,
            site TEXT,
            round TEXT,
            date_played TEXT,
            eco TEXT,
            opening_name TEXT,
            ply_count INTEGER,
            time_control TEXT,
            pgn_moves TEXT NOT NULL,
            extra_tags TEXT,
            mainline_sans TEXT,
            start_fen TEXT,
            created_at INTEGER NOT NULL,
            last_modified INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_games_white ON games(white COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_games_black ON games(black COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_games_date ON games(date_played);
        CREATE INDEX IF NOT EXISTS idx_games_time_control ON games(time_control);
        CREATE INDEX IF NOT EXISTS idx_games_dedup ON games(white, black, date_played, result, mainline_sans);
        CREATE TABLE IF NOT EXISTS player_stats (
            name TEXT NOT NULL COLLATE NOCASE,
            games INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_player_stats_name ON player_stats(name);
        CREATE TABLE IF NOT EXISTS db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    // Index de position (voir explorer::POSITION_TABLES_SQL) : tables creees
    // vides ici (instantane), remplies par explorer::index_game a chaque
    // enregistrement ou, pour une base existante, par "Recalculer les stats"
    // (explorer::rebuild_position_index) -- jamais depuis open().
    conn.execute_batch(explorer::POSITION_TABLES_SQL).map_err(|e| e.to_string())?;
    migrate_add_columns(&conn)?;
    migrate_add_fide_id_columns(&conn)?;
    migrate_mainline_index(&conn)?;
    migrate_white_elo_index(&conn)?;
    migrate_name_indexes(&conn)?;
    // migrate_strip_check_suffixes n'est PAS appelee ici : sur une base de
    // reference de plusieurs millions de parties, c'est une reecriture
    // couteuse (potentiellement la plupart des lignes + deux index a jour),
    // et open() est appele de facon synchrone/bloquante par de nombreuses
    // commandes (ensure_reference_conn, etc.) -- l'executer ici geleait
    // l'explorateur sans le moindre retour visuel, rapporte en pratique par
    // l'utilisateur ("l'explorateur a l'air planté"). A ne relancer que
    // depuis une tache de fond avec progression (meme mecanique que
    // bulk_import_pgn/refresh_fide_players), pas depuis ce chemin synchrone.
    //
    // positions (voir explorer::rebuild_position_index) N'EST PAS rempli
    // ici -- pour exactement la meme raison : sur une base de reference de
    // plusieurs millions de parties, le peupler demande un scan complet de
    // la table, en rejouant chaque partie avec les vraies regles d'echecs. Deja essaye en tache de fond automatique a
    // l'ouverture (silencieuse, jamais finie avant la fermeture sur une
    // grosse base, repartant de zero a chaque lancement) : rapporte par
    // l'utilisateur comme "beaucoup trop long". Regroupes sous "Recalculer
    // les stats de l'explorateur" (menu Outils, voir
    // lib.rs::rebuild_explorer_stats) : uniquement declenches a la demande
    // explicite de l'utilisateur, avec progression. position_index reste
    // neanmoins a jour au fil de l'eau pour toute NOUVELLE partie
    // enregistree/importee (voir save_game/update_game/delete_game) --
    // seules les parties deja presentes avant cette mise a jour ont besoin
    // de ce rattrapage manuel.
    migrate_player_stats(&mut conn)?;
    Ok(conn)
}

/// Meme principe (verification bon marche a chaque ouverture, precalcul
/// couteux une seule fois si necessaire) pour la liste des joueurs : sans
/// precalcul, compter les joueurs distincts d'une base de plusieurs millions
/// de parties (UNION Blancs+Noirs, GROUP BY) prend potentiellement plusieurs
/// minutes -- observe en pratique sur la base de reference de l'utilisateur.
/// Existence (LIMIT 1), pas un COUNT(*) exact -- meme raison que
/// migrate_prefix_stats : seul zero/non-zero importe, et cette verification
/// tourne a chaque ouverture de la base.
fn migrate_player_stats(conn: &mut Connection) -> Result<(), String> {
    let already_built: Option<i64> =
        conn.query_row("SELECT 1 FROM player_stats LIMIT 1", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    if already_built.is_some() {
        return Ok(());
    }
    let has_games: Option<i64> =
        conn.query_row("SELECT 1 FROM games LIMIT 1", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    if has_games.is_none() {
        return Ok(());
    }
    rebuild_player_stats(conn, |_| {})
}

/// La recherche de parties par nom (prefixe insensible a la casse) s'appuie
/// sur des intervalles explicites (col >= ? AND col < ?) plutot que sur LIKE
/// (voir migrate_mainline_index pour le meme constat sur l'explorateur) :
/// pour que cet intervalle utilise l'index au lieu d'un scan complet, la
/// colonne indexee doit deja etre en collation NOCASE. Recree seulement si
/// l'index existant est encore dans son ancienne forme (collation binaire).
fn migrate_name_indexes(conn: &Connection) -> Result<(), String> {
    for (name, column) in [("idx_games_white", "white"), ("idx_games_black", "black")] {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let up_to_date = matches!(&sql, Some(s) if s.contains("COLLATE NOCASE"));
        if !up_to_date {
            conn.execute_batch(&format!(
                "DROP INDEX IF EXISTS {name};
                 CREATE INDEX {name} ON games({column} COLLATE NOCASE);"
            ))
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// L'explorateur d'ouvertures ne lit que mainline_sans et result : un index
/// couvrant les deux evite a SQLite de retourner a la table pour chaque ligne
/// (et l'empeche de preferer a tort un scan complet de idx_games_dedup, plus
/// large, faute de pouvoir "couvrir" la requete avec un index a une seule
/// colonne). Recree seulement si absent ou encore dans son ancienne forme a
/// une colonne, pas a chaque ouverture.
fn migrate_mainline_index(conn: &Connection) -> Result<(), String> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_games_mainline'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let up_to_date = matches!(&sql, Some(s) if s.contains("mainline_sans, result"));
    if !up_to_date {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_games_mainline;
             CREATE INDEX idx_games_mainline ON games(mainline_sans, result);",
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// idx_games_white_elo (filtre par classement de l'explorateur, voir
/// explorer::ExplorerFilters/zobrist_from_and_where) porte sur white_elo,
/// une colonne ajoutee apres coup par migrate_add_columns : ne peut donc pas
/// etre cree dans le CREATE TABLE IF NOT EXISTS initial (echouerait sur une
/// base plus ancienne, ou la colonne n'existe pas encore a ce moment-la) --
/// doit s'executer apres migrate_add_columns, comme migrate_mainline_index
/// pour mainline_sans.
fn migrate_white_elo_index(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_games_white_elo ON games(white_elo);").map_err(|e| e.to_string())
}

/// Supprime les restes des anciennes recherches de position : l'index
/// idx_games_start_fen, la table opening_prefix_stats (recherche par texte de
/// coups) et l'ancienne table position_index (remplacee par `positions`),
/// potentiellement tres volumineuses sur une grosse base. Plus rien ne les lit.
/// Appelee depuis "Recalculer les stats" (lib.rs::rebuild_explorer_stats),
/// pas depuis open().
pub fn drop_legacy_explorer_structures(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_games_start_fen; DROP TABLE IF EXISTS opening_prefix_stats; DROP TABLE IF EXISTS position_index;")
        .map_err(|e| e.to_string())
}

/// VACUUM si plus d'un cinquieme du fichier est constitue de pages libres
/// (tables supprimees : l'ancienne position_index representait des dizaines
/// de Go) -- SQLite ne rend jamais cet espace au disque autrement. Long sur une
/// grosse base (reecriture complete du fichier) : uniquement depuis "Recalculer
/// les stats", jamais depuis open(). `on_start` est appele juste avant, pour
/// que l'interface puisse l'annoncer.
pub fn compact_if_worthwhile(conn: &Connection, on_start: impl FnOnce()) -> Result<(), String> {
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if pages == 0 || free * 5 < pages {
        return Ok(());
    }
    on_start();
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(|e| e.to_string())?;
    conn.execute_batch("VACUUM").map_err(|e| e.to_string())?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(|e| e.to_string())?;
    Ok(())
}

/// Avant ce correctif, mainline_sans etait rempli lors de l'import en masse
/// (voir bulk_import::BulkGameVisitor::san) avec le SAN "complet" du fichier
/// PGN source, "+"/"#" inclus pour un coup qui donne echec/mat -- alors que
/// partout ailleurs dans l'appli (arbre de partie, legal_moves, moteur, voir
/// pgn::san/game_tree::legal_moves/engine::resolve_line) le SAN stocke/compare
/// n'a jamais ce suffixe (shakmaty::San, pas SanPlus). Consequence concrete
/// rapportee par l'utilisateur : un coup avec "+" dans l'explorateur ne
/// pouvait pas se jouer au clic (aucune correspondance dans legal_moves), et
/// rejouer ce coup manuellement faisait ensuite echouer la recherche de
/// parties a cette position (le prefixe de mainline_sans envoye, sans "+",
/// ne correspondait plus a la borne d'intervalle attendue par SQLite).
/// Corrige a la source cote bulk_import pour les
/// imports futurs ; cette migration nettoie les bases deja importees. "+"/"#"
/// ne servent qu'a cette annotation en SAN, jamais a autre chose : un simple
/// retrait de ces caracteres est donc strictement equivalent au SAN sans
/// suffixe utilise partout ailleurs, sans ambiguite possible.
///
/// PAS appelee depuis open() : sur une base de reference de plusieurs
/// millions de parties c'est une reecriture couteuse, et open() est sur le
/// chemin synchrone de nombreuses commandes -- geleait l'explorateur sans le
/// moindre retour visuel (premiere version de ce correctif). Concue au
/// contraire pour tourner sur une connexion separee, depuis une tache de
/// fond (voir lib.rs::maybe_start_check_suffix_cleanup) : paginee par id
/// (empreinte memoire bornee a une page, jamais toute la table en RAM,
/// contrairement a un unique UPDATE ... WHERE mainline_sans LIKE '%+%' qui
/// materialiserait/verrouillerait tout d'un coup) et interrompue au prochain
/// controle si `should_cancel` repond vrai, un commit par page deja fait
/// restant acquis. `on_progress` recoit le nombre de parties nettoyees
/// jusqu'ici (pas de total connu a l'avance -- le calculer couterait un
/// balayage complet supplementaire).
pub fn already_check_suffixes_cleaned(conn: &Connection) -> Result<bool, String> {
    let done: Option<String> = conn
        .query_row("SELECT value FROM db_meta WHERE key = 'check_suffixes_stripped'", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(done.is_some())
}

const STRIP_CHECK_SUFFIXES_PAGE_SIZE: i64 = 5000;

pub fn strip_check_suffixes(
    conn: &mut Connection,
    mut on_progress: impl FnMut(u32),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<u32, String> {
    if already_check_suffixes_cleaned(conn)? {
        return Ok(0);
    }

    let mut cleaned = 0u32;
    let mut last_id = 0i64;
    loop {
        if should_cancel() {
            return Ok(cleaned);
        }
        let page: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, mainline_sans FROM games WHERE id > ?1 ORDER BY id LIMIT ?2")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![last_id, STRIP_CHECK_SUFFIXES_PAGE_SIZE], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default()))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut update = tx.prepare("UPDATE games SET mainline_sans = ?1 WHERE id = ?2").map_err(|e| e.to_string())?;
            for (id, mainline) in &page {
                if mainline.contains(['+', '#']) {
                    let cleaned_mainline: String = mainline.chars().filter(|c| *c != '+' && *c != '#').collect();
                    update.execute(params![cleaned_mainline, id]).map_err(|e| e.to_string())?;
                    cleaned += 1;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        on_progress(cleaned);
    }

    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('check_suffixes_stripped', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(cleaned)
}

/// Lit le tag FEN d'un texte PGN complet (tags + movetext) tel que stocke
/// dans games.pgn_moves -- les tags sont toujours groupes en tete, une paire
/// par ligne ("[Cle \"Valeur\"]"), separes du movetext par une ligne vide
/// (voir assemble_pgn/write_game, seules sources de ce contenu). S'arrete des
/// la premiere ligne qui n'est ni un tag ni vide : inutile de lire le
/// movetext (potentiellement long) pour une seule valeur de tag.
fn extract_fen_tag(pgn: &str) -> Option<String> {
    for line in pgn.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else { break };
        let Some(rest) = rest.strip_prefix("FEN ") else { continue };
        let rest = rest.trim().strip_suffix(']')?.trim();
        let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
        return Some(inner.to_string());
    }
    None
}

/// Vrai si backfill_start_fen a deja tourne sur cette base -- evite de tout
/// rescanner a chaque appel une fois fait (meme principe que
/// already_check_suffixes_cleaned).
pub fn start_fen_backfilled(conn: &Connection) -> Result<bool, String> {
    let done: Option<String> = conn
        .query_row("SELECT value FROM db_meta WHERE key = 'start_fen_backfilled'", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(done.is_some())
}

const BACKFILL_START_FEN_PAGE_SIZE: i64 = 5000;

/// Retrouve start_fen (voir normalize_start_fen) pour les parties deja
/// enregistrees AVANT l'ajout de cette colonne : ALTER TABLE ADD COLUMN la
/// laisse a NULL pour toute ligne existante, meme celle qui porte un tag FEN
/// personnalise dans son pgn_moves deja stocke. Indispensable AVANT
/// explorer::rebuild_position_index, qui rejoue chaque partie depuis son
/// start_fen : sans lui, une partie a depart personnalise serait rejouee
/// depuis la position standard et mal indexee. Ne retouche que les lignes qui
/// ont REELLEMENT un tag FEN non standard : les vraies parties standard
/// restent a NULL, exactement le comportement attendu de normalize_start_fen.
/// Meme mecanique paginee/annulable que strip_check_suffixes (voir plus
/// haut) ; `on_progress` recoit le nombre de lignes examinees jusqu'ici (pas
/// juste celles modifiees), pas de total connu a l'avance.
pub fn backfill_start_fen(
    conn: &mut Connection,
    mut on_progress: impl FnMut(u64),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<u64, String> {
    if start_fen_backfilled(conn)? {
        return Ok(0);
    }

    let mut examined = 0u64;
    let mut updated = 0u64;
    let mut last_id = 0i64;
    loop {
        if should_cancel() {
            return Ok(updated);
        }
        let page: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, pgn_moves FROM games WHERE id > ?1 AND start_fen IS NULL ORDER BY id LIMIT ?2")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![last_id, BACKFILL_START_FEN_PAGE_SIZE], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut update = tx.prepare("UPDATE games SET start_fen = ?1 WHERE id = ?2").map_err(|e| e.to_string())?;
            for (id, pgn) in &page {
                if let Some(start_fen) = normalize_start_fen(extract_fen_tag(pgn).as_deref()) {
                    update.execute(params![start_fen, id]).map_err(|e| e.to_string())?;
                    updated += 1;
                }
                examined += 1;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        on_progress(examined);
    }

    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('start_fen_backfilled', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(updated)
}

/// Les bases creees avant l'ajout du support Elo / du cache des coups n'ont
/// pas ces colonnes ; "CREATE TABLE IF NOT EXISTS" ne les ajoute pas
/// retroactivement.
fn migrate_add_columns(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn.prepare("PRAGMA table_info(games)").map_err(|e| e.to_string())?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    if !existing.iter().any(|c| c == "white_elo") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN white_elo INTEGER;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "black_elo") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN black_elo INTEGER;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "mainline_sans") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN mainline_sans TEXT;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "start_fen") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN start_fen TEXT;")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Identifiant FIDE (voir fide::canonicalize) du Blancs/Noirs d'une partie,
/// quand la canonisation a resolu un candidat unique et non ambigu -- NULL
/// sinon (nom non reconnu, ou encore ambigu meme apres depart par Elo). Les
/// bases importees avant l'ajout de cette resolution n'ont ni la colonne ni,
/// bien sur, la valeur : voir db::backfill_fide_ids (menu Outils) pour la
/// renseigner retroactivement sur une base existante.
fn migrate_add_fide_id_columns(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn.prepare("PRAGMA table_info(games)").map_err(|e| e.to_string())?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    if !existing.iter().any(|c| c == "white_fide_id") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN white_fide_id TEXT;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "black_fide_id") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN black_fide_id TEXT;")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn or_default(v: Option<String>, default: &str) -> String {
    v.unwrap_or_else(|| default.to_string())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// FEN de la position de depart standard, telle qu'ecrite par shakmaty
/// (Fen::from_position(Chess::default())) -- sert a normaliser un tag FEN
/// "explicite mais standard" (rare, mais possible selon la source du PGN) en
/// None, pour qu'il rejoigne bien le meme groupe que les parties standard qui
/// n'ont pas de tag FEN du tout (voir normalize_start_fen).
const STANDARD_START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Position de depart d'une partie, stockee dans la colonne start_fen : None
/// pour le depart standard (le cas courant), Some(fen) pour une partie
/// demarree d'une position personnalisee (Nouvelle position) ou Chess960 --
/// meme convention que pgn.rs (presence du tag FEN, SetUp jamais verifie).
/// Purement informatif pour la recherche de position (voir
/// explorer::index_game/zobrist_from_and_where, insensibles a start_fen),
/// conserve pour l'affichage/export.
pub fn normalize_start_fen(fen: Option<&str>) -> Option<String> {
    let fen = fen?.trim();
    if fen.is_empty() || fen == STANDARD_START_FEN {
        None
    } else {
        Some(fen.to_string())
    }
}

pub fn save_game(
    conn: &Connection,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<i64, String> {
    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let start_fen = normalize_start_fen(tags.get("FEN").map(|s| s.as_str()));
    let timestamp = now();
    conn.execute(
        "INSERT INTO games (white, black, white_elo, black_elo, result, event, site, round, date_played, time_control, pgn_moves, mainline_sans, start_fen, created_at, last_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
        params![
            get("White"),
            get("Black"),
            get_elo("WhiteElo"),
            get_elo("BlackElo"),
            get("Result"),
            get("Event"),
            get("Site"),
            get("Round"),
            get("Date"),
            get("TimeControl"),
            pgn,
            mainline_sans,
            start_fen,
            timestamp,
        ],
    )
    .map_err(|e| e.to_string())?;
    // last_insert_rowid() doit etre capture tout de suite : l'appel suivant
    // fait lui-meme des INSERT (dans position_index), qui l'ecraserait sinon
    // avec le rowid de cette AUTRE table.
    let id = conn.last_insert_rowid();
    // Indexe cette partie par position (voir explorer::index_game) : propage
    // l'erreur (contrairement a player_stats, un simple compteur d'affichage)
    // -- contrairement a l'ancien systeme (texte, "cache de performance" avec
    // repli sur un calcul a la volee), position_index EST la donnee, il n'y a
    // plus de repli possible si une partie n'y figure pas.
    crate::explorer::index_game(conn, id, mainline_sans, start_fen.as_deref(), tags.get("Result").map(|s| s.as_str()))?;
    let _ = increment_player_stats(conn, &get("White").unwrap_or_default(), &get("Black").unwrap_or_default());
    Ok(id)
}

/// A appeler juste apres save_game quand l'import a mass (seul appelant a ce
/// jour, voir bulk_import::process_one_game) a resolu un identifiant FIDE
/// pour Blancs et/ou Noirs (voir fide::canonicalize) -- separe de save_game
/// pour ne pas faire porter ces deux colonnes, utiles seulement a l'import en
/// masse avec base FIDE presente, a tous ses autres appelants (partie unique
/// enregistree/deplacee, tests...).
pub fn set_game_fide_ids(conn: &Connection, id: i64, white_fide_id: Option<&str>, black_fide_id: Option<&str>) -> Result<(), String> {
    conn.execute(
        "UPDATE games SET white_fide_id = ?1, black_fide_id = ?2 WHERE id = ?3",
        params![white_fide_id, black_fide_id, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_game(
    conn: &Connection,
    id: i64,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<(), String> {
    // Ancien contenu : Blancs/Noirs pour decrementer player_stats, coups et
    // position de depart pour retirer les anciennes lignes de `positions`
    // (voir explorer::deindex_game) avant d'indexer le nouveau contenu.
    let old = conn
        .query_row("SELECT white, black, mainline_sans, start_fen FROM games WHERE id = ?1", params![id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .optional()
        .map_err(|e| e.to_string())?;

    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let start_fen = normalize_start_fen(tags.get("FEN").map(|s| s.as_str()));
    let timestamp = now();
    conn.execute(
        "UPDATE games SET white = ?1, black = ?2, white_elo = ?3, black_elo = ?4, result = ?5, event = ?6,
         site = ?7, round = ?8, date_played = ?9, time_control = ?10, pgn_moves = ?11, mainline_sans = ?12, start_fen = ?13, last_modified = ?14 WHERE id = ?15",
        params![
            get("White"),
            get("Black"),
            get_elo("WhiteElo"),
            get_elo("BlackElo"),
            get("Result"),
            get("Event"),
            get("Site"),
            get("Round"),
            get("Date"),
            get("TimeControl"),
            pgn,
            mainline_sans,
            start_fen,
            timestamp,
            id,
        ],
    )
    .map_err(|e| e.to_string())?;

    if let Some((old_white, old_black, old_mainline, old_start_fen)) = &old {
        crate::explorer::deindex_game(conn, id, old_mainline.as_deref().unwrap_or_default(), old_start_fen.as_deref())?;
        let _ = decrement_player_stats(conn, old_white.as_deref().unwrap_or_default(), old_black.as_deref().unwrap_or_default());
    }
    crate::explorer::index_game(conn, id, mainline_sans, start_fen.as_deref(), tags.get("Result").map(|s| s.as_str()))?;
    let _ = increment_player_stats(conn, &get("White").unwrap_or_default(), &get("Black").unwrap_or_default());
    Ok(())
}

pub fn delete_game(conn: &Connection, id: i64) -> Result<(), String> {
    let old = conn
        .query_row("SELECT white, black, mainline_sans, start_fen FROM games WHERE id = ?1", params![id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .optional()
        .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM games WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;

    if let Some((old_white, old_black, old_mainline, old_start_fen)) = old {
        crate::explorer::deindex_game(conn, id, old_mainline.as_deref().unwrap_or_default(), old_start_fen.as_deref())?;
        let _ = decrement_player_stats(conn, old_white.as_deref().unwrap_or_default(), old_black.as_deref().unwrap_or_default());
    }
    Ok(())
}

fn map_game_summary_row(row: &rusqlite::Row) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        id: row.get(0)?,
        white: or_default(row.get(1)?, "?"),
        black: or_default(row.get(2)?, "?"),
        date: or_default(row.get(3)?, "????.??.??"),
        result: or_default(row.get(4)?, "*"),
        event: or_default(row.get(5)?, "?"),
        time_control: or_default(row.get(6)?, "?"),
        white_elo: row.get(7)?,
        black_elo: row.get(8)?,
    })
}

/// Filtres de recherche des parties d'une base. `search` (recherche rapide,
/// Blancs OU Noirs) et les filtres avances (Blancs/Noirs/Resultat/periode,
/// combines en ET) sont mutuellement exclusifs cote interface : si `search`
/// est renseigne, les filtres avances sont ignores.
#[derive(Default)]
pub struct GamesFilter {
    pub search: String,
    pub white: String,
    pub black: String,
    pub result: String,
    pub date_from: String,
    pub date_to: String,
    pub time_control: String,
}

/// Bornes d'un intervalle "commence par" insensible a la casse.
fn prefix_bounds(value: &str) -> (String, String) {
    (value.to_string(), format!("{value}\u{ffff}"))
}

/// Construit la source ("FROM ...", eventuellement avec INDEXED BY ou une
/// sous-requete) et la clause WHERE (avec ses parametres, dans l'ordre des
/// `?N`) a partir d'un `GamesFilter`. Les valeurs viennent de l'utilisateur
/// et ne sont jamais interpolees telles quelles dans le SQL (injection).
///
/// Prefixe (col >= 'x' AND col < 'x\u{ffff}') plutot que "contient"
/// (LIKE '%x%') : sur une base de plusieurs millions de parties, seul un
/// prefixe permet d'utiliser un index au lieu de parcourir toute la table.
///
/// Deux precautions contre le planificateur de requetes de SQLite, constatees
/// a plusieurs reprises sur cette base (ici et dans l'explorateur) :
/// - un intervalle explicite est utilise plutot que LIKE 'x%', qui ne
///   declenche pas de facon fiable l'optimisation d'index de SQLite ;
/// - `INDEXED BY` force explicitement l'index attendu (idx_games_white /
///   idx_games_black), le planificateur ayant deja choisi a tort de scanner
///   entierement idx_games_dedup (le considerant "couvrant") plutot que de
///   chercher dans l'index dedie, beaucoup plus selectif.
fn build_from_and_where(filter: &GamesFilter) -> (String, String, Vec<Box<dyn rusqlite::ToSql>>) {
    if !filter.search.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.search);
        // Reunion de deux recherches d'index dediees plutot qu'un OR sur
        // white/black dans une seule requete : force chaque branche a
        // utiliser son propre index (INDEXED BY), impossible a exprimer
        // sur une seule reference a `games` avec un OR.
        let from = "(SELECT id FROM games INDEXED BY idx_games_white
                       WHERE white COLLATE NOCASE >= ?1 AND white COLLATE NOCASE < ?2
                     UNION
                     SELECT id FROM games INDEXED BY idx_games_black
                       WHERE black COLLATE NOCASE >= ?1 AND black COLLATE NOCASE < ?2) AS matched
                    JOIN games ON games.id = matched.id"
            .to_string();
        return (from, String::new(), vec![Box::new(lo), Box::new(hi)]);
    }

    let mut clauses = Vec::new();
    let mut ps: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if !filter.white.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.white);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!(
            "(white COLLATE NOCASE >= ?{} AND white COLLATE NOCASE < ?{})",
            ps.len() - 1,
            ps.len()
        ));
    }
    if !filter.black.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.black);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!(
            "(black COLLATE NOCASE >= ?{} AND black COLLATE NOCASE < ?{})",
            ps.len() - 1,
            ps.len()
        ));
    }
    if !filter.result.is_empty() {
        ps.push(Box::new(filter.result.clone()));
        clauses.push(format!("result = ?{}", ps.len()));
    }
    if !filter.date_from.is_empty() {
        ps.push(Box::new(filter.date_from.clone()));
        clauses.push(format!("date_played >= ?{}", ps.len()));
    }
    if !filter.date_to.is_empty() {
        ps.push(Box::new(filter.date_to.clone()));
        clauses.push(format!("date_played <= ?{}", ps.len()));
    }
    if !filter.time_control.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.time_control);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!("(time_control >= ?{} AND time_control < ?{})", ps.len() - 1, ps.len()));
    }

    // Un seul filtre sur un nom (sans l'autre) : force son index dedie pour
    // le meme motif que ci-dessus. Combiner white ET black (AND), ou ne
    // filtrer que par resultat/date, reste sur le choix du planificateur.
    let from = if !filter.white.is_empty() && filter.black.is_empty() {
        "games INDEXED BY idx_games_white".to_string()
    } else if filter.white.is_empty() && !filter.black.is_empty() {
        "games INDEXED BY idx_games_black".to_string()
    } else if filter.white.is_empty() && filter.black.is_empty() && !filter.time_control.is_empty() {
        "games INDEXED BY idx_games_time_control".to_string()
    } else {
        "games".to_string()
    };

    let where_clause = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
    (from, where_clause, ps)
}

/// Nombre de parties correspondant aux filtres, ou de la base entiere si
/// aucun filtre n'est actif.
pub fn count_games(conn: &Connection, filter: &GamesFilter) -> Result<u32, String> {
    let (from, where_clause, params) = build_from_and_where(filter);
    let sql = format!("SELECT COUNT(*) FROM {from} {where_clause}");
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let count: i64 = conn
        .query_row(&sql, params_refs.as_slice(), |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

/// Traduit une paire (colonne, sens) fournie par l'interface en fragment SQL
/// fixe : `sort_by`/`sort_dir` viennent de l'utilisateur et ne doivent jamais
/// etre interpoles tels quels dans la requete (injection SQL). Retombe sur le
/// tri par defaut (le plus recent d'abord) pour toute valeur inconnue. Les
/// colonnes sont qualifiees par `games.` : le FROM peut etre une jointure
/// avec une sous-requete (voir build_from_and_where), ou `id`/`white`/`black`
/// seraient ambigus.
fn order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("white", false) => "ORDER BY games.white COLLATE NOCASE ASC, games.id DESC",
        ("white", true) => "ORDER BY games.white COLLATE NOCASE DESC, games.id DESC",
        ("black", false) => "ORDER BY games.black COLLATE NOCASE ASC, games.id DESC",
        ("black", true) => "ORDER BY games.black COLLATE NOCASE DESC, games.id DESC",
        ("date", false) => "ORDER BY games.date_played ASC, games.id DESC",
        ("date", true) => "ORDER BY games.date_played DESC, games.id DESC",
        _ => "ORDER BY games.id DESC",
    }
}

/// Une page de parties, filtrees selon `filter` et triees selon
/// `sort_by`/`sort_dir` (voir `order_by_clause`). Indispensable sur les
/// grosses bases (des centaines de milliers de parties) ou charger la table
/// entiere en un coup n'est pas envisageable.
/// Parties (joueurs, Elo, resultat) pour le calcul de performance Elo du
/// panneau Parties (voir performance::compute).
pub fn load_games_for_performance(conn: &Connection, ids: &[i64]) -> Result<Vec<crate::performance::PerfGame>, String> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!("SELECT white, black, white_elo, black_elo, result FROM games WHERE id IN ({placeholders})");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok(crate::performance::PerfGame {
                    white: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    black: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    white_elo: row.get(2)?,
                    black_elo: row.get(3)?,
                    result: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}

pub fn list_games_page(
    conn: &Connection,
    filter: &GamesFilter,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<GameSummary>, String> {
    let order_by = order_by_clause(sort_by, sort_dir);
    let (from, where_clause, mut params) = build_from_and_where(filter);
    let limit_ph = params.len() + 1;
    let offset_ph = params.len() + 2;
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    let sql = format!(
        "SELECT games.id, games.white, games.black, games.date_played, games.result, games.event, games.time_control, games.white_elo, games.black_elo FROM {from}
         {where_clause} {order_by} LIMIT ?{limit_ph} OFFSET ?{offset_ph}"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), map_game_summary_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

/// FROM+WHERE partages par count_at_zobrist et games_at_zobrist : jointure
/// entre `games` et `position_index` (voir explorer::index_game) sur le hash
/// Zobrist de la position -- contrairement a l'ancien position_from_and_where
/// (texte SAN + start_fen), un hash identifie une ARRANGEMENT du plateau
/// (pieces + trait + roques + colonne en-passant), donc capture aussi les
/// transpositions et les parties parties d'une position de depart differente
/// (standard/Chess960/personnalisee) qui aboutissent au meme plateau.
fn zobrist_from_and_where(
    zobrist: i64,
    filters: &crate::explorer::ExplorerFilters,
) -> (String, String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(zobrist)];
    let mut clauses: Vec<String> = vec!["p.zobrist = ?1".to_string()];

    if let Some(p) = &filters.player {
        clauses.push(p.where_clause(&mut params));
    }
    if let Some((min, max)) = filters.rating_range {
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("white_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("black_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
    }
    if let Some((year_min, year_max)) = filters.year_range {
        params.push(Box::new(format!("{year_min:04}")));
        params.push(Box::new(format!("{year_max:04}\u{ffff}")));
        clauses.push(format!("date_played >= ?{} AND date_played < ?{}", params.len() - 1, params.len()));
    }

    // Avec un filtre joueur, on part de `games` (index sur le nom) et on
    // cherche chaque partie dans positions par sa cle primaire ; sinon, sur la
    // position de depart, on parcourrait 10 M de lignes (voir
    // explorer::stats_query).
    let from = if filters.player.is_some() {
        "games g CROSS JOIN positions p ON p.game_id = g.id AND p.zobrist = ?1".to_string()
    } else {
        "games g JOIN positions p ON p.game_id = g.id".to_string()
    };
    let where_clause = format!("WHERE {}", clauses.join(" AND "));
    (from, where_clause, params)
}

/// Nombre de parties (distinctes) de la base de reference passant par cette
/// position (identifiee par son hash Zobrist, voir explorer::current_zobrist),
/// optionnellement restreint par joueur/classement/annee.
pub fn count_at_zobrist(conn: &Connection, zobrist: i64, filters: &crate::explorer::ExplorerFilters) -> Result<u32, String> {
    if filters.is_empty() {
        // Sans filtre : simple parcours de l'intervalle de cle (zobrist) de
        // `positions` (une ligne par partie), sans toucher a `games` -- rapide
        // meme pour la position de depart (10 M+ de parties).
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM positions WHERE zobrist = ?1", params![zobrist], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        return Ok(count as u32);
    }
    let (from, where_clause, params) = zobrist_from_and_where(zobrist, filters);
    let sql = format!("SELECT COUNT(*) FROM {from} {where_clause}");
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0)).map_err(|e| e.to_string())?;
    Ok(count as u32)
}

/// Traduit (colonne, sens) fournis par l'interface en clause ORDER BY fixe
/// (voir order_by_clause, meme principe pour le panneau Parties principal --
/// id en depart-egalite pour un ordre stable entre deux parties de meme
/// valeur). Retombe sur "les plus recentes d'abord" (id DESC) pour toute
/// valeur vide/inconnue.
fn explorer_games_order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("white", false) => "ORDER BY white COLLATE NOCASE ASC, id DESC",
        ("white", true) => "ORDER BY white COLLATE NOCASE DESC, id DESC",
        ("black", false) => "ORDER BY black COLLATE NOCASE ASC, id DESC",
        ("black", true) => "ORDER BY black COLLATE NOCASE DESC, id DESC",
        ("date", false) => "ORDER BY date_played ASC, id DESC",
        ("date", true) => "ORDER BY date_played DESC, id DESC",
        ("whiteElo", false) => "ORDER BY white_elo ASC, id DESC",
        ("whiteElo", true) => "ORDER BY white_elo DESC, id DESC",
        ("blackElo", false) => "ORDER BY black_elo ASC, id DESC",
        ("blackElo", true) => "ORDER BY black_elo DESC, id DESC",
        _ => "ORDER BY id DESC",
    }
}

/// Parties de la base de reference passant par cette position (hash
/// Zobrist), triees selon `sort_by`/`sort_dir` (les plus recentes d'abord si
/// vide), optionnellement restreintes par joueur/classement/annee. Utilise
/// pour le panneau "parties dans cette position" de l'explorateur
/// d'ouvertures -- une meme partie peut traverser la position plusieurs fois
/// (repetition), d'ou le DISTINCT sur l'id.
pub fn games_at_zobrist(
    conn: &Connection,
    zobrist: i64,
    filters: &crate::explorer::ExplorerFilters,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<ExplorerGameSummary>, String> {
    // Ordre par defaut : game_id decroissant = parcours a l'envers de
    // l'intervalle de cle (zobrist, game_id) de `positions`, les 200 dernieres
    // parties sont lues sans trier les millions de correspondances. Un tri
    // explicite exige en revanche de tout visiter.
    let order_by = if sort_by.is_empty() { "ORDER BY p.game_id DESC" } else { explorer_games_order_by_clause(sort_by, sort_dir) };
    let (from, where_clause, mut params) = zobrist_from_and_where(zobrist, filters);
    let limit_ph = params.len() + 1;
    let offset_ph = params.len() + 2;
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    let sql = format!(
        "SELECT g.id, g.white, g.black, g.date_played, g.result, g.event, g.time_control, g.white_elo, g.black_elo
         FROM {from} {where_clause}
         {order_by} LIMIT ?{limit_ph} OFFSET ?{offset_ph}"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), map_explorer_game_summary_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSummary {
    pub name: String,
    pub games: u32,
}

/// Reconstruit entierement player_stats a partir des parties actuelles :
/// necessaire apres un import en masse, ou pour la premiere ouverture d'une
/// base importee avant l'ajout de cette table (voir migrate_player_stats).
/// Un seul passage sur white/black, agrege en Rust (HashMap) plutot que par
/// un GROUP BY SQL sur l'union des deux colonnes -- sur la base de reference
/// de l'utilisateur (~10M parties), la version SQL restait bloquee plusieurs
/// minutes (pas d'index utilisable pour un GROUP BY sur toutes les valeurs
/// distinctes), le seul cout ici est celui d'un aller-retour sur la table
/// (meme ordre de grandeur qu'un COUNT(*)). La cle du HashMap est la version
/// en minuscules du nom (les deux colonnes de l'index sont en COLLATE
/// NOCASE, donc "Kasparov,G" et "kasparov,g" doivent fusionner) ; la valeur
/// garde l'orthographe rencontree en premier pour l'affichage.
pub fn rebuild_player_stats(conn: &mut Connection, mut on_progress: impl FnMut(crate::explorer::RebuildProgress)) -> Result<(), String> {
    use crate::explorer::RebuildProgress;
    const PROGRESS_EVERY: u64 = 20_000;
    let tally = {
        let mut stmt = conn.prepare("SELECT white, black FROM games").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)))
            .map_err(|e| e.to_string())?;

        let mut tally: HashMap<String, (String, u64)> = HashMap::new();
        let mut examined = 0u64;
        for row in rows {
            let (white, black) = row.map_err(|e| e.to_string())?;
            for name in [white, black].into_iter().flatten() {
                if name.is_empty() || name == "?" {
                    continue;
                }
                let entry = tally.entry(name.to_lowercase()).or_insert_with(|| (name.clone(), 0));
                entry.1 += 1;
            }
            examined += 1;
            if examined % PROGRESS_EVERY == 0 {
                on_progress(RebuildProgress::Reading { examined });
            }
        }
        on_progress(RebuildProgress::Reading { examined });
        tally
    };

    let total_to_write = tally.len() as u64;
    on_progress(RebuildProgress::Writing { written: 0, total: total_to_write });

    // Meme raison que explorer::rebuild_position_index : validations par lots
    // plutot qu'une seule transaction geante, pour ne pas laisser le WAL
    // grossir sans controle sur une base de plusieurs millions de joueurs
    // distincts.
    conn.execute("DELETE FROM player_stats", []).map_err(|e| e.to_string())?;
    const COMMIT_BATCH_SIZE: u64 = 100_000;
    let mut written = 0u64;
    let mut entries = tally.values();
    loop {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut insert = tx.prepare("INSERT INTO player_stats (name, games) VALUES (?1, ?2)").map_err(|e| e.to_string())?;
            let mut in_batch = 0u64;
            for (display_name, cnt) in entries.by_ref() {
                insert.execute(params![display_name, *cnt as i64]).map_err(|e| e.to_string())?;
                written += 1;
                in_batch += 1;
                if written % PROGRESS_EVERY == 0 {
                    on_progress(RebuildProgress::Writing { written, total: total_to_write });
                }
                if in_batch >= COMMIT_BATCH_SIZE {
                    break;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        if written >= total_to_write {
            break;
        }
    }
    on_progress(RebuildProgress::Writing { written: total_to_write, total: total_to_write });
    Ok(())
}

/// A appeler juste apres l'ajout d'une partie : ajuste player_stats sans tout
/// recalculer (meme principe que explorer::index_game pour position_index).
fn increment_player_stats(conn: &Connection, white: &str, black: &str) -> Result<(), String> {
    for name in [white, black] {
        if name.is_empty() || name == "?" {
            continue;
        }
        conn.execute(
            "INSERT INTO player_stats (name, games) VALUES (?1, 1)
             ON CONFLICT(name) DO UPDATE SET games = games + 1",
            params![name],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Symetrique de increment_player_stats, a appeler juste avant une
/// suppression ou un remplacement de partie.
fn decrement_player_stats(conn: &Connection, white: &str, black: &str) -> Result<(), String> {
    for name in [white, black] {
        if name.is_empty() || name == "?" {
            continue;
        }
        conn.execute("UPDATE player_stats SET games = games - 1 WHERE name = ?1 AND games > 0", params![name])
            .map_err(|e| e.to_string())?;
    }
    conn.execute("DELETE FROM player_stats WHERE games <= 0", []).map_err(|e| e.to_string())?;
    Ok(())
}

/// Clef de regroupement du dedoublonnage pleine base (voir scan_duplicates) :
/// les memes cinq colonnes que find_duplicate (verifie a l'import, partie
/// par partie). NULL est ici toujours ramene a "" avant comparaison -- deux
/// parties sans Blancs/date/... doivent quand meme se regrouper entre elles,
/// pas y echapper faute de valeur (meme effet que le GROUP BY SQL
/// precedemment utilise ici, qui traite deja deux NULL comme egaux).
type DedupKey = (String, String, String, String, String);

fn dedup_key(white: &Option<String>, black: &Option<String>, date: &Option<String>, result: &Option<String>, mainline: &Option<String>) -> DedupKey {
    (
        white.clone().unwrap_or_default(),
        black.clone().unwrap_or_default(),
        date.clone().unwrap_or_default(),
        result.clone().unwrap_or_default(),
        mainline.clone().unwrap_or_default(),
    )
}

/// Parcourt `games` par id croissant en construisant l'ensemble des clefs
/// deja rencontrees (voir dedup_key) : la PREMIERE occurrence d'une clef est
/// donc toujours celle du plus petit id du groupe, sans avoir besoin d'un
/// second passage pour le determiner. `delete` false (count_duplicate_games)
/// se contente de compter ; `delete` true (deduplicate_games) efface chaque
/// doublon des qu'il est identifie, par lots d'une page (meme resilience que
/// db::strip_check_suffixes : un arret en cours de route ne perd que la page
/// en cours, pas tout le travail deja commite).
///
/// En Rust plutot qu'un GROUP BY SQL sur ces cinq colonnes (non couvertes par
/// un index) : meme raison, deja constatee sur cette base, que
/// rebuild_player_stats -- un GROUP BY sur des valeurs non indexees necessite
/// un tri/hachage complet cote SQLite, reste bloque plusieurs minutes sur les
/// ~10-12M parties de la base de reference de l'utilisateur.
fn scan_duplicates(conn: &mut Connection, delete: bool, mut on_progress: impl FnMut(u64, u64)) -> Result<u32, String> {
    const PAGE_SIZE: i64 = 20_000;
    let mut seen: std::collections::HashSet<DedupKey> = std::collections::HashSet::new();
    let mut examined = 0u64;
    let mut duplicates = 0u32;
    let mut last_id = 0i64;

    loop {
        let page: Vec<(i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> = {
            let mut stmt = conn
                .prepare("SELECT id, white, black, date_played, result, mainline_sans, start_fen FROM games WHERE id > ?1 ORDER BY id LIMIT ?2")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![last_id, PAGE_SIZE], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let mut to_delete: Vec<(i64, String, Option<String>)> = Vec::new();
        for (id, white, black, date, result, mainline, start_fen) in &page {
            examined += 1;
            let key = dedup_key(white, black, date, result, mainline);
            if !seen.insert(key) {
                duplicates += 1;
                if delete {
                    to_delete.push((*id, mainline.clone().unwrap_or_default(), start_fen.clone()));
                }
            }
        }

        if delete && !to_delete.is_empty() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            {
                let mut del = tx.prepare("DELETE FROM games WHERE id = ?1").map_err(|e| e.to_string())?;
                for (id, mainline, start_fen) in &to_delete {
                    del.execute(params![id]).map_err(|e| e.to_string())?;
                    crate::explorer::deindex_game(&tx, *id, mainline, start_fen.as_deref())?;
                }
            }
            tx.commit().map_err(|e| e.to_string())?;
        }

        on_progress(examined, duplicates as u64);
    }

    Ok(duplicates)
}

/// Compte les parties qui seraient supprimees par deduplicate_games, sans
/// rien modifier -- sert a afficher un nombre avant confirmation (menu
/// Outils > Dedoublonner la base).
pub fn count_duplicate_games(conn: &mut Connection, on_progress: impl FnMut(u64, u64)) -> Result<u32, String> {
    scan_duplicates(conn, false, on_progress)
}

/// Supprime les parties strictement dupliquees (meme regroupement que
/// count_duplicate_games), en gardant dans chaque groupe la partie au plus
/// petit id (la plus ancienne). Retire aussi les lignes de `positions` de
/// chaque partie supprimee ; player_stats, lui, est laisse a l'appelant
/// (relancer rebuild_player_stats juste apres, voir lib.rs). Renvoie le nombre
/// supprime.
pub fn deduplicate_games(conn: &mut Connection, on_progress: impl FnMut(u64, u64)) -> Result<u32, String> {
    scan_duplicates(conn, true, on_progress)
}

/// Renseigne retroactivement white_fide_id/black_fide_id (voir
/// migrate_add_fide_id_columns) sur les parties qui n'en ont pas encore --
/// necessaire pour les parties importees avant l'ajout de cette resolution,
/// ou dont l'import a eu lieu sans base FIDE encore telechargee (voir
/// lib.rs::bulk_import_pgn, fide_conn absent). Reecrit aussi le nom vers sa
/// forme canonique, exactement comme le ferait un import en masse avec la
/// base FIDE deja presente (voir fide::canonicalize) -- meme regle : sans
/// candidat unique et non ambigu (au besoin departage par l'Elo enregistre
/// sur la partie), le nom est laisse tel quel et aucun id n'est pose. Renvoie
/// le nombre de parties ou au moins un id (Blancs ou Noirs) a pu etre resolu.
pub fn backfill_fide_ids(games_conn: &mut Connection, fide_conn: &Connection, mut on_progress: impl FnMut(u64, u64)) -> Result<u32, String> {
    const PROGRESS_EVERY: u64 = 5_000;
    struct Row {
        id: i64,
        white: Option<String>,
        black: Option<String>,
        white_elo: Option<i64>,
        black_elo: Option<i64>,
        white_fide_id: Option<String>,
        black_fide_id: Option<String>,
    }

    // Premier passage : charge les parties concernees (sur une base jamais
    // encore resolue, potentiellement la quasi-totalite des ~10-12M
    // parties de la base de reference de l'utilisateur) -- signale une
    // progression ici aussi (pas seulement pendant la mise a jour plus bas,
    // voir la boucle suivante) : query_map pioche une ligne a la fois dans
    // le curseur SQLite sous-jacent (une seule requete), donc un simple
    // compteur suffit, comme pour rebuild_position_index/rebuild_player_stats.
    // Sans ca, ce chargement -- potentiellement plusieurs dizaines de
    // secondes a lui seul sur la base entiere -- ne donnait aucun retour
    // visuel avant que la boucle de mise a jour ne demarre.
    let rows: Vec<Row> = {
        let mut stmt = games_conn
            .prepare(
                "SELECT id, white, black, white_elo, black_elo, white_fide_id, black_fide_id FROM games
                 WHERE white_fide_id IS NULL OR black_fide_id IS NULL",
            )
            .map_err(|e| e.to_string())?;
        let mapped_rows = stmt.query_map([], |row| {
            Ok(Row {
                id: row.get(0)?,
                white: row.get(1)?,
                black: row.get(2)?,
                white_elo: row.get(3)?,
                black_elo: row.get(4)?,
                white_fide_id: row.get(5)?,
                black_fide_id: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

        let mut collected = Vec::new();
        let mut loaded = 0u64;
        for row in mapped_rows.filter_map(|r| r.ok()) {
            collected.push(row);
            loaded += 1;
            if loaded % PROGRESS_EVERY == 0 {
                on_progress(loaded, 0);
            }
        }
        collected
    };

    // Ne tente la resolution FIDE que pour la colonne qui n'a pas deja un
    // id (l'autre camp de la meme partie peut deja en avoir un, si un
    // premier passage avait resolu Blancs mais pas Noirs).
    fn resolve(fide_conn: &Connection, name: &Option<String>, elo: Option<i64>, existing_id: &Option<String>) -> (Option<String>, Option<String>) {
        if existing_id.is_some() {
            return (name.clone(), existing_id.clone());
        }
        match name.as_deref() {
            Some(n) if !n.is_empty() && n != "?" => match crate::fide::canonicalize(fide_conn, n, elo.map(|v| v as i32)) {
                Some((canonical, fide_id)) => (Some(canonical), Some(fide_id)),
                None => (name.clone(), None),
            },
            _ => (name.clone(), None),
        }
    }

    let mut examined = 0u64;
    let mut resolved = 0u32;
    let tx = games_conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut update = tx
            .prepare("UPDATE games SET white = ?1, white_fide_id = ?2, black = ?3, black_fide_id = ?4 WHERE id = ?5")
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (new_white, new_white_id) = resolve(fide_conn, &row.white, row.white_elo, &row.white_fide_id);
            let (new_black, new_black_id) = resolve(fide_conn, &row.black, row.black_elo, &row.black_fide_id);
            if (new_white_id.is_some() && row.white_fide_id.is_none()) || (new_black_id.is_some() && row.black_fide_id.is_none()) {
                resolved += 1;
            }
            update
                .execute(params![new_white, new_white_id, new_black, new_black_id, row.id])
                .map_err(|e| e.to_string())?;
            examined += 1;
            if examined % PROGRESS_EVERY == 0 {
                on_progress(examined, resolved as u64);
            }
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    on_progress(examined, resolved as u64);
    Ok(resolved)
}

/// Nombre de joueurs distincts (Blancs ou Noirs) correspondant au prefixe de
/// recherche, ou de la base entiere si `search` est vide. Lit player_stats
/// (precalcule) plutot que d'agreger games a la volee.
pub fn count_players(conn: &Connection, search: &str) -> Result<u32, String> {
    let count: i64 = if search.is_empty() {
        conn.query_row("SELECT COUNT(*) FROM player_stats", [], |row| row.get(0))
    } else {
        let (lo, hi) = prefix_bounds(search);
        conn.query_row(
            "SELECT COUNT(*) FROM player_stats WHERE name >= ?1 AND name < ?2",
            params![lo, hi],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

fn map_player_row(row: &rusqlite::Row) -> rusqlite::Result<PlayerSummary> {
    Ok(PlayerSummary { name: row.get(0)?, games: row.get::<_, i64>(1)? as u32 })
}

/// player_stats reste petit (une ligne par joueur distinct, pas par partie)
/// meme sur une base de plusieurs millions de parties : contrairement au
/// decompte par nom (voir count_players), trier par nombre de parties sans
/// index dedie reste largement assez rapide ici.
fn players_order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("name", false) => "ORDER BY name ASC",
        ("name", true) => "ORDER BY name DESC",
        ("games", false) => "ORDER BY games ASC, name ASC",
        _ => "ORDER BY games DESC, name ASC",
    }
}

/// Page de joueurs (nom + nombre de parties, Blancs+Noirs confondus), triee
/// selon `sort_by`/`sort_dir` (par defaut : les plus actifs d'abord).
pub fn list_players_page(
    conn: &Connection,
    search: &str,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<PlayerSummary>, String> {
    let order_by = players_order_by_clause(sort_by, sort_dir);
    if search.is_empty() {
        let sql = format!("SELECT name, games FROM player_stats {order_by} LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit, offset], map_player_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    } else {
        let (lo, hi) = prefix_bounds(search);
        let sql = format!("SELECT name, games FROM player_stats WHERE name >= ?1 AND name < ?2 {order_by} LIMIT ?3 OFFSET ?4");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![lo, hi, limit, offset], map_player_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
}

/// Cherche une partie deja presente dans la base avec les memes Blancs/Noirs/Date/Resultat
/// et la meme suite de coups (SAN) que celle donnee. Utilise pour alerter l'utilisateur
/// sans jamais bloquer la sauvegarde (voir save_game_to cote lib.rs).
/// Cherche une partie deja presente avec les memes Blancs/Noirs/Date/Resultat
/// et exactement la meme suite de coups. Compare directement la colonne
/// `mainline_sans` mise en cache a la sauvegarde plutot que de reparser (et
/// revalider coup par coup) chaque partie candidate : indispensable pour
/// rester rapide sur une base de plusieurs centaines de milliers de parties.
/// Les parties enregistrees avant l'ajout de cette colonne (mainline_sans
/// NULL) ne peuvent pas etre detectees comme doublons ; c'est un compromis
/// acceptable plutot que de re-parser toute une grosse base a chaque appel.
pub fn find_duplicate(
    conn: &Connection,
    tags: &BTreeMap<String, String>,
    moves: &[String],
) -> Result<Option<GameSummary>, String> {
    let white = tags.get("White").cloned().unwrap_or_default();
    let black = tags.get("Black").cloned().unwrap_or_default();
    let date = tags.get("Date").cloned().unwrap_or_default();
    let result = tags.get("Result").cloned().unwrap_or_default();
    let moves_str = moves.join(" ");

    conn.query_row(
        "SELECT id, white, black, date_played, result, event FROM games
         WHERE white = ?1 AND black = ?2 AND date_played = ?3 AND result = ?4 AND mainline_sans = ?5
         LIMIT 1",
        params![white, black, date, result, moves_str],
        |row| {
            Ok(GameSummary {
                id: row.get(0)?,
                white: or_default(row.get(1)?, "?"),
                black: or_default(row.get(2)?, "?"),
                date: or_default(row.get(3)?, "????.??.??"),
                result: or_default(row.get(4)?, "*"),
                event: or_default(row.get(5)?, "?"),
                time_control: String::new(),
                white_elo: None,
                black_elo: None,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn load_game_pgn(conn: &Connection, id: i64) -> Result<Option<String>, String> {
    conn.query_row("SELECT pgn_moves FROM games WHERE id = ?1", params![id], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(|e| e.to_string())
}

/// Recharge une partie sous la forme attendue par save_game (tags, pgn,
/// mainline_sans) : les colonnes structurees (white, black, ...) sont deja
/// la version fiable des tags PGN de cette partie, inutile de reparser le
/// texte pour les retrouver. Utilise pour deplacer une partie vers une autre
/// base (voir move_game cote lib.rs).
pub fn load_game_for_move(conn: &Connection, id: i64) -> Result<Option<(BTreeMap<String, String>, String, String)>, String> {
    conn.query_row(
        "SELECT white, black, white_elo, black_elo, result, event, site, round, date_played, time_control, pgn_moves, mainline_sans
         FROM games WHERE id = ?1",
        params![id],
        |row| {
            let mut tags = BTreeMap::new();
            let mut set = |key: &str, value: Option<String>| {
                if let Some(v) = value {
                    tags.insert(key.to_string(), v);
                }
            };
            set("White", row.get::<_, Option<String>>(0)?);
            set("Black", row.get::<_, Option<String>>(1)?);
            set("WhiteElo", row.get::<_, Option<i64>>(2)?.map(|v| v.to_string()));
            set("BlackElo", row.get::<_, Option<i64>>(3)?.map(|v| v.to_string()));
            set("Result", row.get::<_, Option<String>>(4)?);
            set("Event", row.get::<_, Option<String>>(5)?);
            set("Site", row.get::<_, Option<String>>(6)?);
            set("Round", row.get::<_, Option<String>>(7)?);
            set("Date", row.get::<_, Option<String>>(8)?);
            set("TimeControl", row.get::<_, Option<String>>(9)?);
            let pgn: String = row.get(10)?;
            let mainline_sans: Option<String> = row.get(11)?;
            Ok((tags, pgn, mainline_sans.unwrap_or_default()))
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_for_strip_check_suffixes_tests() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
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
            ) WITHOUT ROWID;
            CREATE TABLE db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn strip_check_suffixes_cleans_mainline_sans_and_rebuilds_position_index() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        // simule une partie importee avant le correctif : mainline_sans garde
        // le "+"/"#" du PGN source (voir l'ancien bulk_import::BulkGameVisitor::san).
        conn.execute(
            "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
             VALUES ('Alice', 'Bob', '1-0', '1. e4 e5 2. Qh5 Nc6 3. Qxf7# 1-0', 'e4 e5 Qh5 Nc6 Qxf7#', 0, 0)",
            [],
        )
        .unwrap();

        let mut progress_calls = Vec::new();
        let cleaned = strip_check_suffixes(&mut conn, |n| progress_calls.push(n), || false).unwrap();
        assert_eq!(cleaned, 1);
        assert_eq!(progress_calls, vec![1]);
        // le nettoyage ne touche pas a `positions` (deja sans suffixe a
        // l'indexation) : cette partie inseree brute n'y figure pas encore
        crate::explorer::rebuild_position_index(&mut conn, |_| {}, || false).unwrap();

        let mainline: String = conn.query_row("SELECT mainline_sans FROM games WHERE id = 1", [], |row| row.get(0)).unwrap();
        assert_eq!(mainline, "e4 e5 Qh5 Nc6 Qxf7");

        // position_index reconstruit a partir du mainline_sans nettoye :
        // le prefixe "e4 e5 Qh5 Nc6" (sans suffixe) doit maintenant matcher.
        // Le "+" est recalcule par explore() (voir annotate_check_suffixes),
        // pas copie du "#" (fictif) du PGN de depart -- Qxf7 met le roi noir
        // en echec ici, sans etre mat.
        let stats = crate::explorer::explore(
            &conn,
            &["e4".to_string(), "e5".to_string(), "Qh5".to_string(), "Nc6".to_string()],
            &crate::explorer::ExplorerFilters::default(),
            None,
        )
        .unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qxf7+");

        // idempotent : un second appel ne doit rien casser (deja marque fait),
        // et ne redeclenche aucun travail (progress jamais appele).
        assert!(already_check_suffixes_cleaned(&conn).unwrap());
        let mut second_progress_calls = Vec::new();
        let cleaned_again = strip_check_suffixes(&mut conn, |n| second_progress_calls.push(n), || false).unwrap();
        assert_eq!(cleaned_again, 0);
        assert!(second_progress_calls.is_empty());
    }

    #[test]
    fn extract_fen_tag_reads_the_value_and_stops_before_the_movetext() {
        let pgn = "[White \"Alice\"]\n[FEN \"8/8/8/8/8/8/8/K6k w - - 0 1\"]\n[Result \"*\"]\n\n1. Kb2 *";
        assert_eq!(extract_fen_tag(pgn).as_deref(), Some("8/8/8/8/8/8/8/K6k w - - 0 1"));
        assert_eq!(extract_fen_tag("[White \"Alice\"]\n[Result \"*\"]\n\n1. e4 *"), None);
    }

    // Cas rapporte par l'utilisateur : une partie a depart personnalise
    // (Nouvelle position, Chess960) deja enregistree AVANT l'ajout de la
    // colonne start_fen restait introuvable par l'explorateur (ALTER TABLE
    // ADD COLUMN ne backfille jamais les lignes existantes) -- simule ici en
    // inserant directement une ligne avec un tag FEN dans pgn_moves mais
    // start_fen encore a NULL, exactement l'etat d'une base ouverte pour la
    // premiere fois apres cette mise a jour.
    #[test]
    fn backfill_start_fen_recovers_custom_positions_saved_before_the_column_existed() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        conn.execute(
            "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, start_fen, created_at, last_modified)
             VALUES ('Carol', 'Dave', '1-0',
             '[White \"Carol\"]\n[Black \"Dave\"]\n[Result \"1-0\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 5\"]\n\n1. e4 e5 1-0',
             'e4 e5', NULL, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, start_fen, created_at, last_modified)
             VALUES ('Alice', 'Bob', '1-0', '[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0', 'e4 e5', NULL, 0, 0)",
            [],
        )
        .unwrap();

        let mut progress_calls = Vec::new();
        let updated = backfill_start_fen(&mut conn, |n| progress_calls.push(n), || false).unwrap();
        assert_eq!(updated, 1);
        assert_eq!(progress_calls, vec![2]);

        let carol_start_fen: Option<String> =
            conn.query_row("SELECT start_fen FROM games WHERE white = 'Carol'", [], |row| row.get(0)).unwrap();
        assert_eq!(carol_start_fen.as_deref(), Some("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 5"));

        // partie standard : reste a NULL (voir normalize_start_fen), pas de
        // faux positif introduit par le rattrapage.
        let alice_start_fen: Option<String> =
            conn.query_row("SELECT start_fen FROM games WHERE white = 'Alice'", [], |row| row.get(0)).unwrap();
        assert_eq!(alice_start_fen, None);

        // idempotent : deja marque termine, un second appel ne refait rien.
        assert!(start_fen_backfilled(&conn).unwrap());
        let updated_again = backfill_start_fen(&mut conn, |_| {}, || false).unwrap();
        assert_eq!(updated_again, 0);
    }

    #[test]
    fn strip_check_suffixes_pages_across_multiple_batches() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        // plus de lignes que STRIP_CHECK_SUFFIXES_PAGE_SIZE ne le permettrait
        // en une seule page, pour verifier que la pagination par id couvre
        // bien toute la table (pas seulement la premiere page).
        for i in 0..(STRIP_CHECK_SUFFIXES_PAGE_SIZE * 2 + 7) {
            conn.execute(
                "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
                 VALUES ('Alice', 'Bob', '1-0', '1. e4+ 1-0', 'e4+', 0, 0)",
                [],
            )
            .unwrap();
            let _ = i;
        }

        let cleaned = strip_check_suffixes(&mut conn, |_| {}, || false).unwrap();
        assert_eq!(cleaned as i64, STRIP_CHECK_SUFFIXES_PAGE_SIZE * 2 + 7);

        let remaining_dirty: i64 = conn
            .query_row("SELECT COUNT(*) FROM games WHERE mainline_sans LIKE '%+%' OR mainline_sans LIKE '%#%'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_dirty, 0);
    }

    #[test]
    fn strip_check_suffixes_stops_at_the_next_check_when_cancelled() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        for _ in 0..(STRIP_CHECK_SUFFIXES_PAGE_SIZE + 5) {
            conn.execute(
                "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
                 VALUES ('Alice', 'Bob', '1-0', '1. e4+ 1-0', 'e4+', 0, 0)",
                [],
            )
            .unwrap();
        }

        // annule des le premier controle : rien ne doit etre nettoye, et le
        // travail (deja) commit d'une page precedente resterait acquis s'il y
        // en avait eu une -- ici, aucune.
        let cleaned = strip_check_suffixes(&mut conn, |_| {}, || true).unwrap();
        assert_eq!(cleaned, 0);
        assert!(!already_check_suffixes_cleaned(&conn).unwrap());
    }

    #[test]
    fn migrates_old_schema_without_elo_columns() {
        let conn = Connection::open_in_memory().unwrap();
        // schema tel qu'il existait avant l'ajout des colonnes Elo
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, result TEXT, event TEXT, site TEXT,
                round TEXT, date_played TEXT, eco TEXT, opening_name TEXT,
                ply_count INTEGER, time_control TEXT, pgn_moves TEXT NOT NULL,
                extra_tags TEXT, created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();

        migrate_add_columns(&conn).unwrap();

        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        let id = save_game(&conn, "1. e4 *", &tags, "e4").unwrap();
        assert!(id > 0);
    }

    fn fresh_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    fn base_tags(white: &str, black: &str, date: &str, result: &str) -> BTreeMap<String, String> {
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), white.to_string());
        tags.insert("Black".to_string(), black.to_string());
        tags.insert("Date".to_string(), date.to_string());
        tags.insert("Result".to_string(), result.to_string());
        tags
    }

    #[test]
    fn find_duplicate_matches_same_moves_only() {
        let conn = fresh_conn();
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

        let tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        save_game(&conn, "1. e4 e5 1-0", &tags, "e4 e5").unwrap();

        // memes joueurs/date/resultat mais coups differents : pas un doublon
        let different_moves = vec!["d4".to_string(), "d5".to_string()];
        assert!(find_duplicate(&conn, &tags, &different_moves).unwrap().is_none());

        // coups identiques : doublon detecte
        let same_moves = vec!["e4".to_string(), "e5".to_string()];
        assert!(find_duplicate(&conn, &tags, &same_moves).unwrap().is_some());
    }

    fn no_filters() -> crate::explorer::ExplorerFilters {
        crate::explorer::ExplorerFilters::default()
    }

    // Hash Zobrist de la position atteinte apres `moves` depuis le depart
    // standard -- les tests de games_at_zobrist/count_at_zobrist comparent
    // toujours des POSITIONS (voir explorer::current_zobrist), plus du texte
    // de coups.
    fn zobrist_after(moves: &[&str]) -> i64 {
        let moves: Vec<String> = moves.iter().map(|s| s.to_string()).collect();
        crate::explorer::current_zobrist(&moves, None).unwrap()
    }

    #[test]
    fn games_at_zobrist_paginates_correctly() {
        let conn = open(Path::new(":memory:")).unwrap();
        for i in 0..5 {
            save_game(&conn, "1. e4 *", &base_tags(&format!("W{i}"), "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        }
        // ne doit jamais apparaitre : autre premier coup
        save_game(&conn, "1. d4 *", &base_tags("Erin", "Frank", "2024.01.01", "*"), "d4 d5").unwrap();

        let zobrist = zobrist_after(&["e4"]);
        assert_eq!(count_at_zobrist(&conn, zobrist, &no_filters()).unwrap(), 5);

        // les plus recentes d'abord (id decroissant) : W4 puis W3
        let page0 = games_at_zobrist(&conn, zobrist, &no_filters(), "", "", 2, 0).unwrap();
        assert_eq!(page0.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W4", "W3"]);

        // page suivante (offset 2) : W2 puis W1, "d4" jamais inclus
        let page1 = games_at_zobrist(&conn, zobrist, &no_filters(), "", "", 2, 2).unwrap();
        assert_eq!(page1.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W2", "W1"]);

        // derniere page partielle (offset 4, limit 2) : un seul resultat
        let page2 = games_at_zobrist(&conn, zobrist, &no_filters(), "", "", 2, 4).unwrap();
        assert_eq!(page2.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W0"]);
    }

    #[test]
    fn games_at_zobrist_matches_exact_and_continuations_only() {
        let conn = open(Path::new(":memory:")).unwrap();

        // s'arrete pile a "e4 e5" : doit apparaitre pour la position apres "e4 e5"
        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        // continue au-dela de "e4 e5" : doit aussi apparaitre (la partie EST passee par cette position)
        save_game(&conn, "1. e4 e5 2. Nf3 *", &base_tags("Carol", "Dave", "2024.01.02", "*"), "e4 e5 Nf3").unwrap();
        // autre position : ne doit pas apparaitre
        save_game(&conn, "1. d4 d5 *", &base_tags("Erin", "Frank", "2024.01.03", "*"), "d4 d5").unwrap();

        let zobrist = zobrist_after(&["e4", "e5"]);
        assert_eq!(count_at_zobrist(&conn, zobrist, &no_filters()).unwrap(), 2);
        let games = games_at_zobrist(&conn, zobrist, &no_filters(), "", "", 10, 0).unwrap();
        let whites: Vec<&str> = games.iter().map(|g| g.white.as_str()).collect();
        assert_eq!(whites.len(), 2);
        assert!(whites.contains(&"Alice"));
        assert!(whites.contains(&"Carol"));

        // position de depart (aucun coup joue) : toutes les parties
        let start_zobrist = zobrist_after(&[]);
        assert_eq!(count_at_zobrist(&conn, start_zobrist, &no_filters()).unwrap(), 3);
    }

    // Coeur de la refonte Zobrist : l'utilisateur veut retrouver TOUTES les
    // parties qui ont atteint une position donnee, y compris par
    // TRANSPOSITION (un ordre de coups different) -- 1.d4 Nf6 2.c4 et 1.c4
    // Nf6 2.d4 aboutissent au meme plateau, et doivent toutes deux apparaitre
    // pour cette position, alors que leurs mainline_sans respectifs
    // ("d4 Nf6 c4" vs "c4 Nf6 d4") ne partagent aucun prefixe de texte commun.
    #[test]
    fn games_at_zobrist_finds_transposed_games() {
        let conn = open(Path::new(":memory:")).unwrap();
        save_game(&conn, "1. d4 Nf6 2. c4 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "d4 Nf6 c4").unwrap();
        save_game(&conn, "1. c4 Nf6 2. d4 *", &base_tags("Carol", "Dave", "2024.01.02", "*"), "c4 Nf6 d4").unwrap();
        // ne doit jamais apparaitre : n'atteint jamais cette position
        save_game(&conn, "1. e4 e5 *", &base_tags("Erin", "Frank", "2024.01.03", "*"), "e4 e5").unwrap();

        let via_d4_first = zobrist_after(&["d4", "Nf6", "c4"]);
        let via_c4_first = zobrist_after(&["c4", "Nf6", "d4"]);
        assert_eq!(via_d4_first, via_c4_first, "les deux ordres de coups doivent aboutir au meme hash Zobrist");

        assert_eq!(count_at_zobrist(&conn, via_d4_first, &no_filters()).unwrap(), 2);
        let games = games_at_zobrist(&conn, via_d4_first, &no_filters(), "", "", 10, 0).unwrap();
        let whites: Vec<&str> = games.iter().map(|g| g.white.as_str()).collect();
        assert_eq!(whites.len(), 2);
        assert!(whites.contains(&"Alice"));
        assert!(whites.contains(&"Carol"));
    }

    #[test]
    fn normalize_start_fen_collapses_absent_or_standard_fen_to_none() {
        assert_eq!(normalize_start_fen(None), None);
        assert_eq!(normalize_start_fen(Some("")), None);
        assert_eq!(normalize_start_fen(Some(STANDARD_START_FEN)), None);
        assert_eq!(
            normalize_start_fen(Some("nrbbknqr/pppppppp/8/8/8/8/PPPPPPPP/NRBBKNQR w HBhb - 0 0")),
            Some("nrbbknqr/pppppppp/8/8/8/8/PPPPPPPP/NRBBKNQR w HBhb - 0 0".to_string())
        );
    }

    #[test]
    fn games_at_zobrist_sorts_by_the_requested_column() {
        let conn = open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        tags.insert("WhiteElo".to_string(), "2200".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        let mut tags = base_tags("Carol", "Dave", "2024.01.02", "0-1");
        tags.insert("WhiteElo".to_string(), "2600".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        let mut tags = base_tags("Erin", "Frank", "2024.01.03", "1/2-1/2");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();

        let zobrist = zobrist_after(&["e4", "e5"]);
        let desc = games_at_zobrist(&conn, zobrist, &no_filters(), "whiteElo", "desc", 10, 0).unwrap();
        assert_eq!(desc.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["Carol", "Erin", "Alice"]);

        let asc = games_at_zobrist(&conn, zobrist, &no_filters(), "whiteElo", "asc", 10, 0).unwrap();
        assert_eq!(asc.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["Alice", "Erin", "Carol"]);
    }

    #[test]
    fn games_at_zobrist_respects_player_filter() {
        let conn = open(Path::new(":memory:")).unwrap();

        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Carol", "Alice", "2024.01.02", "*"), "e4 e5").unwrap();

        let zobrist = zobrist_after(&["e4", "e5"]);

        // Alice en Blancs : seulement la premiere partie
        let white_filters =
            crate::explorer::ExplorerFilters { player: Some(crate::explorer::PlayerFilter { name: "Alice".to_string(), white: true, exact: true }), ..Default::default() };
        assert_eq!(count_at_zobrist(&conn, zobrist, &white_filters).unwrap(), 1);
        let games = games_at_zobrist(&conn, zobrist, &white_filters, "", "", 10, 0).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].white, "Alice");

        // Alice en Noirs : seulement la seconde
        let black_filters =
            crate::explorer::ExplorerFilters { player: Some(crate::explorer::PlayerFilter { name: "Alice".to_string(), white: false, exact: true }), ..Default::default() };
        assert_eq!(count_at_zobrist(&conn, zobrist, &black_filters).unwrap(), 1);
        let games = games_at_zobrist(&conn, zobrist, &black_filters, "", "", 10, 0).unwrap();
        assert_eq!(games[0].black, "Alice");
    }

    #[test]
    fn games_at_zobrist_respects_rating_range_filter() {
        let conn = open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "2450".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        // un seul camp dans l'intervalle : exclue
        let mut tags = base_tags("Carol", "Dave", "2024.01.02", "0-1");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "1500".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();

        let zobrist = zobrist_after(&["e4", "e5"]);
        let filters = crate::explorer::ExplorerFilters { rating_range: Some((2300, 2500)), ..Default::default() };
        assert_eq!(count_at_zobrist(&conn, zobrist, &filters).unwrap(), 1);
        let games = games_at_zobrist(&conn, zobrist, &filters, "", "", 10, 0).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].white, "Alice");
    }

    #[test]
    fn games_at_zobrist_respects_year_range_filter() {
        let conn = open(Path::new(":memory:")).unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2015.06.01", "1-0"), "e4 e5").unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Carol", "Dave", "1990.01.01", "0-1"), "e4 e5").unwrap();

        let zobrist = zobrist_after(&["e4", "e5"]);
        let filters = crate::explorer::ExplorerFilters { year_range: Some((2010, 2020)), ..Default::default() };
        assert_eq!(count_at_zobrist(&conn, zobrist, &filters).unwrap(), 1);
        let games = games_at_zobrist(&conn, zobrist, &filters, "", "", 10, 0).unwrap();
        assert_eq!(games[0].white, "Alice");
    }

    #[test]
    fn list_players_page_aggregates_white_and_black_across_games() {
        let conn = open(Path::new(":memory:")).unwrap();

        save_game(&conn, "1. e4 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4").unwrap();
        save_game(&conn, "1. e4 *", &base_tags("Bob", "Alice", "2024.01.02", "*"), "e4").unwrap();
        save_game(&conn, "1. e4 *", &base_tags("Carol", "Alice", "2024.01.03", "*"), "e4").unwrap();

        assert_eq!(count_players(&conn, "").unwrap(), 3);
        let players = list_players_page(&conn, "", "", "", 10, 0).unwrap();
        // Alice apparait dans les 3 parties (2x Blancs+Noirs melanges) : la plus active, en tete
        assert_eq!(players[0].name, "Alice");
        assert_eq!(players[0].games, 3);
        let bob = players.iter().find(|p| p.name == "Bob").unwrap();
        assert_eq!(bob.games, 2);

        // recherche par prefixe : ne garde que Carol
        assert_eq!(count_players(&conn, "Car").unwrap(), 1);
        let filtered = list_players_page(&conn, "Car", "", "", 10, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Carol");

        // tri alphabetique explicite
        let alpha = list_players_page(&conn, "", "name", "asc", 10, 0).unwrap();
        assert_eq!(alpha.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob", "Carol"]);
        let alpha_desc = list_players_page(&conn, "", "name", "desc", 10, 0).unwrap();
        assert_eq!(alpha_desc.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Carol", "Bob", "Alice"]);
    }

    #[test]
    fn rebuild_player_stats_merges_names_case_insensitively() {
        // insertion SQL directe (pas save_game) : simule une base existante
        // ouverte pour la premiere fois apres l'ajout de player_stats, dont
        // le seul peuplement possible est le rebuild complet fait par
        // migrate_player_stats.
        let mut conn = open(Path::new(":memory:")).unwrap();
        conn.execute("DELETE FROM player_stats", []).unwrap();
        conn.execute_batch(
            "INSERT INTO games (white, black, pgn_moves, mainline_sans, created_at, last_modified)
             VALUES ('Kasparov,G', 'Karpov,A', '1. e4 *', 'e4', 0, 0),
                    ('kasparov,g', 'Short,N', '1. e4 *', 'e4', 0, 0);",
        )
        .unwrap();

        rebuild_player_stats(&mut conn, |_| {}).unwrap();

        // Kasparov (fusionne), Karpov, Short : 3 joueurs distincts
        assert_eq!(count_players(&conn, "").unwrap(), 3);
        let players = list_players_page(&conn, "", "", "", 10, 0).unwrap();
        // "Kasparov,G" et "kasparov,g" fusionnent (COLLATE NOCASE) en un seul joueur, 2 parties
        let kasparov = players.iter().find(|p| p.name.eq_ignore_ascii_case("kasparov,g")).unwrap();
        assert_eq!(kasparov.games, 2);
    }

    fn position_index_count(conn: &Connection, zobrist: i64) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM positions WHERE zobrist = ?1", params![zobrist], |row| row.get(0)).unwrap()
    }

    #[test]
    fn position_index_follows_save_update_delete() {
        let conn = open(Path::new(":memory:")).unwrap();

        let tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        let id = save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        let start = zobrist_after(&[]);
        let after_e4 = zobrist_after(&["e4"]);
        let after_e4_e5 = zobrist_after(&["e4", "e5"]);
        let after_d4 = zobrist_after(&["d4"]);
        assert_eq!(position_index_count(&conn, start), 1);
        assert_eq!(position_index_count(&conn, after_e4), 1);
        assert_eq!(position_index_count(&conn, after_e4_e5), 1);

        // la partie change de premier coup : l'ancienne contribution doit
        // disparaitre, la nouvelle apparaitre
        update_game(&conn, id, "1. d4 d5 *", &tags, "d4 d5").unwrap();
        assert_eq!(position_index_count(&conn, after_e4), 0);
        assert_eq!(position_index_count(&conn, after_e4_e5), 0);
        assert_eq!(position_index_count(&conn, after_d4), 1);
        // la position de depart, elle, reste traversee par la partie modifiee
        assert_eq!(position_index_count(&conn, start), 1);

        delete_game(&conn, id).unwrap();
        assert_eq!(position_index_count(&conn, after_d4), 0);
        assert_eq!(position_index_count(&conn, start), 0);
    }

    #[test]
    fn count_and_deduplicate_games_remove_only_exact_repeats() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        let first_id = save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        // strict duplicate (memes 5 champs de regroupement) : doit disparaitre
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        // meme joueurs/date/resultat mais coups differents : PAS un duplicat
        save_game(&conn, "1. d4 d5 *", &tags, "d4 d5").unwrap();

        assert_eq!(count_duplicate_games(&mut conn, |_, _| {}).unwrap(), 1);
        assert_eq!(deduplicate_games(&mut conn, |_, _| {}).unwrap(), 1);
        assert_eq!(count_duplicate_games(&mut conn, |_, _| {}).unwrap(), 0);

        // la partie supprimee n'a laisse aucune ligne dans `positions`
        let orphans: i64 = conn.query_row("SELECT COUNT(*) FROM positions WHERE game_id NOT IN (SELECT id FROM games)", [], |r| r.get(0)).unwrap();
        assert_eq!(orphans, 0);

        // seule la partie strictement dupliquee a disparu, et c'est bien la
        // plus ancienne (plus petit id) qui a ete gardee
        let remaining: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM games ORDER BY id").unwrap();
            stmt.query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect()
        };
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0], first_id);
    }

    #[test]
    fn set_game_fide_ids_updates_only_the_targeted_row() {
        let conn = open(Path::new(":memory:")).unwrap();
        let id_a = save_game(&conn, "1. e4 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4").unwrap();
        let id_b = save_game(&conn, "1. e4 *", &base_tags("Carol", "Dave", "2024.01.02", "*"), "e4").unwrap();

        set_game_fide_ids(&conn, id_a, Some("123"), None).unwrap();

        let fetch = |id: i64| -> (Option<String>, Option<String>) {
            conn.query_row(
                "SELECT white_fide_id, black_fide_id FROM games WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(fetch(id_a), (Some("123".to_string()), None));
        assert_eq!(fetch(id_b), (None, None));
    }

    fn fide_conn_with_players(players: &[(&str, &str, Option<i32>)]) -> Connection {
        let conn = crate::fide::open_at(Path::new(":memory:")).unwrap();
        for (id, name, std_rating) in players {
            conn.execute(
                "INSERT INTO fide_players (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, 'FRA', 'M', '', '', '', ?3, NULL, NULL, NULL, '')",
                params![id, name, std_rating],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn backfill_fide_ids_resolves_unambiguous_names_and_is_idempotent() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let fide_conn = fide_conn_with_players(&[("100", "Giri, Anish", Some(2750)), ("101", "Carlsen, Magnus", Some(2830))]);

        // "Giri,A" et "Carlsen, M" resolvent tous deux sans ambiguite -> nom
        // reecrit + id pose pour les deux camps de la meme partie
        let resolvable = save_game(&conn, "1. e4 *", &base_tags("Giri,A", "Carlsen, M", "2024.01.01", "*"), "e4").unwrap();

        assert_eq!(backfill_fide_ids(&mut conn, &fide_conn, |_, _| {}).unwrap(), 1);

        let (white, white_id, black, black_id): (Option<String>, Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT white, white_fide_id, black, black_fide_id FROM games WHERE id = ?1",
                params![resolvable],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(white.as_deref(), Some("Giri, Anish"));
        assert_eq!(white_id, Some("100".to_string()));
        assert_eq!(black.as_deref(), Some("Carlsen, Magnus"));
        assert_eq!(black_id, Some("101".to_string()));

        // relancer le backfill ne doit rien re-resoudre (deja fait) : le
        // filtre WHERE white_fide_id IS NULL OR black_fide_id IS NULL exclut
        // deja cette partie
        assert_eq!(backfill_fide_ids(&mut conn, &fide_conn, |_, _| {}).unwrap(), 0);
    }

    #[test]
    fn backfill_fide_ids_leaves_no_id_when_still_ambiguous() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let fide_conn = fide_conn_with_players(&[("1", "Nguyen, Anh", None), ("2", "Nguyen, Anton", None)]);
        let id = save_game(&conn, "1. e4 *", &base_tags("Nguyen,A", "Bob", "2024.01.01", "*"), "e4").unwrap();

        assert_eq!(backfill_fide_ids(&mut conn, &fide_conn, |_, _| {}).unwrap(), 0);

        let (white, white_id): (Option<String>, Option<String>) = conn
            .query_row("SELECT white, white_fide_id FROM games WHERE id = ?1", params![id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        // ambigu : nom laisse tel quel, pas d'id pose
        assert_eq!(white.as_deref(), Some("Nguyen,A"));
        assert_eq!(white_id, None);
    }
}
