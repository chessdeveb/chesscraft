use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
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
            created_at INTEGER NOT NULL,
            last_modified INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_games_white ON games(white);
        CREATE INDEX IF NOT EXISTS idx_games_black ON games(black);
        CREATE INDEX IF NOT EXISTS idx_games_date ON games(date_played);",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn save_game(conn: &Connection, pgn: &str, tags: &BTreeMap<String, String>) -> Result<i64, String> {
    let get = |key: &str| tags.get(key).cloned();
    let timestamp = now();
    conn.execute(
        "INSERT INTO games (white, black, result, event, site, round, date_played, pgn_moves, created_at, last_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            get("White"),
            get("Black"),
            get("Result"),
            get("Event"),
            get("Site"),
            get("Round"),
            get("Date"),
            pgn,
            timestamp,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn load_latest_game(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT pgn_moves FROM games ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}
