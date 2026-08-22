use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
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
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
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
            created_at INTEGER NOT NULL,
            last_modified INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_games_white ON games(white);
        CREATE INDEX IF NOT EXISTS idx_games_black ON games(black);
        CREATE INDEX IF NOT EXISTS idx_games_date ON games(date_played);
        CREATE INDEX IF NOT EXISTS idx_games_dedup ON games(white, black, date_played, result, mainline_sans);",
    )
    .map_err(|e| e.to_string())?;
    migrate_add_columns(&conn)?;
    Ok(conn)
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

pub fn save_game(
    conn: &Connection,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<i64, String> {
    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let timestamp = now();
    conn.execute(
        "INSERT INTO games (white, black, white_elo, black_elo, result, event, site, round, date_played, pgn_moves, mainline_sans, created_at, last_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
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
            pgn,
            mainline_sans,
            timestamp,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update_game(
    conn: &Connection,
    id: i64,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<(), String> {
    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let timestamp = now();
    conn.execute(
        "UPDATE games SET white = ?1, black = ?2, white_elo = ?3, black_elo = ?4, result = ?5, event = ?6,
         site = ?7, round = ?8, date_played = ?9, pgn_moves = ?10, mainline_sans = ?11, last_modified = ?12 WHERE id = ?13",
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
            pgn,
            mainline_sans,
            timestamp,
            id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_game(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM games WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
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
    })
}

/// Nombre de parties correspondant a la recherche (Blancs/Noirs), ou de la
/// base entiere si `search` est vide.
pub fn count_games(conn: &Connection, search: &str) -> Result<u32, String> {
    let count: i64 = if search.is_empty() {
        conn.query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))
    } else {
        let pattern = format!("%{search}%");
        conn.query_row(
            "SELECT COUNT(*) FROM games WHERE white LIKE ?1 OR black LIKE ?1",
            params![pattern],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

/// Une page de parties (triees par id decroissant), filtrees par Blancs/Noirs
/// si `search` est non vide. Indispensable sur les grosses bases (des
/// centaines de milliers de parties) ou charger la table entiere en un coup
/// n'est pas envisageable.
pub fn list_games_page(conn: &Connection, search: &str, limit: u32, offset: u32) -> Result<Vec<GameSummary>, String> {
    if search.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT id, white, black, date_played, result, event FROM games
                 ORDER BY id DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| e.to_string())?;
        let result = stmt
            .query_map(params![limit, offset], map_game_summary_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        result
    } else {
        let pattern = format!("%{search}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, white, black, date_played, result, event FROM games
                 WHERE white LIKE ?1 OR black LIKE ?1
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| e.to_string())?;
        let result = stmt
            .query_map(params![pattern, limit, offset], map_game_summary_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        result
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

/// Coups (deja mis en cache a la sauvegarde) et resultat de chaque partie de
/// la base, pour l'explorateur d'ouvertures. Beaucoup plus leger et rapide
/// qu'un `SELECT pgn_moves` suivi d'une reparse + revalidation complete de
/// chaque partie. Les parties enregistrees avant l'ajout du cache
/// (mainline_sans NULL) sont ignorees.
pub fn all_games_mainline(conn: &Connection) -> Result<Vec<(String, Option<String>)>, String> {
    let mut stmt = conn
        .prepare("SELECT mainline_sans, result FROM games WHERE mainline_sans IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            );",
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
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );",
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
}
