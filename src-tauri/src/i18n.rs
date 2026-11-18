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

/// Langue d'affichage de Windows, ramenee aux deux langues de l'appli :
/// "fr" si Windows est en francais, "en" pour toute autre langue. None si elle
/// ne peut pas etre lue (autre systeme que Windows, appel en echec).
#[cfg(windows)]
fn system_language() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    const LANG_FRENCH: u16 = 0x0C;
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid == 0 {
        return None;
    }
    // les 10 bits de poids faible : la langue sans la region (fr-FR, fr-CA, fr-BE...)
    Some(if langid & 0x3FF == LANG_FRENCH { "fr" } else { "en" }.to_string())
}

#[cfg(not(windows))]
fn system_language() -> Option<String> {
    None
}

/// Langue de l'appli : le choix enregistre par l'utilisateur (Preferences >
/// Langue), sinon, tant qu'il n'a rien choisi, la langue de Windows, sinon le
/// francais.
pub fn load_language(app_data_dir: &Path) -> String {
    match fs::read_to_string(language_file(app_data_dir)) {
        Ok(s) if s.trim() == "en" => "en".to_string(),
        Ok(s) if s.trim() == "fr" => "fr".to_string(),
        _ => system_language().unwrap_or_else(|| "fr".to_string()),
    }
}

pub fn save_language(app_data_dir: &Path, lang: &str) -> Result<(), String> {
    fs::write(language_file(app_data_dir), lang).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chesscraft-i18n-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_saved_choice_wins_over_the_system_language() {
        let dir = temp_dir("saved");
        save_language(&dir, "en").unwrap();
        assert_eq!(load_language(&dir), "en");
        save_language(&dir, "fr").unwrap();
        assert_eq!(load_language(&dir), "fr");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_saved_choice_the_language_follows_the_system_or_falls_back_to_french() {
        let dir = temp_dir("none");
        let expected = system_language().unwrap_or_else(|| "fr".to_string());
        assert_eq!(load_language(&dir), expected);
        assert!(expected == "fr" || expected == "en");
        let _ = fs::remove_dir_all(&dir);
    }
}
