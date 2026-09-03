use rusqlite::{params, Connection};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const DOWNLOAD_URL: &str = "https://ratings.fide.com/download/players_list.zip";
const ARCHIVE_ENTRY_NAME: &str = "players_list_foa.txt";

/// Positions (debut de colonne) du format a largeur fixe publie par la FIDE,
/// determinees depuis la ligne d'en-tete du fichier reel (chaque ligne fait
/// 162 caracteres + CRLF) :
/// ID Number | Name | Fed | Sex | Tit | WTit | OTit | FOA | SRtng | SGm | SK
/// | RRtng | RGm | Rk | BRtng | BGm | BK | B-day | Flag
/// Pas de format documente officiellement stable : si la FIDE change un jour
/// la mise en page, `parse_line` echouera silencieusement ligne par ligne
/// (voir la boucle dans `refresh`) plutot que de faire planter tout l'import.
const COL_STARTS: [usize; 19] = [0, 15, 76, 80, 84, 89, 94, 109, 113, 119, 123, 126, 132, 136, 139, 145, 149, 152, 158];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FidePlayer {
    pub fide_id: String,
    pub name: String,
    pub fed: String,
    pub sex: String,
    pub title: String,
    pub w_title: String,
    pub o_title: String,
    pub std_rating: Option<i32>,
    pub rapid_rating: Option<i32>,
    pub blitz_rating: Option<i32>,
    pub birth_year: Option<i32>,
    pub flag: String,
}

fn col(line: &str, i: usize) -> &str {
    let start = COL_STARTS[i];
    let end = COL_STARTS.get(i + 1).copied().unwrap_or(line.len());
    if start >= line.len() {
        return "";
    }
    line.get(start..end.min(line.len())).unwrap_or("").trim()
}

fn parse_rating(s: &str) -> Option<i32> {
    let n: i32 = s.parse().ok()?;
    if n == 0 {
        None
    } else {
        Some(n)
    }
}

/// Une ligne du fichier FIDE vers un FidePlayer, ou None si la ligne est
/// trop courte / mal formee (voir COL_STARTS) plutot que de paniquer.
fn parse_line(line: &str) -> Option<FidePlayer> {
    if line.len() < COL_STARTS[1] {
        return None;
    }
    let fide_id = col(line, 0).to_string();
    if fide_id.is_empty() {
        return None;
    }
    Some(FidePlayer {
        fide_id,
        name: col(line, 1).to_string(),
        fed: col(line, 2).to_string(),
        sex: col(line, 3).to_string(),
        title: col(line, 4).to_string(),
        w_title: col(line, 5).to_string(),
        o_title: col(line, 6).to_string(),
        std_rating: parse_rating(col(line, 8)),
        rapid_rating: parse_rating(col(line, 11)),
        blitz_rating: parse_rating(col(line, 14)),
        birth_year: col(line, 17).parse().ok(),
        flag: col(line, 18).to_string(),
    })
}

fn db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("fide_players.sqlite")
}

pub fn open(app_data_dir: &Path) -> Result<Connection, String> {
    open_at(&db_path(app_data_dir))
}

