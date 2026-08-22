use std::fs;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 10;

fn registry_file(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("recent_pgns.json")
}

fn load(app_data_dir: &Path) -> Vec<String> {
    match fs::read_to_string(registry_file(app_data_dir)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save(app_data_dir: &Path, entries: &[String]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(registry_file(app_data_dir), text).map_err(|e| e.to_string())
}

pub fn add_recent(app_data_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load(app_data_dir);
    entries.retain(|p| p != path);
    entries.insert(0, path.to_string());
    entries.truncate(MAX_RECENT);
    save(app_data_dir, &entries)
}

pub fn list_recent(app_data_dir: &Path) -> Vec<String> {
    load(app_data_dir)
}
