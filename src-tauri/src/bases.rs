use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
struct BaseEntry {
    name: String,
    path: String,
    #[serde(default)]
    is_reference: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BaseInfo {
    pub name: String,
    pub path: String,
    pub games: u32,
    pub is_reference: bool,
    pub missing: bool,
}

fn registry_file(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("bases.json")
}

fn load_registry(app_data_dir: &Path) -> Vec<BaseEntry> {
    match fs::read_to_string(registry_file(app_data_dir)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_registry(app_data_dir: &Path, entries: &[BaseEntry]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(registry_file(app_data_dir), text).map_err(|e| e.to_string())
}

pub fn registry_exists(app_data_dir: &Path) -> bool {
    registry_file(app_data_dir).exists()
}

fn count_games(path: &str) -> u32 {
    Connection::open(path)
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT COUNT(*) FROM games", [], |row| row.get::<_, i64>(0))
                .ok()
        })
        .unwrap_or(0) as u32
}

pub fn list_bases(app_data_dir: &Path) -> Vec<BaseInfo> {
    load_registry(app_data_dir)
        .into_iter()
        .map(|e| {
            let missing = !Path::new(&e.path).exists();
            BaseInfo {
                games: if missing { 0 } else { count_games(&e.path) },
                name: e.name,
                path: e.path,
                is_reference: e.is_reference,
                missing,
            }
        })
        .collect()
}

pub fn register_base(app_data_dir: &Path, name: &str, path: &str) -> Result<(), String> {
    let mut entries = load_registry(app_data_dir);
    if !entries.iter().any(|e| e.path == path) {
        entries.push(BaseEntry {
            name: name.to_string(),
            path: path.to_string(),
            is_reference: false,
        });
        save_registry(app_data_dir, &entries)?;
    }
    Ok(())
}

/// Une seule base de reference a la fois : la marquer desactive
/// automatiquement ce marquage sur toutes les autres.
pub fn set_reference(app_data_dir: &Path, path: &str, is_reference: bool) -> Result<(), String> {
    let mut entries = load_registry(app_data_dir);
    let exists = entries.iter().any(|e| e.path == path);
    if !exists {
        return Ok(());
    }
    for entry in entries.iter_mut() {
        entry.is_reference = is_reference && entry.path == path;
    }
    save_registry(app_data_dir, &entries)
}

pub fn has_reference(app_data_dir: &Path) -> bool {
    load_registry(app_data_dir).iter().any(|e| e.is_reference && Path::new(&e.path).exists())
}

pub fn unregister(app_data_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load_registry(app_data_dir);
    entries.retain(|e| e.path != path);
    save_registry(app_data_dir, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("chesscraft-bases-test-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_one_reference_base_at_a_time() {
        let dir = temp_dir();
        register_base(&dir, "A", "a.sqlite").unwrap();
        register_base(&dir, "B", "b.sqlite").unwrap();

        set_reference(&dir, "a.sqlite", true).unwrap();
        let bases = list_bases(&dir);
        assert!(bases.iter().find(|b| b.path == "a.sqlite").unwrap().is_reference);
        assert!(!bases.iter().find(|b| b.path == "b.sqlite").unwrap().is_reference);

        set_reference(&dir, "b.sqlite", true).unwrap();
        let bases = list_bases(&dir);
        assert!(!bases.iter().find(|b| b.path == "a.sqlite").unwrap().is_reference);
        assert!(bases.iter().find(|b| b.path == "b.sqlite").unwrap().is_reference);

        fs::remove_dir_all(&dir).ok();
    }
}