/// Separee de `open` pour rester testable avec le sentinel SQLite
/// `:memory:` tel quel (open() le concatenerait avec app_data_dir en un
/// chemin de fichier invalide, `:memory:\fide_players.sqlite`).
fn open_at(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    // meme raisonnement que db::open : WAL laisse la table se lire pendant
    // qu'un rafraichissement ecrit (~1,9M lignes, plusieurs secondes), et le
    // busy_timeout evite un "database is locked" immediat en cas de bref
    // chevauchement.
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS fide_players (
            fide_id TEXT PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE,
            fed TEXT NOT NULL,
            sex TEXT NOT NULL,
            title TEXT NOT NULL,
            w_title TEXT NOT NULL,
            o_title TEXT NOT NULL,
            std_rating INTEGER,
            rapid_rating INTEGER,
            blitz_rating INTEGER,
            birth_year INTEGER,
            flag TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_fide_players_name ON fide_players(name);
        CREATE TABLE IF NOT EXISTS fide_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

pub fn last_updated(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT value FROM fide_meta WHERE key = 'last_updated'", [], |row| row.get(0))
        .ok()
}

fn prefix_bounds(value: &str) -> (String, String) {
    (value.to_string(), format!("{value}\u{ffff}"))
}

pub fn count_fide_players(conn: &Connection, search: &str) -> Result<u32, String> {
    let count: i64 = if search.is_empty() {
        conn.query_row("SELECT COUNT(*) FROM fide_players", [], |row| row.get(0))
    } else {
        let (lo, hi) = prefix_bounds(search);
        conn.query_row(
            "SELECT COUNT(*) FROM fide_players WHERE name >= ?1 AND name < ?2",
            params![lo, hi],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

fn map_fide_player_row(row: &rusqlite::Row) -> rusqlite::Result<FidePlayer> {
    Ok(FidePlayer {
        fide_id: row.get(0)?,
        name: row.get(1)?,
        fed: row.get(2)?,
        sex: row.get(3)?,
        title: row.get(4)?,
        w_title: row.get(5)?,
        o_title: row.get(6)?,
        std_rating: row.get(7)?,
        rapid_rating: row.get(8)?,
        blitz_rating: row.get(9)?,
        birth_year: row.get(10)?,
        flag: row.get(11)?,
    })
}

fn order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir == "desc";
    match (sort_by, desc) {
        ("name", true) => "ORDER BY name DESC",
        ("name", false) => "ORDER BY name ASC",
        ("fed", true) => "ORDER BY fed DESC, name ASC",
        ("fed", false) => "ORDER BY fed ASC, name ASC",
        ("std", true) => "ORDER BY std_rating DESC, name ASC",
        ("std", false) => "ORDER BY std_rating ASC, name ASC",
        ("rapid", true) => "ORDER BY rapid_rating DESC, name ASC",
        ("rapid", false) => "ORDER BY rapid_rating ASC, name ASC",
        ("blitz", true) => "ORDER BY blitz_rating DESC, name ASC",
        ("blitz", false) => "ORDER BY blitz_rating ASC, name ASC",
        ("birthYear", true) => "ORDER BY birth_year DESC, name ASC",
        ("birthYear", false) => "ORDER BY birth_year ASC, name ASC",
        _ => "ORDER BY std_rating DESC, name ASC",
    }
}

const SELECT_COLUMNS: &str =
    "fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag";

pub fn list_fide_players_page(
    conn: &Connection,
    search: &str,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<FidePlayer>, String> {
    let order_by = order_by_clause(sort_by, sort_dir);
    if search.is_empty() {
        let sql = format!("SELECT {SELECT_COLUMNS} FROM fide_players {order_by} LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![limit, offset], map_fide_player_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    } else {
        let (lo, hi) = prefix_bounds(search);
        let sql = format!("SELECT {SELECT_COLUMNS} FROM fide_players WHERE name >= ?1 AND name < ?2 {order_by} LIMIT ?3 OFFSET ?4");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![lo, hi, limit, offset], map_fide_player_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
}

/// Telecharge la liste FIDE (combinee Standard+Rapide+Blitz), la decompresse
/// et remplace entierement fide_players. `on_progress` est appele tous les
/// 20000 joueurs traites (pas a chaque ligne -- ~1,9M lignes au total, l'appel
/// IPC vers le frontend a un cout non negligeable a cette frequence).
/// En flux (BufReader sur l'entree du zip), jamais tout le texte (~310 Mo) en
/// memoire d'un coup -- meme raisonnement que pour l'import PGN en masse (voir
/// bulk_import::bulk_import) : la taille de la source ne doit pas dependre de
/// la RAM disponible.
pub fn refresh(app_data_dir: &Path, mut on_progress: impl FnMut(u32)) -> Result<u32, String> {
    let resp = ureq::get(DOWNLOAD_URL)
        .call()
        .map_err(|e| format!("Téléchargement de la liste FIDE échoué : {e}"))?;
    let mut zip_bytes = Vec::new();
    resp.into_reader().read_to_end(&mut zip_bytes).map_err(|e| e.to_string())?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;
    let entry = archive
        .by_name(ARCHIVE_ENTRY_NAME)
        .map_err(|e| format!("Archive FIDE inattendue (entrée {ARCHIVE_ENTRY_NAME} introuvable) : {e}"))?;
    let reader = BufReader::new(entry);

    let mut conn = open(app_data_dir)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM fide_players", []).map_err(|e| e.to_string())?;
    let mut count = 0u32;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO fide_players
                 (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(|e| e.to_string())?;
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| e.to_string())?;
            if i == 0 {
                continue; // en-tete
            }
            let Some(p) = parse_line(&line) else { continue };
            stmt.execute(params![
                p.fide_id,
                p.name,
                p.fed,
                p.sex,
                p.title,
                p.w_title,
                p.o_title,
                p.std_rating,
                p.rapid_rating,
                p.blitz_rating,
                p.birth_year,
                p.flag,
            ])
            .map_err(|e| e.to_string())?;
            count += 1;
            if count % 20000 == 0 {
                on_progress(count);
            }
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    tx.execute(
        "INSERT INTO fide_meta (key, value) VALUES ('last_updated', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![now.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    on_progress(count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_line() -> String {
        // reconstruit une ligne au bon alignement a partir des offsets reels,
        // avec les valeurs verifiees sur le vrai fichier (Magnus Carlsen)
        let mut line = " ".repeat(162);
        let put = |line: &mut String, start: usize, text: &str| {
            line.replace_range(start..start + text.len(), text);
        };
        put(&mut line, 0, "1503014");
        put(&mut line, 15, "Carlsen, Magnus");
        put(&mut line, 76, "NOR");
        put(&mut line, 80, "M");
        put(&mut line, 84, "GM");
        put(&mut line, 113, "2823");
        put(&mut line, 126, "2803");
        put(&mut line, 139, "2860");
        put(&mut line, 152, "1990");
        line
    }

    #[test]
    fn parse_line_reads_all_fields_at_the_right_offsets() {
        let p = parse_line(&sample_line()).unwrap();
        assert_eq!(p.fide_id, "1503014");
        assert_eq!(p.name, "Carlsen, Magnus");
        assert_eq!(p.fed, "NOR");
        assert_eq!(p.sex, "M");
        assert_eq!(p.title, "GM");
        assert_eq!(p.std_rating, Some(2823));
        assert_eq!(p.rapid_rating, Some(2803));
        assert_eq!(p.blitz_rating, Some(2860));
        assert_eq!(p.birth_year, Some(1990));
    }

    #[test]
    fn parse_line_treats_zero_rating_as_unrated() {
        let mut line = " ".repeat(162);
        line.replace_range(0..7, "1234567");
        line.replace_range(15..19, "Test");
        line.replace_range(113..114, "0");
        let p = parse_line(&line).unwrap();
        assert_eq!(p.std_rating, None);
    }

    #[test]
    fn parse_line_rejects_a_line_without_an_id() {
        let line = " ".repeat(162);
        assert!(parse_line(&line).is_none());
    }

    #[test]
    fn list_fide_players_page_filters_sorts_and_paginates() {
        let conn = open_at(Path::new(":memory:")).unwrap();
        let insert = |id: &str, name: &str, std: Option<i32>| {
            conn.execute(
                "INSERT INTO fide_players (fide_id, name, fed, sex, title, w_title, o_title, std_rating, rapid_rating, blitz_rating, birth_year, flag)
                 VALUES (?1, ?2, 'FRA', 'M', '', '', '', ?3, NULL, NULL, NULL, '')",
                params![id, name, std],
            )
            .unwrap();
        };
        insert("1", "Alice", Some(2400));
        insert("2", "Bob", Some(2600));
        insert("3", "Carol", None);

        assert_eq!(count_fide_players(&conn, "").unwrap(), 3);
        let by_rating = list_fide_players_page(&conn, "", "std", "desc", 10, 0).unwrap();
        assert_eq!(by_rating[0].name, "Bob");

        let filtered = list_fide_players_page(&conn, "Ali", "", "", 10, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Alice");

        let page1 = list_fide_players_page(&conn, "", "name", "asc", 2, 0).unwrap();
        assert_eq!(page1.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob"]);
        let page2 = list_fide_players_page(&conn, "", "name", "asc", 2, 2).unwrap();
        assert_eq!(page2.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Carol"]);
    }
}
