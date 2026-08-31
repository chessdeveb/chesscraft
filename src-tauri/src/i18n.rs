use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Meme fichier de langue que le cote React (src/locales/*.json), embarque a
// la compilation : ajouter une langue = ajouter un fichier ici ET dans
// src/locales/, sans dupliquer les traductions. Cles a plat ("menu.file")
// plutot qu'imbriquees pour rester triviales a charger des deux cotes.
const FR: &str = include_str!("../../src/locales/fr.json");
const EN: &str = include_str!("../../src/locales/en.json");

pub fn translations(lang: &str) -> HashMap<String, String> {
    let raw = if lang == "en" { EN } else { FR };
    serde_json::from_str(raw).unwrap_or_default()
}

fn language_file(app_data_dir: &Path) -> std::path::PathBuf {
    app_data_dir.join("language.txt")
}

pub fn load_language(app_data_dir: &Path) -> String {
    match fs::read_to_string(language_file(app_data_dir)) {
        Ok(s) if s.trim() == "en" => "en".to_string(),
        _ => "fr".to_string(),
    }
}

pub fn save_language(app_data_dir: &Path, lang: &str) -> Result<(), String> {
    fs::write(language_file(app_data_dir), lang).map_err(|e| e.to_string())
}
