mod bases;
mod bulk_import;
mod chess_com;
// pub : reutilises directement par les outils de maintenance ponctuels sous
// src/bin/ (voir normalize_sans.rs), qui ne font pas partie de l'appli
// Tauri elle-meme -- ces modules doivent donc rester atteignables depuis
// l'exterieur de ce crate lib.
pub mod db;
mod eco;
mod engine;
pub mod explorer;
mod fide;
mod game_tree;
mod i18n;
mod lichess;
mod performance;
mod pgn;
mod recent_pgns;

use bases::BaseInfo;
use db::GameSummary;
use engine::EngineHandle;
use explorer::OpeningMoveStat;
use game_tree::{GameTree, MoveDto, ShapeDto, TreeDto};
use pgn::PgnGameSummary;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};

struct GameSession {
    tree: GameTree,
    db: Option<Connection>,
    active_base_path: Option<String>,
    active_game_id: Option<i64>,
    engine: Option<EngineHandle>,
    // Prepation d'adversaire (voir open_player_prep_tab) : restreint
    // l'explorateur d'ouvertures de cet onglet aux seules parties de ce
    // joueur, dans cette couleur. None pour un onglet normal.
    explorer_player_filter: Option<explorer::PlayerFilter>,
}

impl GameSession {
    fn new() -> Self {
        GameSession {
            tree: GameTree::new(),
            db: None,
            active_base_path: None,
            active_game_id: None,
            engine: None,
            explorer_player_filter: None,
        }
    }
}

struct AppState {
    sessions: Mutex<HashMap<u32, GameSession>>,
    next_tab_id: Mutex<u32>,
    bulk_import_cancel: Mutex<bool>,
    // Empeche deux imports en masse simultanes (ex. double-clic) : deux
    // ecrivains concurrents sur le meme fichier SQLite se marchent dessus
    // (verrous, transactions qui ne se voient pas mutuellement) et gonflent
    // artificiellement le compteur d'echecs.
    bulk_import_running: Mutex<bool>,
    // Meme raison que bulk_import_running : empeche deux rafraichissements
    // simultanes de la liste FIDE (voir refresh_fide_players).
    fide_refresh_running: Mutex<bool>,
    // Empeche deux nettoyages "+"/"#" simultanes de la meme base de reference
    // (voir maybe_start_check_suffix_cleanup) -- meme raison que
    // bulk_import_running/fide_refresh_running.
    check_suffixes_cleanup_running: Mutex<bool>,
    // Empeche deux operations de maintenance (menu Outils) simultanees --
    // meme raison que bulk_import_running. Partagee par les quatre (recalcul
    // stats, comptage doublons, dedoublonnage, identifiants FIDE) : rare que
    // l'utilisateur en lance deux a la fois, et ca evite en prime deux
    // ecrivains concurrents sur la meme base choisie.
    tool_job_running: Mutex<bool>,
    // Empeche deux rapports de partie simultanes (un seul process moteur
    // dedie a la fois, voir start_game_report) -- champ distinct de
    // tool_job_running car semantique differente (analyse d'une partie
    // precise dans un onglet, pas maintenance d'une base).
    game_report_running: Mutex<bool>,
    // Connexion vers la base de reference, reutilisee entre les appels de
    // l'explorateur d'ouvertures : en rouvrir une a chaque coup joue coute
    // tres cher sur une base de plusieurs Go (WAL a re-verifier a chaque
    // fois). Reouverte seulement si la base de reference a change.
    reference_conn: Mutex<Option<(String, Connection)>>,
    // Resultats de l'explorateur pour les prefixes deja demandes (position de
    // depart, "e4", "e4 e5", ...). Sur une base de plusieurs millions de
    // parties, les premiers coups sont interroges par toutes les parties et
    // restent couteux a agreger (l'essentiel de la base correspond au
    // prefixe) : vide seulement quand la base de reference change ou qu'un
    // import en masse la modifie.
    explorer_cache: Mutex<HashMap<String, Vec<OpeningMoveStat>>>,
    // Langue courante du menu natif (persistee dans language.txt) : le menu
    // est reconstruit et rattache a toutes les fenetres a chaque changement.
    language: Mutex<String>,
    // Cases a cocher "Affichage > panneau" (une par fenetre, cle
    // "<label fenetre>:<id case>") : gardees ici pour pouvoir les mettre a
    // jour individuellement (voir set_panel_checked) sans reconstruire tout
    // le menu -- necessaire pour que la case reflete l'etat reel d'un
    // panneau ferme via son propre bouton plutot que via le menu.
    panel_menu_items: Mutex<HashMap<String, tauri::menu::CheckMenuItem<tauri::Wry>>>,
}

fn allocate_tab_id(state: &AppState) -> u32 {
    let mut next = state.next_tab_id.lock().unwrap();
    let id = *next;
    *next += 1;
    id
}

fn insert_session(state: &AppState, tab_id: u32, session: GameSession) -> TreeDto {
    let dto = session.tree.to_dto();
    state.sessions.lock().unwrap().insert(tab_id, session);
    dto
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TabDto {
    tab_id: u32,
    tree: TreeDto,
    // Distingue cote frontend un onglet de preparation d'adversaire (voir
    // open_player_prep_tab) d'un onglet de partie classique : titre d'onglet
    // different, panneaux par defaut differents (explorateur ouvert, notation
    // fermee -- il n'y a pas de coups a lister).
    #[serde(default)]
    is_player_prep: bool,
}

#[tauri::command]
fn new_game(state: State<AppState>) -> TabDto {
    let tab_id = allocate_tab_id(&state);
    let tree = insert_session(&state, tab_id, GameSession::new());
    TabDto { tab_id, tree, is_player_prep: false }
}

/// Nouvel onglet demarrant sur une position choisie dans l'editeur de
/// position (Fichier > Nouvelle position) plutot qu'au depart standard.
#[tauri::command]
fn new_game_from_fen(state: State<AppState>, fen: String) -> Result<TabDto, String> {
    let tab_id = allocate_tab_id(&state);
    let mut session = GameSession::new();
    session.tree = GameTree::from_fen(&fen)?;
    let tree = insert_session(&state, tab_id, session);
    Ok(TabDto { tab_id, tree, is_player_prep: false })
}

/// Remplace la partie de l'onglet `tab_id` par une nouvelle partie Chess960
/// (bouton "type de jeu" pres de la navigation, fenetre partie) -- contrairement
/// a new_game_from_fen, ne cree pas de nouvel onglet : reinitialise celui en
/// cours, comme une "nouvelle partie" locale. La confirmation de perte des
/// coups deja joues est geree cote frontend avant l'appel.
#[tauri::command]
fn new_chess960_game(state: State<AppState>, tab_id: u32, position_number: u16) -> Result<TreeDto, String> {
    let fen = game_tree::chess960_start_fen(position_number)?;
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree = GameTree::from_fen(&fen)?;
    session.db = None;
    session.active_base_path = None;
    session.active_game_id = None;
    Ok(session.tree.to_dto())
}

/// Ouvre un nouvel onglet vierge dont l'explorateur d'ouvertures ne
/// considere que les parties de `name` jouees avec les blancs (white=true)
/// ou les noirs -- preparation d'adversaire depuis le clic droit sur la
/// liste des joueurs. Le nom du joueur est mis en tag White/Black de la
/// partie vide pour que l'onglet s'affiche deja avec un titre parlant.
#[tauri::command]
fn open_player_prep_tab(state: State<AppState>, name: String, white: bool) -> TabDto {
    let tab_id = allocate_tab_id(&state);
    let mut session = GameSession::new();
    session.tree.tags.insert(if white { "White" } else { "Black" }.to_string(), name.clone());
    session.explorer_player_filter = Some(explorer::PlayerFilter { name, white, exact: true });
    let tree = insert_session(&state, tab_id, session);
    TabDto { tab_id, tree, is_player_prep: true }
}

#[tauri::command]
fn close_tab(state: State<AppState>, tab_id: u32) {
    if let Some(mut session) = state.sessions.lock().unwrap().remove(&tab_id) {
        if let Some(mut engine) = session.engine.take() {
            engine.stop();
        }
    }
}

#[tauri::command]
fn get_tree(state: State<AppState>, tab_id: u32) -> Result<TreeDto, String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    Ok(session.tree.to_dto())
}

#[tauri::command]
fn get_active_base_path(state: State<AppState>, tab_id: u32) -> Result<Option<String>, String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    Ok(session.active_base_path.clone())
}

#[tauri::command]
fn legal_moves(state: State<AppState>, tab_id: u32, node_id: u32) -> Result<Vec<MoveDto>, String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.legal_moves(node_id)
}

#[tauri::command]
fn play_move(
    state: State<AppState>,
    tab_id: u32,
    node_id: u32,
    from: &str,
    to: &str,
    promotion: Option<&str>,
) -> Result<u32, String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.play_move(node_id, from, to, promotion)
}

#[tauri::command]
fn set_comment(
    state: State<AppState>,
    tab_id: u32,
    node_id: u32,
    before: Option<String>,
    after: Option<String>,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.set_comment(node_id, before, after)
}

#[tauri::command]
fn set_nags(state: State<AppState>, tab_id: u32, node_id: u32, nags: Vec<u8>) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.set_nags(node_id, nags)
}

#[tauri::command]
fn promote_variation(state: State<AppState>, tab_id: u32, node_id: u32) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.promote_variation(node_id)
}

#[tauri::command]
fn delete_variation(state: State<AppState>, tab_id: u32, node_id: u32) -> Result<u32, String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.delete_variation(node_id)
}

#[tauri::command]
fn set_shapes(state: State<AppState>, tab_id: u32, node_id: u32, shapes: Vec<ShapeDto>) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.set_shapes(node_id, shapes)
}

#[tauri::command]
fn set_tags(state: State<AppState>, tab_id: u32, tags: std::collections::BTreeMap<String, String>) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    session.tree.set_tags(tags);
    Ok(())
}

#[tauri::command]
fn list_pgn_games(app: AppHandle, path: String) -> Result<Vec<PgnGameSummary>, String> {
    // std::fs::read (octets bruts), pas read_to_string : un fichier PGN
    // importe n'est pas forcement en UTF-8 (Latin-1/Windows-1252 courant sur
    // les noms accentues) -- read_to_string plantait alors avec "stream did
    // not contain valid UTF-8" des l'ouverture. Voir pgn::list_games.
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let games = pgn::list_games(&bytes)?;
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = recent_pgns::add_recent(&dir, &path);
    }
    Ok(games)
}

#[tauri::command]
fn import_pgn(state: State<AppState>, path: String, index: u32) -> Result<TabDto, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let parsed = pgn::parse_game_at(&bytes, index)?;
    let mut session = GameSession::new();
    session.tree = parsed;
    let tab_id = allocate_tab_id(&state);
    let tree = insert_session(&state, tab_id, session);
    Ok(TabDto { tab_id, tree, is_player_prep: false })
}

// Journal durable des imports en masse : ecrit independamment de l'etat de
// l'interface, pour retrouver les chiffres meme si la fenetre de resultat a
// disparu (ex. rechargement silencieux de la webview sur un tres gros import).
fn log_bulk_import_result(
    app: &AppHandle,
    pgn_path: &str,
    db_path: &str,
    result: &Result<bulk_import::BulkImportSummary, String>,
) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = match result {
        Ok(summary) => serde_json::json!({
            "timestamp": timestamp,
            "pgnPath": pgn_path,
            "dbPath": db_path,
            "total": summary.total,
            "imported": summary.imported,
            "skippedDuplicates": summary.skipped_duplicates,
            "skippedInvalid": summary.skipped_invalid,
            "failed": summary.failed,
        }),
        Err(e) => serde_json::json!({
            "timestamp": timestamp,
            "pgnPath": pgn_path,
            "dbPath": db_path,
            "error": e,
        }),
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("import_log.jsonl"))
    {
        use std::io::Write;
        let _ = writeln!(file, "{entry}");
    }
}

/// Coeur commun a bulk_import_pgn (fichier local) et import_lichess_games
/// (telechargement direct) : ouvre la base, importe le flux PGN donne avec
/// deduplication + canonicalisation FIDE, recalcule les stats precalculees de
/// l'explorateur. `source` n'a besoin que de Read -- BufReader sur fichier ou
/// reponse HTTP en flux, jamais charge entierement en memoire (des bases
/// source comme LumbrasGigaBase depassent 8 Go ; voir bulk_import::bulk_import).
fn run_bulk_import(
    app: &AppHandle,
    db_path: &str,
    source: impl std::io::Read,
    mut on_progress: impl FnMut(&bulk_import::BulkImportSummary),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<bulk_import::BulkImportSummary, String> {
    let mut conn = db::open(Path::new(db_path))?;

    // Reecrit Blancs/Noirs vers leur forme canonique FIDE quand elle est
    // resolue sans ambiguite (voir fide::canonicalize, departage par Elo en
    // cas d'homonymie -- ex. "Giri, Anish" vs "Giri, Andrea"), et recupere au
    // passage l'identifiant FIDE (pose sur games.white_fide_id/black_fide_id,
    // voir bulk_import::process_one_game) -- memorise par (nom, indice Elo)
    // deja vu (fide_name_cache) : une source rejoue tres souvent les memes
    // joueurs a un Elo proche, inutile de refaire la recherche FIDE a chaque
    // partie (meme principe que rebuild_player_stats).
    // Base FIDE absente ou pas encore telechargee (voir refresh_fide_players) :
    // fide_conn est alors None et canonicalize_name ne fait rien, l'import se
    // deroule normalement.
    let fide_conn = app.path().app_data_dir().ok().and_then(|dir| fide::open(&dir).ok());
    let mut fide_name_cache: HashMap<(String, Option<i32>), Option<(String, String)>> = HashMap::new();
    let canonicalize_name = move |raw: &str, elo_hint: Option<i32>| -> Option<(String, String)> {
        let conn = fide_conn.as_ref()?;
        let cache_key = (raw.to_string(), elo_hint);
        if let Some(cached) = fide_name_cache.get(&cache_key) {
            return cached.clone();
        }
        let resolved = fide::canonicalize(conn, raw, elo_hint);
        fide_name_cache.insert(cache_key, resolved.clone());
        resolved
    };

    let summary = bulk_import::bulk_import(&mut conn, source, &mut on_progress, &mut should_cancel, canonicalize_name)?;
    // `positions` est deja a jour : chaque partie importee l'indexe en
    // ligne (voir bulk_import::process_one_game -> db::save_game ->
    // explorer::index_game, dont l'erreur est propagee, pas un simple cache
    // best-effort) -- un rebuild_position_index complet ici serait un second
    // balayage integral redondant. player_stats, lui, s'incremente aussi en
    // ligne mais en best-effort (`let _ =`, voir increment_player_stats) :
    // ce rebuild complet rattrape tout increment silencieusement manque.
    let _ = db::rebuild_player_stats(&mut conn, |_| {});
    Ok(summary)
}

#[tauri::command]
fn bulk_import_pgn(app: AppHandle, state: State<AppState>, db_path: String, pgn_path: String) -> Result<(), String> {
    {
        let mut running = state.bulk_import_running.lock().unwrap();
        if *running {
            return Err("un import en masse est déjà en cours".to_string());
        }
        *running = true;
    }
    *state.bulk_import_cancel.lock().unwrap() = false;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let cancel_app = app.clone();
        let result = (|| -> Result<bulk_import::BulkImportSummary, String> {
            let pgn_file = std::fs::File::open(&pgn_path).map_err(|e| e.to_string())?;
            let pgn_reader = std::io::BufReader::new(pgn_file);
            run_bulk_import(
                &app,
                &db_path,
                pgn_reader,
                move |summary| {
                    let _ = progress_app.emit("bulk-import-progress", summary.clone());
                },
                move || *cancel_app.state::<AppState>().bulk_import_cancel.lock().unwrap(),
            )
        })();

        log_bulk_import_result(&app, &pgn_path, &db_path, &result);
        // meme en cas d'echec partiel, des lots ont pu etre commit (voir
        // bulk_import::BATCH_SIZE) : le cache de l'explorateur pourrait donc
        // etre perime, on le vide dans tous les cas.
        app.state::<AppState>().explorer_cache.lock().unwrap().clear();

        match result {
            Ok(summary) => {
                let _ = app.emit("bulk-import-done", summary);
            }
            Err(e) => {
                let _ = app.emit("bulk-import-error", e);
            }
        }
        *app.state::<AppState>().bulk_import_running.lock().unwrap() = false;
    });
    Ok(())
}

/// Cadences proposees par les dialogues d'import Lichess ET chess.com (voir
/// lichess::PERF_TYPES / chess_com::PERF_TYPES -- les deux valent
/// litteralement ["bullet", "blitz", "rapid"], reutilise donc pour les deux
/// plutot que d'exposer une commande quasi identique par site) -- expose au
/// frontend pour ne pas dupliquer la liste exacte attendue par ces API.
#[tauri::command]
fn get_lichess_perf_types() -> Vec<String> {
    lichess::PERF_TYPES.iter().map(|s| s.to_string()).collect()
}

/// Bouton "Importer depuis Lichess" (accueil, a cote de "Importer en masse") :
/// telecharge en flux les parties d'un pseudo via l'API publique de Lichess
/// (aucun jeton requis) puis les fait passer par le meme import en masse que
/// pour un fichier PGN local (voir run_bulk_import) -- meme deduplication,
/// mêmes evenements de progression "bulk-import-*", donc reutilise tel quel
/// cote frontend.
#[tauri::command]
fn import_lichess_games(
    app: AppHandle,
    state: State<AppState>,
    db_path: String,
    username: String,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    perf_types: Vec<String>,
) -> Result<(), String> {
    let username = username.trim().to_string();
    if !lichess::valid_username(&username) {
        return Err("pseudo Lichess invalide".to_string());
    }
    if perf_types.is_empty() {
        return Err("choisissez au moins une cadence (bullet, blitz, rapide)".to_string());
    }
    {
        let mut running = state.bulk_import_running.lock().unwrap();
        if *running {
            return Err("un import en masse est déjà en cours".to_string());
        }
        *running = true;
    }
    *state.bulk_import_cancel.lock().unwrap() = false;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let cancel_app = app.clone();
        let url = lichess::export_url(&username, since_ms, until_ms, &perf_types);
        let result = (|| -> Result<bulk_import::BulkImportSummary, String> {
            let resp = ureq::get(&url)
                .call()
                .map_err(|e| format!("Téléchargement Lichess échoué : {e}"))?;
            run_bulk_import(
                &app,
                &db_path,
                resp.into_reader(),
                move |summary| {
                    let _ = progress_app.emit("bulk-import-progress", summary.clone());
                },
                move || *cancel_app.state::<AppState>().bulk_import_cancel.lock().unwrap(),
            )
        })();

        log_bulk_import_result(&app, &format!("lichess:{username}"), &db_path, &result);
        app.state::<AppState>().explorer_cache.lock().unwrap().clear();

        match result {
            Ok(summary) => {
                let _ = app.emit("bulk-import-done", summary);
            }
            Err(e) => {
                let _ = app.emit("bulk-import-error", e);
            }
        }
        *app.state::<AppState>().bulk_import_running.lock().unwrap() = false;
    });
    Ok(())
}

/// Bouton "Importer > Un compte chess.com" (accueil) : telecharge les
/// archives mensuelles d'un pseudo via l'API publique chess.com (aucun jeton
/// requis), filtrees par periode/cadence (voir chess_com::download_pgn), puis
/// les fait passer par le meme import en masse que Lichess/PGN local (voir
/// run_bulk_import) -- pas de flux ici (contrairement a Lichess) : chess.com
/// n'expose les parties que par archive mensuelle complete, tout doit donc
/// etre assemble en memoire avant de les passer au parseur PGN.
#[tauri::command]
fn import_chesscom_games(
    app: AppHandle,
    state: State<AppState>,
    db_path: String,
    username: String,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    perf_types: Vec<String>,
) -> Result<(), String> {
    let username = username.trim().to_string();
    if !chess_com::valid_username(&username) {
        return Err("pseudo chess.com invalide".to_string());
    }
    if perf_types.is_empty() {
        return Err("choisissez au moins une cadence (bullet, blitz, rapide)".to_string());
    }
    {
        let mut running = state.bulk_import_running.lock().unwrap();
        if *running {
            return Err("un import en masse est déjà en cours".to_string());
        }
        *running = true;
    }
    *state.bulk_import_cancel.lock().unwrap() = false;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let cancel_app = app.clone();
        let result = (|| -> Result<bulk_import::BulkImportSummary, String> {
            let pgn_bytes = chess_com::download_pgn(&username, since_ms, until_ms, &perf_types)?;
            run_bulk_import(
                &app,
                &db_path,
                std::io::Cursor::new(pgn_bytes),
                move |summary| {
                    let _ = progress_app.emit("bulk-import-progress", summary.clone());
                },
                move || *cancel_app.state::<AppState>().bulk_import_cancel.lock().unwrap(),
            )
        })();

        log_bulk_import_result(&app, &format!("chesscom:{username}"), &db_path, &result);
        app.state::<AppState>().explorer_cache.lock().unwrap().clear();

        match result {
            Ok(summary) => {
                let _ = app.emit("bulk-import-done", summary);
            }
            Err(e) => {
                let _ = app.emit("bulk-import-error", e);
            }
        }
        *app.state::<AppState>().bulk_import_running.lock().unwrap() = false;
    });
    Ok(())
}

#[tauri::command]
fn cancel_bulk_import(state: State<AppState>) {
    *state.bulk_import_cancel.lock().unwrap() = true;
}

/// Empeche deux operations de maintenance simultanees (voir
/// AppState::tool_job_running) -- a appeler en tout premier dans chacune des
/// quatre commandes ci-dessous, avant de lancer le thread.
fn begin_tool_job(state: &State<AppState>) -> Result<(), String> {
    let mut running = state.tool_job_running.lock().unwrap();
    if *running {
        return Err("une opération de maintenance est déjà en cours".to_string());
    }
    *running = true;
    Ok(())
}

/// Progression periodique d'une operation de maintenance (menu Outils),
/// emise en tache de fond pendant le calcul -- voir les quatre commandes
/// ci-dessous. `matched` n'a de sens que pour Dedoublonner (doublons
/// trouves/supprimes) et Resoudre les identifiants FIDE (parties resolues) ;
/// pour Recalculer les stats de l'explorateur, seul `examined` est
/// pertinent (0 fige pour `matched`), voir ToolBaseWorking cote frontend.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ToolProgress {
    examined: u64,
    matched: u64,
}

/// Progression d'un rebuild_position_index/rebuild_player_stats, avec le nom
/// de l'etape en cours ("openings"/"players") -- voir
/// explorer::RebuildProgress pour la distinction lecture/ecriture (les
/// additionner en un seul compteur, essaye d'abord, produisait un total
/// depassant le nombre de parties de la base des que l'ecriture avait plus
/// de lignes distinctes que la lecture n'a de parties, rapporte par
/// l'utilisateur comme un chiffre qui ne semblait pas avoir de sens).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ToolRebuildProgress {
    stage: &'static str,
    #[serde(flatten)]
    progress: explorer::RebuildProgress,
}

/// Menu Outils > Recalculer les stats de l'explorateur : reconstruit
/// positions/player_stats sur une base choisie manuellement (voir
/// explorer::rebuild_position_index, db::rebuild_player_stats). Sert a rattraper
/// un import en masse interrompu avant sa fin (l'appli fermee en cours de
/// route) -- ce recalcul automatique n'a alors jamais eu lieu, voir
/// bulk_import_pgn.
///
/// En tache de fond (thread dedie, meme mecanique que bulk_import_pgn) : une
/// premiere version synchrone (executee directement dans le handler de la
/// commande) rendait la fenetre entiere non-reactive le temps du calcul --
/// sur la base de reference reelle de l'utilisateur (plusieurs millions de
/// parties), assez longtemps pour que Windows signale carrement l'appli comme
/// "ne repond plus", meme avec un message affiche avant l'appel. Le probleme
/// n'etait donc pas juste l'absence de retour visuel (voir la premiere
/// tentative de correctif) mais bien le blocage de la boucle de messages
/// Win32 par un calcul synchrone dans le handler de commande -- la commande
/// doit rendre la main immediatement, le vrai travail se faisant sur un
/// thread separe qui previent via un evenement une fois termine.
#[tauri::command]
fn rebuild_explorer_stats(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    begin_tool_job(&state)?;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = (|| -> Result<(), String> {
            let mut conn = db::open(Path::new(&path))?;
            // Rattrapage start_fen (voir db::backfill_start_fen) : doit passer
            // AVANT rebuild_position_index, qui rejoue chaque partie depuis sa
            // colonne start_fen (parties a depart personnalise enregistrees
            // avant l'ajout de cette colonne).
            db::backfill_start_fen(&mut conn, |examined| {
                let _ = progress_app.emit(
                    "tool-rebuild-stats-progress",
                    ToolRebuildProgress { stage: "startFen", progress: explorer::RebuildProgress::Reading { examined } },
                );
            }, || false)?;
            explorer::rebuild_position_index(&mut conn, |progress| {
                let _ = progress_app.emit("tool-rebuild-stats-progress", ToolRebuildProgress { stage: "openings", progress });
            }, || false)?;
            // Restes des anciennes recherches de position (ancienne table
            // position_index, opening_prefix_stats...) : inutiles une fois
            // `positions` reconstruite, on les retire.
            db::drop_legacy_explorer_structures(&conn)?;
            db::rebuild_player_stats(&mut conn, |progress| {
                let _ = progress_app.emit("tool-rebuild-stats-progress", ToolRebuildProgress { stage: "players", progress });
            })?;
            // Rend au disque l'espace libere par les tables supprimees.
            db::compact_if_worthwhile(&conn, || {
                let _ = progress_app.emit(
                    "tool-rebuild-stats-progress",
                    ToolRebuildProgress { stage: "compact", progress: explorer::RebuildProgress::Writing { written: 0, total: 0 } },
                );
            })?;
            Ok(())
        })();
        app.state::<AppState>().explorer_cache.lock().unwrap().clear();
        let _ = match &result {
            Ok(()) => app.emit("tool-rebuild-stats-done", ()),
            Err(e) => app.emit("tool-rebuild-stats-error", e),
        };
        *app.state::<AppState>().tool_job_running.lock().unwrap() = false;
    });
    Ok(())
}

/// Menu Outils > Dedoublonner la base, etape 1/2 : compte les doublons sans
/// rien supprimer (voir db::count_duplicate_games), pour confirmer avant
/// suppression. En tache de fond pour la meme raison que
/// rebuild_explorer_stats -- le GROUP BY sur cinq colonnes non couvertes par
/// un index necessite un tri/hachage complet de toute la table.
#[tauri::command]
fn count_duplicate_games(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    begin_tool_job(&state)?;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = (|| -> Result<u32, String> {
            let mut conn = db::open(Path::new(&path))?;
            db::count_duplicate_games(&mut conn, |examined, matched| {
                let _ = progress_app.emit("tool-count-duplicates-progress", ToolProgress { examined, matched });
            })
        })();
        let _ = match &result {
            Ok(count) => app.emit("tool-count-duplicates-done", count),
            Err(e) => app.emit("tool-count-duplicates-error", e),
        };
        *app.state::<AppState>().tool_job_running.lock().unwrap() = false;
    });
    Ok(())
}

/// Menu Outils > Dedoublonner la base, etape 2/2 (apres confirmation) :
/// supprime les doublons puis relance le recalcul des stats de l'explorateur
/// (des parties ont disparu, voir db::deduplicate_games) -- meme sequence
/// qu'un import en masse. Emet le nombre de parties supprimees. En tache de
/// fond, meme raison que rebuild_explorer_stats.
#[tauri::command]
fn deduplicate_base(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    begin_tool_job(&state)?;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = (|| -> Result<u32, String> {
            let mut conn = db::open(Path::new(&path))?;
            let deleted = db::deduplicate_games(&mut conn, |examined, matched| {
                let _ = progress_app.emit("tool-deduplicate-progress", ToolProgress { examined, matched });
            })?;
            if deleted > 0 {
                // Evenement distinct de "tool-deduplicate-progress" (etape de
                // detection/suppression, forme (examined, matched)) : cette
                // etape-ci a sa propre forme (stage + phase lecture/ecriture,
                // voir ToolRebuildProgress), les melanger sous le meme nom
                // d'evenement aurait ete ambigu a interpreter cote frontend.
                db::rebuild_player_stats(&mut conn, |progress| {
                    let _ = progress_app.emit("tool-deduplicate-rebuild-progress", ToolRebuildProgress { stage: "players", progress });
                })?;
            }
            Ok(deleted)
        })();
        app.state::<AppState>().explorer_cache.lock().unwrap().clear();
        let _ = match &result {
            Ok(deleted) => app.emit("tool-deduplicate-done", deleted),
            Err(e) => app.emit("tool-deduplicate-error", e),
        };
        *app.state::<AppState>().tool_job_running.lock().unwrap() = false;
    });
    Ok(())
}

/// Menu Outils > Resoudre les identifiants FIDE : renseigne
/// white_fide_id/black_fide_id sur les parties qui n'en ont pas encore (voir
/// db::backfill_fide_ids) -- necessaire pour les bases importees avant
/// l'ajout de cette resolution, ou importees sans base FIDE encore
/// telechargee a l'epoque. Base FIDE absente : fide::open la cree vide,
/// backfill_fide_ids ne resoudra alors simplement rien (0 parties
/// renseignees), pas une erreur. En tache de fond, meme raison que
/// rebuild_explorer_stats.
#[tauri::command]
fn backfill_fide_ids(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    begin_tool_job(&state)?;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = (|| -> Result<u32, String> {
            let mut conn = db::open(Path::new(&path))?;
            let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let fide_conn = fide::open(&dir)?;
            db::backfill_fide_ids(&mut conn, &fide_conn, |examined, matched| {
                let _ = progress_app.emit("tool-backfill-fide-ids-progress", ToolProgress { examined, matched });
            })
        })();
        let _ = match &result {
            Ok(resolved) => app.emit("tool-backfill-fide-ids-done", resolved),
            Err(e) => app.emit("tool-backfill-fide-ids-error", e),
        };
        *app.state::<AppState>().tool_job_running.lock().unwrap() = false;
    });
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FideRefreshDone {
    count: u32,
}

/// Telecharge et reimporte entierement la liste des joueurs FIDE (voir
/// fide::refresh) en tache de fond -- meme mecanique que bulk_import_pgn
/// (thread dedie, evenements de progression, verrou anti-double-declenchement).
#[tauri::command]
fn refresh_fide_players(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    {
        let mut running = state.fide_refresh_running.lock().unwrap();
        if *running {
            return Err("un rafraîchissement de la liste FIDE est déjà en cours".to_string());
        }
        *running = true;
    }
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = fide::refresh(&app_data_dir, move |count| {
            let _ = progress_app.emit("fide-refresh-progress", count);
        });
        match result {
            Ok(count) => {
                let _ = app.emit("fide-refresh-done", FideRefreshDone { count });
            }
            Err(e) => {
                let _ = app.emit("fide-refresh-error", e);
            }
        }
        *app.state::<AppState>().fide_refresh_running.lock().unwrap() = false;
    });
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FidePlayersPage {
    players: Vec<fide::FidePlayer>,
    total: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FideStatus {
    last_updated: Option<String>,
    total: u32,
}

#[tauri::command]
fn get_fide_status(app: AppHandle) -> Result<FideStatus, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let conn = fide::open(&app_data_dir)?;
    Ok(FideStatus { last_updated: fide::last_updated(&conn), total: fide::count_fide_players(&conn, "")? })
}

#[tauri::command]
fn list_fide_players(
    app: AppHandle,
    search: String,
    sort_by: String,
    sort_dir: String,
    offset: u32,
    limit: u32,
) -> Result<FidePlayersPage, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let conn = fide::open(&app_data_dir)?;
    let total = fide::count_fide_players(&conn, &search)?;
    let players = fide::list_fide_players_page(&conn, &search, &sort_by, &sort_dir, limit, offset)?;
    Ok(FidePlayersPage { players, total })
}

#[tauri::command]
fn list_recent_pgns(app: AppHandle) -> Result<Vec<BaseInfo>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for path in recent_pgns::list_recent(&dir) {
        let name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .to_string();
        let games = std::fs::read(&path)
            .ok()
            .and_then(|bytes| pgn::list_games(&bytes).ok())
            .map(|g| g.len() as u32)
            .unwrap_or(0);
        out.push(BaseInfo { name, path, games, is_reference: false, missing: false });
    }
    Ok(out)
}

#[tauri::command]
fn export_pgn(state: State<AppState>, tab_id: u32, path: String) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    let pgn = pgn::write_game(&session.tree);
    std::fs::write(&path, pgn).map_err(|e| e.to_string())
}

#[tauri::command]
fn start_engine(app: AppHandle, state: State<AppState>, tab_id: u32, path: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    if let Some(mut existing) = session.engine.take() {
        existing.stop();
    }
    session.engine = Some(engine::start(app, &path, tab_id)?);
    Ok(())
}

#[tauri::command]
fn stop_engine(state: State<AppState>, tab_id: u32) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    if let Some(mut existing) = session.engine.take() {
        existing.stop();
    }
    Ok(())
}

#[tauri::command]
fn analyze_position(
    state: State<AppState>,
    tab_id: u32,
    fen: String,
    depth: u32,
    multipv: u32,
    threads: u32,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    match session.engine.as_mut() {
        Some(handle) => handle.analyze(&fen, depth, multipv, threads),
        None => Err("le moteur n'est pas démarré".to_string()),
    }
}

#[tauri::command]
fn logical_cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

#[tauri::command]
fn pause_engine(state: State<AppState>, tab_id: u32) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    match session.engine.as_mut() {
        Some(handle) => handle.pause(),
        None => Err("le moteur n'est pas démarré".to_string()),
    }
}

// --- Rapport de partie (bouton "Rapport de partie" du panneau Notation) ---
//
// Modele repris de lichess (formules publiees dans lila/modules/analyse/src/main/Advice.scala) :
// conversion score -> pourcentage de gain (Win%), puis perte de Win% d'un
// coup a l'autre pour la precision et la classification (gaffe/erreur/
// imprecision). L'agregation par partie ici est une simple moyenne des
// precisions par coup -- lichess utilise en vrai une moyenne ponderee par
// fenetre glissante combinee a une moyenne harmonique (explique a
// l'utilisateur), volontairement simplifiee ici plutot que reproduite a
// l'identique.

/// Progression du rapport de partie -- evenement propre a l'onglet
/// ("game-report-progress-{tab_id}"), meme principe que "engine-info-{tab}",
/// pour ne pas melanger deux rapports lances depuis deux onglets differents.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameReportProgress {
    done: usize,
    total: usize,
}

/// Un point de la courbe d'evaluation (voir GameReportChart.tsx) : une
/// entree par position de la ligne principale, racine (ply 0, avant le
/// premier coup) incluse pour que la courbe parte d'un point defini.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameReportMoveEval {
    node_id: u32,
    ply: u32,
    // SAN du coup qui MENE a cette position (None pour la racine, ply 0) --
    // pour l'infobulle du graphique cote frontend ("12... Nc3 éval : +0.23").
    san: Option<String>,
    white_win_percent: f64,
    score_cp: Option<i32>,
    score_mate: Option<i32>,
    // Code NAG (2/4/6, voir classify_drop) si ce coup est classe comme
    // imprecision/erreur/gaffe ; None pour la racine et les coups corrects.
    classification: Option<u8>,
}

/// Statistiques d'un rapport de partie pour un camp (voir le panneau
/// "Analyse de l'ordinateur" de chess.com/lichess, dont s'inspire le
/// panneau cote frontend).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameReportSideStats {
    accuracy: f64,
    blunders: u32,
    mistakes: u32,
    inaccuracies: u32,
    // Perte moyenne en centipions (ACPL) -- moyenne, sur les coups de ce
    // camp, de la difference d'evaluation (dans sa propre perspective,
    // jamais negative) entre avant et apres son coup.
    avg_centipawn_loss: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameReportSummary {
    white: GameReportSideStats,
    black: GameReportSideStats,
    moves: Vec<GameReportMoveEval>,
}

/// Empeche deux rapports de partie simultanes (voir AppState::game_report_running).
fn begin_game_report(state: &State<AppState>) -> Result<(), String> {
    let mut running = state.game_report_running.lock().unwrap();
    if *running {
        return Err("un rapport de partie est déjà en cours".to_string());
    }
    *running = true;
    Ok(())
}

fn fen_white_to_move(fen: &str) -> bool {
    fen.split_whitespace().nth(1) != Some("b")
}

/// Equivalent centipions d'un score, mat compris (approxime -- lichess a sa
/// propre regle de conversion, non documentee publiquement) : un mat en N
/// sature vite vers +-100% de Win%, seule la magnitude relative compte pour
/// nos seuils de classification.
fn score_to_cp_equivalent(score_cp: Option<i32>, score_mate: Option<i32>) -> f64 {
    if let Some(mate) = score_mate {
        let magnitude = 10_000.0 - (mate.unsigned_abs() as f64).min(100.0) * 10.0;
        if mate > 0 { magnitude } else { -magnitude }
    } else {
        score_cp.unwrap_or(0) as f64
    }
}

/// Pourcentage de gain du point de vue des Blancs (formule lichess).
fn win_percent(score_cp: Option<i32>, score_mate: Option<i32>) -> f64 {
    let cp = score_to_cp_equivalent(score_cp, score_mate);
    50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * cp).exp()) - 1.0)
}

fn mover_win_percent(eval: &engine::PositionEval, mover_white: bool) -> f64 {
    let white_win = win_percent(eval.score_cp, eval.score_mate);
    if mover_white { white_win } else { 100.0 - white_win }
}

/// Evaluation en centipions du point de vue du joueur au trait (positif =
/// bon pour lui) -- pour la perte moyenne en centipions (ACPL), distincte
/// du Win% utilise pour la precision/classification.
fn mover_cp(eval: &engine::PositionEval, mover_white: bool) -> f64 {
    let white_cp = score_to_cp_equivalent(eval.score_cp, eval.score_mate);
    if mover_white { white_cp } else { -white_cp }
}

/// Precision d'un coup a partir de la perte de Win% qu'il occasionne (formule lichess).
fn move_accuracy(win_percent_drop: f64) -> f64 {
    (103.1668 * (-0.04354 * win_percent_drop).exp() - 3.1669).clamp(0.0, 100.0)
}

/// Classe un coup en gaffe/erreur/imprecision selon la perte de Win% (seuils
/// lichess : >=30/20/10) -- ramene au code NAG le plus proche deja propose
/// par l'appli (voir nag.ts, MOVE_NAG_CHOICES) : ?? gaffe, ? erreur, ?!
/// imprecision (pas de code NAG standard dedie a l'imprecision, ?! est
/// celui qui s'en rapproche le plus parmi ceux deja geres par l'appli).
fn classify_drop(win_percent_drop: f64) -> Option<u8> {
    if win_percent_drop >= 30.0 {
        Some(4)
    } else if win_percent_drop >= 20.0 {
        Some(2)
    } else if win_percent_drop >= 10.0 {
        Some(6)
    } else {
        None
    }
}

fn format_eval(eval: &engine::PositionEval) -> String {
    if let Some(mate) = eval.score_mate {
        format!("#{mate}")
    } else {
        format!("{:.2}", eval.score_cp.unwrap_or(0) as f64 / 100.0)
    }
}

/// Retire une eventuelle annotation "[%eval ...]" deja presente (rapport
/// relance sur la meme partie) avant d'ajouter la nouvelle -- sinon elles
/// s'empilent a chaque relance.
fn strip_eval_tag(comment: &str) -> String {
    let mut out = comment.to_string();
    if let Some(start) = out.find("[%eval") {
        if let Some(end_rel) = out[start..].find(']') {
            let end = start + end_rel + 1;
            out.replace_range(start..end, "");
        }
    }
    out.trim().to_string()
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        100.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Retrouve dans un commentaire l'evaluation ecrite par format_eval (voir
/// apply_game_report) : "#N"/"#-N" pour un mat, sinon un nombre de pions
/// (converti en centipions). None si le tag est absent ou mal forme.
fn parse_eval_tag(comment: &str) -> Option<(Option<i32>, Option<i32>)> {
    let start = comment.find("[%eval")?;
    let rest = &comment[start + "[%eval".len()..];
    let end = rest.find(']')?;
    let value = rest[..end].trim();
    if let Some(mate_str) = value.strip_prefix('#') {
        Some((None, Some(mate_str.parse().ok()?)))
    } else {
        let pawns: f64 = value.parse().ok()?;
        Some((Some((pawns * 100.0).round() as i32), None))
    }
}

/// Applique les evaluations calculees a l'arbre : classification (NAG) et
/// annotation "[%eval ...]" (convention PGN standard, voir parseClock cote
/// frontend pour le precedent avec "[%clk ...]") sur chaque coup de la
/// ligne principale -- se retrouve donc automatiquement dans la base au
/// prochain enregistrement de la partie (pgn::write_game serialise les
/// commentaires), sans mecanisme de sauvegarde dedie.
#[derive(Default)]
struct SideAccumulator {
    accuracies: Vec<f64>,
    cp_losses: Vec<f64>,
    blunders: u32,
    mistakes: u32,
    inaccuracies: u32,
}

impl SideAccumulator {
    fn into_stats(self) -> GameReportSideStats {
        GameReportSideStats {
            accuracy: mean(&self.accuracies),
            blunders: self.blunders,
            mistakes: self.mistakes,
            inaccuracies: self.inaccuracies,
            avg_centipawn_loss: if self.cp_losses.is_empty() {
                0.0
            } else {
                self.cp_losses.iter().sum::<f64>() / self.cp_losses.len() as f64
            },
        }
    }
}

/// Calcule le rapport (precision, classification par coup) a partir
/// d'evaluations deja connues -- pure, ne modifie pas l'arbre. Utilisee a la
/// fois juste apres une analyse fraiche (apply_game_report, qui annote
/// ensuite l'arbre a partir du meme resultat) et pour reconstruire l'affichage
/// depuis les "[%eval ...]" deja sauvegardes a la reouverture d'une partie
/// (voir game_report_from_annotations).
fn compute_report_summary(
    tree: &GameTree,
    mainline_ids: &[u32],
    fens: &[String],
    evals: &[engine::PositionEval],
) -> GameReportSummary {
    let mut white_acc = SideAccumulator::default();
    let mut black_acc = SideAccumulator::default();
    let mut moves = Vec::with_capacity(mainline_ids.len());
    moves.push(GameReportMoveEval {
        node_id: mainline_ids[0],
        ply: 0,
        san: None,
        white_win_percent: win_percent(evals[0].score_cp, evals[0].score_mate),
        score_cp: evals[0].score_cp,
        score_mate: evals[0].score_mate,
        classification: None,
    });

    for i in 1..mainline_ids.len() {
        let node_id = mainline_ids[i];
        let mover_white = fen_white_to_move(&fens[i - 1]);
        let win_before = mover_win_percent(&evals[i - 1], mover_white);
        let win_after = mover_win_percent(&evals[i], mover_white);
        let drop = win_before - win_after;
        let accuracy = move_accuracy(drop);
        let cp_loss = (mover_cp(&evals[i - 1], mover_white) - mover_cp(&evals[i], mover_white)).max(0.0);
        let acc = if mover_white { &mut white_acc } else { &mut black_acc };
        acc.accuracies.push(accuracy);
        acc.cp_losses.push(cp_loss);

        let classification = classify_drop(drop);
        if let Some(code) = classification {
            match code {
                4 => acc.blunders += 1,
                2 => acc.mistakes += 1,
                6 => acc.inaccuracies += 1,
                _ => {}
            }
        }

        moves.push(GameReportMoveEval {
            node_id,
            ply: i as u32,
            san: tree.node(node_id).and_then(|n| n.san.clone()),
            white_win_percent: win_percent(evals[i].score_cp, evals[i].score_mate),
            score_cp: evals[i].score_cp,
            score_mate: evals[i].score_mate,
            classification,
        });
    }

    GameReportSummary {
        white: white_acc.into_stats(),
        black: black_acc.into_stats(),
        moves,
    }
}

/// Ecrit une annotation "[%eval ...]" sur le noeud donne, en remplacant une
/// eventuelle annotation precedente (rapport relance sur la meme partie) --
/// commun a la racine (evaluation avant le premier coup, comment_after
/// puisqu'elle n'a pas de coup propre) et a chaque coup de la ligne
/// principale.
fn annotate_eval(tree: &mut GameTree, node_id: u32, eval: &engine::PositionEval) {
    let existing_after = tree.node(node_id).and_then(|n| n.comment_after.clone()).unwrap_or_default();
    let stripped = strip_eval_tag(&existing_after);
    let eval_tag = format!("[%eval {}]", format_eval(eval));
    let comment = if stripped.is_empty() { eval_tag } else { format!("{stripped} {eval_tag}") };
    let before = tree.node(node_id).and_then(|n| n.comment_before.clone());
    let _ = tree.set_comment(node_id, before, Some(comment));
}

fn apply_game_report(
    tree: &mut GameTree,
    mainline_ids: &[u32],
    fens: &[String],
    evals: &[engine::PositionEval],
) -> GameReportSummary {
    let summary = compute_report_summary(tree, mainline_ids, fens, evals);

    annotate_eval(tree, mainline_ids[0], &evals[0]);
    for (i, mv) in summary.moves.iter().enumerate().skip(1) {
        let mut nags = Vec::new();
        if let Some(code) = mv.classification {
            nags.push(code);
        }
        let _ = tree.set_nags(mv.node_id, nags);
        annotate_eval(tree, mv.node_id, &evals[i]);
    }

    summary
}

/// Reconstruit le rapport de partie a partir des "[%eval ...]" deja
/// sauvegardes sur la ligne principale (voir apply_game_report/annotate_eval)
/// -- pour le reafficher automatiquement a la reouverture d'une partie deja
/// analysee, sans relancer le moteur. None si la ligne principale ne porte
/// pas d'evaluation sur chacun de ses noeuds (jamais analysee, ou analyse
/// partielle/tronquee depuis).
fn game_report_from_annotations(tree: &GameTree) -> Option<GameReportSummary> {
    let mainline_ids = tree.mainline_node_ids();
    if mainline_ids.len() < 2 {
        return None;
    }
    let mut evals = Vec::with_capacity(mainline_ids.len());
    for &id in &mainline_ids {
        let comment = tree.node(id).and_then(|n| n.comment_after.clone()).unwrap_or_default();
        let (score_cp, score_mate) = parse_eval_tag(&comment)?;
        evals.push(engine::PositionEval { score_cp, score_mate });
    }
    let fens: Vec<String> = mainline_ids.iter().map(|&id| tree.node_fen(id).unwrap_or_default()).collect();
    Some(compute_report_summary(tree, &mainline_ids, &fens, &evals))
}

#[tauri::command]
fn get_game_report(state: State<AppState>, tab_id: u32) -> Result<Option<GameReportSummary>, String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
    Ok(game_report_from_annotations(&session.tree))
}

#[cfg(test)]
mod game_report_tests {
    use super::*;

    fn eval(cp: i32) -> engine::PositionEval {
        engine::PositionEval { score_cp: Some(cp), score_mate: None }
    }

    #[test]
    fn parse_eval_tag_reads_centipawns_and_mate() {
        assert_eq!(parse_eval_tag("[%eval 1.25]"), Some((Some(125), None)));
        assert_eq!(parse_eval_tag("[%eval -0.40]"), Some((Some(-40), None)));
        assert_eq!(parse_eval_tag("[%eval #-3]"), Some((None, Some(-3))));
        assert_eq!(parse_eval_tag("some text [%eval #5] more"), Some((None, Some(5))));
        assert_eq!(parse_eval_tag("no eval here"), None);
    }

    // Cas rapporte par l'utilisateur : le rapport de partie disparaissait a
    // la reouverture d'une partie sauvegardee -- alors que les annotations
    // dont il depend (NAG + "[%eval ...]") sont bien persistees. Verifie
    // qu'a partir des seules annotations ecrites par apply_game_report,
    // game_report_from_annotations reconstruit le meme rapport sans redonner
    // les evaluations d'origine.
    #[test]
    fn game_report_survives_a_save_reload_round_trip() {
        let mut tree = GameTree::new();
        let n1 = tree.play_move(0, "e2", "e4", None).unwrap();
        tree.play_move(n1, "e7", "e5", None).unwrap();
        let mainline_ids = tree.mainline_node_ids();
        let fens: Vec<String> = mainline_ids.iter().map(|&id| tree.node_fen(id).unwrap()).collect();
        // net effondrement du Win% des Blancs apres 1. e4 -- doit se classer gaffe (NAG 4).
        let evals = vec![eval(20), eval(-600), eval(-590)];

        let live = apply_game_report(&mut tree, &mainline_ids, &fens, &evals);
        assert_eq!(live.white.blunders, 1);
        assert_eq!(tree.node(n1).unwrap().nags, vec![4]);

        let reloaded = game_report_from_annotations(&tree).expect("rapport reconstruit depuis les annotations");
        assert_eq!(reloaded.white.blunders, live.white.blunders);
        assert_eq!(reloaded.moves.len(), live.moves.len());
        for (a, b) in reloaded.moves.iter().zip(live.moves.iter()) {
            assert_eq!(a.classification, b.classification);
            assert_eq!(a.score_cp, b.score_cp);
        }
    }

    #[test]
    fn game_report_from_annotations_is_none_when_never_analyzed() {
        let mut tree = GameTree::new();
        tree.play_move(0, "e2", "e4", None).unwrap();
        assert!(game_report_from_annotations(&tree).is_none());
    }
}

/// Bouton "Rapport de partie" du panneau Notation : analyse chaque position
/// de la ligne principale avec un process moteur dedie (voir
/// engine::analyze_positions), puis annote l'arbre (NAG + "[%eval ...]").
/// En tache de fond (meme raison que rebuild_explorer_stats : ne pas geler
/// la boucle de messages Win32 le temps de l'analyse).
#[tauri::command]
fn start_game_report(
    app: AppHandle,
    state: State<AppState>,
    tab_id: u32,
    engine_path: String,
    depth: u32,
    threads: u32,
) -> Result<(), String> {
    begin_game_report(&state)?;

    let extraction = {
        let sessions = state.sessions.lock().unwrap();
        sessions.get(&tab_id).map(|session| {
            let ids = session.tree.mainline_node_ids();
            let fens: Vec<String> = ids.iter().map(|&id| session.tree.node_fen(id).unwrap_or_default()).collect();
            (ids, fens)
        })
    };
    let Some((mainline_ids, fens)) = extraction else {
        *state.game_report_running.lock().unwrap() = false;
        return Err("onglet introuvable".to_string());
    };
    if mainline_ids.len() < 2 {
        *state.game_report_running.lock().unwrap() = false;
        return Err("aucun coup à analyser".to_string());
    }

    std::thread::spawn(move || {
        let progress_event = format!("game-report-progress-{tab_id}");
        let done_event = format!("game-report-done-{tab_id}");
        let error_event = format!("game-report-error-{tab_id}");
        let total = fens.len();
        let progress_app = app.clone();

        let outcome = engine::analyze_positions(&engine_path, &fens, depth, threads, |done| {
            let _ = progress_app.emit(&progress_event, GameReportProgress { done, total });
        });

        let app_state = app.state::<AppState>();
        let summary = outcome.and_then(|evals| {
            let mut sessions = app_state.sessions.lock().unwrap();
            let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;
            Ok(apply_game_report(&mut session.tree, &mainline_ids, &fens, &evals))
        });

        let _ = match &summary {
            Ok(s) => app.emit(&done_event, s),
            Err(e) => app.emit(&error_event, e),
        };
        *app_state.game_report_running.lock().unwrap() = false;
    });

    Ok(())
}

// L'explorateur (stats, liste de parties, aperçu) s'appuie toujours sur la
// base marquee "reference", pas sur la base d'origine de la partie affichee
// (qui peut tres bien ne pas en avoir une, ex. une partie ouverte depuis un
// fichier PGN). Rouvre/reutilise la connexion mise en cache dans
// state.reference_conn selon que la base de reference a change ou non.
fn ensure_reference_conn(app: &AppHandle, state: &State<AppState>) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let reference = bases::list_bases(&dir)
        .into_iter()
        .find(|b| b.is_reference && !b.missing)
        .ok_or_else(|| "aucune base de référence définie (Accueil > clic sur une base > Marquer comme référence)".to_string())?;

    let mut conn_cache = state.reference_conn.lock().unwrap();
    let needs_reopen = !matches!(conn_cache.as_ref(), Some((path, _)) if *path == reference.path);
    if needs_reopen {
        let conn = db::open(Path::new(&reference.path))?;
        maybe_start_check_suffix_cleanup(app, state, &conn, &reference.path);
        *conn_cache = Some((reference.path.clone(), conn));
        state.explorer_cache.lock().unwrap().clear();
    }
    Ok(())
}

/// Nettoie en tache de fond le "+"/"#" laisse par un import en masse anterieur
/// a la correction de bulk_import (voir db::strip_check_suffixes) -- sans
/// quoi un coup donnant echec/mat reste introuvable par l'explorateur
/// d'ouvertures une fois rejoue. Verifie a chaque (re)ouverture de la base de
/// reference (donc rarement, pas a chaque coup), sur la connexion qu'on vient
/// d'ouvrir -- lecture ponctuelle bon marche (un seul SELECT indexe). Tourne
/// ensuite sur SA PROPRE connexion, separee de celle mise en cache dans
/// reference_conn : sur une base de plusieurs millions de parties ce
/// nettoyage peut prendre plusieurs minutes, et ne doit jamais retenir le
/// verrou partage avec les commandes de l'explorateur (c'est precisement ce
/// qui geleait l'explorateur dans une premiere version de ce correctif, ou le
/// nettoyage tournait de facon synchrone dans db::open lui-meme).
fn maybe_start_check_suffix_cleanup(app: &AppHandle, state: &State<AppState>, conn: &Connection, path: &str) {
    if db::already_check_suffixes_cleaned(conn).unwrap_or(true) {
        return;
    }
    let mut running = state.check_suffixes_cleanup_running.lock().unwrap();
    if *running {
        return;
    }
    *running = true;
    drop(running);

    let app = app.clone();
    let path = path.to_string();
    std::thread::spawn(move || {
        let result = db::open(Path::new(&path)).and_then(|mut cleanup_conn| {
            let progress_app = app.clone();
            db::strip_check_suffixes(
                &mut cleanup_conn,
                move |count| {
                    let _ = progress_app.emit("check-suffixes-progress", count);
                },
                || false,
            )
        });
        match result {
            Ok(count) => {
                let _ = app.emit("check-suffixes-done", count);
            }
            Err(e) => {
                let _ = app.emit("check-suffixes-error", e);
            }
        }
        *app.state::<AppState>().check_suffixes_cleanup_running.lock().unwrap() = false;
    });
}

/// Filtre de preparation d'adversaire actif sur cet onglet, le cas echeant
/// (voir GameSession::explorer_player_filter / open_player_prep_tab).
fn player_filter_for_tab(state: &State<AppState>, tab_id: u32) -> Option<explorer::PlayerFilter> {
    state.sessions.lock().unwrap().get(&tab_id).and_then(|s| s.explorer_player_filter.clone())
}

/// Combine le filtre joueur de preparation d'adversaire (propre a l'onglet,
/// verrouille des l'ouverture -- voir player_filter_for_tab) avec les
/// filtres ad-hoc du panneau explorateur (reglables librement par
/// l'utilisateur, envoyes a chaque appel plutot que stockes cote session --
/// pas besoin d'une commande separee pour les modifier). Le filtre joueur de
/// preparation, s'il existe, l'emporte : un onglet de preparation ne montre
/// pas les controles de filtre joueur ad-hoc, les deux ne devraient donc
/// jamais etre actifs simultanement en pratique.
fn build_explorer_filters(
    state: &State<AppState>,
    tab_id: u32,
    player_name: Option<String>,
    player_white: Option<bool>,
    rating_min: Option<i32>,
    rating_max: Option<i32>,
    year_min: Option<i32>,
    year_max: Option<i32>,
) -> explorer::ExplorerFilters {
    let player = player_filter_for_tab(state, tab_id).or_else(|| {
        let name = player_name?;
        if name.trim().is_empty() {
            return None;
        }
        Some(explorer::PlayerFilter { name, white: player_white.unwrap_or(true), exact: false })
    });
    let rating_range = match (rating_min, rating_max) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    };
    let year_range = match (year_min, year_max) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    };
    explorer::ExplorerFilters { player, rating_range, year_range }
}

/// Nom/code ECO de la position atteinte par `moves`, pour l'en-tete du
/// panneau explorateur -- ne depend d'aucune base (referentiel embarque dans
/// le binaire, voir eco.rs), donc pas besoin de ensure_reference_conn ici.
#[tauri::command]
fn classify_opening(moves: Vec<String>) -> Option<eco::EcoInfo> {
    eco::classify(&moves)
}

/// Delai maximum d'une requete de l'explorateur sur la base de reference (voir
/// explorer::with_time_limit).
const EXPLORER_QUERY_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn opening_explorer(
    app: AppHandle,
    state: State<AppState>,
    tab_id: u32,
    moves: Vec<String>,
    start_fen: Option<String>,
    player_name: Option<String>,
    player_white: Option<bool>,
    rating_min: Option<i32>,
    rating_max: Option<i32>,
    year_min: Option<i32>,
    year_max: Option<i32>,
) -> Result<Vec<OpeningMoveStat>, String> {
    ensure_reference_conn(&app, &state)?;
    let filters = build_explorer_filters(&state, tab_id, player_name, player_white, rating_min, rating_max, year_min, year_max);
    // Normalise ici (pas cote frontend) : le frontend envoie systematiquement
    // le FEN racine de l'onglet affiche, standard ou non -- seul le backend a
    // besoin de savoir que la position standard vaut None (voir
    // db::normalize_start_fen).
    let start_fen = db::normalize_start_fen(start_fen.as_deref());

    // le cache partage (explorer_cache) ne couvre que la vue non filtree, a
    // depart standard, commune a tous les onglets ; des que le moindre filtre
    // (preparation ou ad-hoc) est actif OU que la partie affichee ne demarre
    // pas de la position standard (Nouvelle position, Chess960), la reponse
    // est propre a cette combinaison precise et reste rapide sans avoir
    // besoin d'etre mise en cache (voir explorer::ExplorerFilters).
    if filters.is_empty() && start_fen.is_none() {
        let key = moves.join(" ");
        if let Some(cached) = state.explorer_cache.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let conn_cache = state.reference_conn.lock().unwrap();
        let conn = &conn_cache.as_ref().unwrap().1;
        let stats = explorer::with_time_limit(conn, EXPLORER_QUERY_LIMIT, || explorer::explore(conn, &moves, &filters, None))?;
        state.explorer_cache.lock().unwrap().insert(key, stats.clone());
        return Ok(stats);
    }
    let conn_cache = state.reference_conn.lock().unwrap();
    let conn = &conn_cache.as_ref().unwrap().1;
    explorer::with_time_limit(conn, EXPLORER_QUERY_LIMIT, || explorer::explore(conn, &moves, &filters, start_fen.as_deref()))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ExplorerGamesPage {
    games: Vec<db::ExplorerGameSummary>,
    total: u32,
    base_path: String,
}

/// Parties de la base de reference passant par la position atteinte par
/// `moves`, paginees. Complement de `opening_explorer` (qui ne donne que les
/// stats agregees par coup suivant) pour le panneau "parties dans cette
/// position".
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn opening_explorer_games(
    app: AppHandle,
    state: State<AppState>,
    tab_id: u32,
    moves: Vec<String>,
    start_fen: Option<String>,
    offset: u32,
    limit: u32,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    player_name: Option<String>,
    player_white: Option<bool>,
    rating_min: Option<i32>,
    rating_max: Option<i32>,
    year_min: Option<i32>,
    year_max: Option<i32>,
) -> Result<ExplorerGamesPage, String> {
    ensure_reference_conn(&app, &state)?;
    let filters = build_explorer_filters(&state, tab_id, player_name, player_white, rating_min, rating_max, year_min, year_max);
    // Voir opening_explorer : normalise ici, le frontend envoie toujours le
    // FEN racine de l'onglet affiche, standard ou non.
    let start_fen = db::normalize_start_fen(start_fen.as_deref());
    let conn_cache = state.reference_conn.lock().unwrap();
    let (base_path, conn) = conn_cache.as_ref().unwrap();
    // Aucune position (coup illegal depuis ce depart, ex. venant de l'editeur
    // de position -- voir game_tree::plays_and_queries_an_en_passant_capture_from_a_custom_position) :
    // aucune partie ne peut y correspondre, pas d'appel SQL a faire.
    let Some(zobrist) = explorer::current_zobrist(&moves, start_fen.as_deref()) else {
        return Ok(ExplorerGamesPage { games: Vec::new(), total: 0, base_path: base_path.clone() });
    };
    let (total, games) = explorer::with_time_limit(conn, EXPLORER_QUERY_LIMIT, || {
        let total = db::count_at_zobrist(conn, zobrist, &filters)?;
        let games = db::games_at_zobrist(
            conn,
            zobrist,
            &filters,
            sort_by.as_deref().unwrap_or(""),
            sort_dir.as_deref().unwrap_or(""),
            limit,
            offset,
        )?;
        Ok((total, games))
    })?;
    Ok(ExplorerGamesPage { games, total, base_path: base_path.clone() })
}

/// Charge une partie de la base de reference sous forme d'arbre, en lecture
/// seule, sans creer de session/onglet -- utilise pour le petit echiquier de
/// balayage dans le panneau "parties dans cette position".
#[tauri::command]
fn opening_explorer_preview_game(app: AppHandle, state: State<AppState>, id: i64) -> Result<TreeDto, String> {
    ensure_reference_conn(&app, &state)?;
    let conn_cache = state.reference_conn.lock().unwrap();
    let conn = &conn_cache.as_ref().unwrap().1;
    let pgn_text = db::load_game_pgn(conn, id)?.ok_or_else(|| "partie introuvable".to_string())?;
    let tree = pgn::parse_game(&pgn_text)?;
    Ok(tree.to_dto())
}

#[tauri::command]
fn list_bases(app: AppHandle) -> Result<Vec<BaseInfo>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(bases::list_bases(&dir))
}

#[tauri::command]
fn create_base(app: AppHandle, name: String, path: String) -> Result<BaseInfo, String> {
    db::open(Path::new(&path))?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    bases::register_base(&dir, &name, &path)?;
    Ok(BaseInfo { name, path, games: 0, is_reference: false, missing: false })
}

#[tauri::command]
fn set_base_reference(app: AppHandle, path: String, is_reference: bool) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    bases::set_reference(&dir, &path, is_reference)
}

#[tauri::command]
fn delete_base(app: AppHandle, path: String, delete_file: bool) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    bases::unregister(&dir, &path)?;
    if delete_file && Path::new(&path).exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn add_existing_base(app: AppHandle, path: String) -> Result<BaseInfo, String> {
    let conn = db::open(Path::new(&path))?;
    let name = Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Base")
        .to_string();
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    bases::register_base(&dir, &name, &path)?;
    let games = db::count_games(&conn, &db::GamesFilter::default())?;
    Ok(BaseInfo { name, path, games, is_reference: false, missing: false })
}

/// Performance Elo (methode FIDE) du joueur le plus present dans les parties
/// selectionnees du panneau Parties ; None s'il n'y a rien d'exploitable.
#[tauri::command]
fn games_performance(path: String, ids: Vec<i64>) -> Result<Option<performance::Performance>, String> {
    let conn = db::open(Path::new(&path))?;
    let games = db::load_games_for_performance(&conn, &ids)?;
    Ok(performance::compute(&games))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GamesPage {
    games: Vec<GameSummary>,
    total: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GamesFilterDto {
    #[serde(default)]
    search: String,
    #[serde(default)]
    white: String,
    #[serde(default)]
    black: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    date_from: String,
    #[serde(default)]
    date_to: String,
    #[serde(default)]
    time_control: String,
}

impl From<GamesFilterDto> for db::GamesFilter {
    fn from(dto: GamesFilterDto) -> Self {
        db::GamesFilter {
            search: dto.search,
            white: dto.white,
            black: dto.black,
            result: dto.result,
            date_from: dto.date_from,
            date_to: dto.date_to,
            time_control: dto.time_control,
        }
    }
}

#[tauri::command]
fn list_games_in_base(
    path: String,
    filter: GamesFilterDto,
    sort_by: String,
    sort_dir: String,
    offset: u32,
    limit: u32,
) -> Result<GamesPage, String> {
    let conn = db::open(Path::new(&path))?;
    let filter: db::GamesFilter = filter.into();
    let total = db::count_games(&conn, &filter)?;
    let games = db::list_games_page(&conn, &filter, &sort_by, &sort_dir, limit, offset)?;
    Ok(GamesPage { games, total })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayersPage {
    players: Vec<db::PlayerSummary>,
    total: u32,
}

#[tauri::command]
fn list_players_in_base(
    path: String,
    search: String,
    sort_by: String,
    sort_dir: String,
    offset: u32,
    limit: u32,
) -> Result<PlayersPage, String> {
    let conn = db::open(Path::new(&path))?;
    let total = db::count_players(&conn, &search)?;
    let players = db::list_players_page(&conn, &search, &sort_by, &sort_dir, limit, offset)?;
    Ok(PlayersPage { players, total })
}

#[tauri::command]
fn delete_game(state: State<AppState>, path: String, id: i64) -> Result<(), String> {
    let conn = db::open(Path::new(&path))?;
    db::delete_game(&conn, id)?;
    let mut sessions = state.sessions.lock().unwrap();
    for session in sessions.values_mut() {
        if session.active_base_path.as_deref() == Some(path.as_str()) && session.active_game_id == Some(id) {
            session.active_game_id = None;
        }
    }
    Ok(())
}

#[tauri::command]
fn move_game(
    state: State<AppState>,
    from_path: String,
    to_path: String,
    id: i64,
    copy: bool,
    force: bool,
) -> Result<db::SaveResult, String> {
    let source = db::open(Path::new(&from_path))?;
    let target = db::open(Path::new(&to_path))?;

    let (tags, pgn_text, mainline_sans) = db::load_game_for_move(&source, id)?
        .ok_or_else(|| "partie introuvable".to_string())?;
    let moves: Vec<String> = mainline_sans.split_whitespace().map(|s| s.to_string()).collect();

    if !force {
        if let Some(existing) = db::find_duplicate(&target, &tags, &moves)? {
            return Ok(db::SaveResult::Duplicate { existing });
        }
    }

    let new_id = db::save_game(&target, &pgn_text, &tags, &mainline_sans)?;
    if !copy {
        db::delete_game(&source, id)?;

        let mut sessions = state.sessions.lock().unwrap();
        for session in sessions.values_mut() {
            if session.active_base_path.as_deref() == Some(from_path.as_str()) && session.active_game_id == Some(id) {
                session.active_game_id = None;
            }
        }
    }
    Ok(db::SaveResult::Saved { id: new_id })
}

#[tauri::command]
fn open_game(state: State<AppState>, path: String, id: i64) -> Result<TabDto, String> {
    let conn = db::open(Path::new(&path))?;
    let pgn_text = db::load_game_pgn(&conn, id)?.ok_or_else(|| "partie introuvable".to_string())?;
    let parsed = pgn::parse_game(&pgn_text)?;
    let mut session = GameSession::new();
    session.tree = parsed;
    session.db = Some(conn);
    session.active_base_path = Some(path);
    session.active_game_id = Some(id);
    let tab_id = allocate_tab_id(&state);
    let tree = insert_session(&state, tab_id, session);
    Ok(TabDto { tab_id, tree, is_player_prep: false })
}

#[tauri::command]
fn save_game_to(state: State<AppState>, tab_id: u32, path: String, force: bool) -> Result<db::SaveResult, String> {
    let conn = db::open(Path::new(&path))?;
    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions.get_mut(&tab_id).ok_or_else(|| "onglet introuvable".to_string())?;

    let moves = session.tree.mainline_sans();
    let moves_str = moves.join(" ");

    if session.active_base_path.as_deref() == Some(path.as_str()) {
        if let Some(id) = session.active_game_id {
            let pgn_text = pgn::write_game(&session.tree);
            db::update_game(&conn, id, &pgn_text, &session.tree.tags, &moves_str)?;
            session.db = Some(conn);
            return Ok(db::SaveResult::Saved { id });
        }
    }

    if !force {
        if let Some(existing) = db::find_duplicate(&conn, &session.tree.tags, &moves)? {
            return Ok(db::SaveResult::Duplicate { existing });
        }
    }
    let pgn_text = pgn::write_game(&session.tree);
    let id = db::save_game(&conn, &pgn_text, &session.tree.tags, &moves_str)?;
    session.db = Some(conn);
    session.active_base_path = Some(path);
    session.active_game_id = Some(id);
    Ok(db::SaveResult::Saved { id })
}

/// Construit un menu natif complet, independant (jamais partage entre deux
/// fenetres : chaque fenetre doit appeler cette fonction pour obtenir son
/// propre objet Menu -- reutiliser le meme objet sur deux fenetres a deja
/// fait planter l'appli sous Windows). `home_panels`/`game_panels`
/// determinent quelles cases du menu Affichage sont activees selon le type
/// de fenetre (Accueil ou Partie).
fn build_menu(app: &AppHandle, home_panels: bool, game_panels: bool, lang: &str, window_label: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let t = i18n::translations(lang);
    let t = |key: &str| t.get(key).cloned().unwrap_or_else(|| key.to_string());

    let new_game_item = MenuItemBuilder::with_id("new_game", t("menu.file.newGame"))
        .accelerator("CmdOrCtrl+N")
        .build(app)?;
    let new_position_item = MenuItemBuilder::with_id("new_position", t("menu.file.newPosition"))
        .accelerator("CmdOrCtrl+P")
        .build(app)?;
    let import_item = MenuItemBuilder::with_id("import_pgn", t("menu.file.openPgn"))
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    // Sous-menu "Importer" (fichier PGN local vers une base, ou compte en
    // ligne Lichess/chess.com) : les trois amenent au meme flux de choix de
    // base + barre de progression cote frontend (voir HomeWindow.tsx), seule
    // la source des parties differe.
    let import_pgn_bulk_item = MenuItemBuilder::with_id("bulk_import_pgn", t("menu.file.import.pgn"))
        .accelerator("CmdOrCtrl+I")
        .build(app)?;
    let import_lichess_item = MenuItemBuilder::with_id("import_lichess", t("menu.file.import.lichess")).build(app)?;
    let import_chesscom_item = MenuItemBuilder::with_id("import_chesscom", t("menu.file.import.chesscom")).build(app)?;
    let import_submenu = SubmenuBuilder::new(app, t("menu.file.import"))
        .item(&import_pgn_bulk_item)
        .item(&import_lichess_item)
        .item(&import_chesscom_item)
        .build()?;
    let export_item = MenuItemBuilder::with_id("export_pgn", t("menu.file.exportPgn"))
        .accelerator("CmdOrCtrl+E")
        .build(app)?;
    let save_item = MenuItemBuilder::with_id("save_game", t("menu.file.save"))
        .accelerator("CmdOrCtrl+S")
        .build(app)?;
    let file_menu = SubmenuBuilder::new(app, t("menu.file"))
        .item(&new_game_item)
        .item(&new_position_item)
        .separator()
        .item(&import_item)
        .item(&import_submenu)
        .item(&export_item)
        .separator()
        .item(&save_item)
        .build()?;

    // Operations de maintenance sur une base (menu Outils) : demandent
    // toutes de choisir la base ciblee (voir HomeWindow.tsx, meme sequence
    // que File > Importer une base), independamment de l'onglet/fenetre
    // courant -- pas besoin d'un etat home_panels/game_panels comme pour les
    // cases a cocher Affichage.
    let rebuild_explorer_stats_item =
        MenuItemBuilder::with_id("rebuild_explorer_stats", t("menu.tools.rebuildExplorerStats")).build(app)?;
    let deduplicate_base_item = MenuItemBuilder::with_id("deduplicate_base", t("menu.tools.deduplicateBase")).build(app)?;
    let backfill_fide_ids_item = MenuItemBuilder::with_id("backfill_fide_ids", t("menu.tools.backfillFideIds")).build(app)?;
    let tools_menu = SubmenuBuilder::new(app, t("menu.tools"))
        .item(&rebuild_explorer_stats_item)
        .item(&deduplicate_base_item)
        .item(&backfill_fide_ids_item)
        .build()?;

    let panel_bases = CheckMenuItemBuilder::with_id("panel_bases", t("menu.display.bases"))
        .enabled(home_panels)
        .checked(home_panels)
        .build(app)?;
    let panel_pgn = CheckMenuItemBuilder::with_id("panel_pgn", t("menu.display.pgn"))
        .enabled(home_panels)
        .checked(home_panels)
        .build(app)?;
    let panel_parties = CheckMenuItemBuilder::with_id("panel_parties", t("menu.display.games"))
        .enabled(home_panels)
        .checked(home_panels)
        .build(app)?;
    let panel_players = CheckMenuItemBuilder::with_id("panel_players", t("menu.display.players"))
        .enabled(home_panels)
        .checked(home_panels)
        .build(app)?;
    let panel_fide = CheckMenuItemBuilder::with_id("panel_fide", t("menu.display.fide"))
        .enabled(home_panels)
        .checked(home_panels)
        .build(app)?;
    let panel_board = CheckMenuItemBuilder::with_id("panel_board", t("menu.display.board"))
        .enabled(game_panels)
        .checked(game_panels)
        .build(app)?;
    let panel_notation = CheckMenuItemBuilder::with_id("panel_notation", t("menu.display.notation"))
        .enabled(game_panels)
        .checked(game_panels)
        .build(app)?;
    let panel_explorer = CheckMenuItemBuilder::with_id("panel_explorer", t("menu.display.explorer"))
        .enabled(game_panels)
        .checked(game_panels)
        .build(app)?;
    let panel_engine = CheckMenuItemBuilder::with_id("panel_engine", t("menu.display.engine"))
        .enabled(game_panels)
        .checked(false)
        .build(app)?;
    let display_menu = SubmenuBuilder::new(app, t("menu.display"))
        .item(&panel_bases)
        .item(&panel_pgn)
        .item(&panel_parties)
        .item(&panel_players)
        .item(&panel_fide)
        .separator()
        .item(&panel_board)
        .item(&panel_notation)
        .item(&panel_explorer)
        .item(&panel_engine)
        .build()?;

    // conserve les poignees des cases a cocher pour pouvoir les mettre a
    // jour individuellement ensuite (voir set_panel_checked) sans
    // reconstruire tout le menu.
    {
        let app_state = app.state::<AppState>();
        let mut items = app_state.panel_menu_items.lock().unwrap();
        for (id, item) in [
            ("panel_bases", panel_bases),
            ("panel_pgn", panel_pgn),
            ("panel_parties", panel_parties),
            ("panel_players", panel_players),
            ("panel_fide", panel_fide),
            ("panel_board", panel_board),
            ("panel_notation", panel_notation),
            ("panel_explorer", panel_explorer),
            ("panel_engine", panel_engine),
        ] {
            items.insert(format!("{window_label}:{id}"), item);
        }
    }

    let board_theme_brown = MenuItemBuilder::with_id("board-theme-brown", t("menu.preferences.boardColors.brown")).build(app)?;
    let board_theme_blue = MenuItemBuilder::with_id("board-theme-blue", t("menu.preferences.boardColors.blue")).build(app)?;
    let board_theme_green = MenuItemBuilder::with_id("board-theme-green", t("menu.preferences.boardColors.green")).build(app)?;
    let board_theme_grey = MenuItemBuilder::with_id("board-theme-grey", t("menu.preferences.boardColors.grey")).build(app)?;
    let board_theme_wood = MenuItemBuilder::with_id("board-theme-wood", t("menu.preferences.boardColors.wood")).build(app)?;
    let board_colors_menu = SubmenuBuilder::new(app, t("menu.preferences.boardColors"))
        .item(&board_theme_brown)
        .item(&board_theme_blue)
        .item(&board_theme_green)
        .item(&board_theme_grey)
        .item(&board_theme_wood)
        .build()?;
    // Pas d'option "Systeme" ici : le systeme ne sert que de valeur par
    // defaut avant tout choix (voir colorScheme.ts, loadStoredColorScheme
    // retombe sur "system" tant que rien n'est enregistre) -- une fois que
    // l'utilisateur a choisi Clair ou Sombre, ce choix reste, sans option
    // pour "redevenir" suiveur du systeme.
    let color_scheme_light =
        MenuItemBuilder::with_id("color-scheme-light", t("menu.preferences.appearance.light")).build(app)?;
    let color_scheme_dark =
        MenuItemBuilder::with_id("color-scheme-dark", t("menu.preferences.appearance.dark")).build(app)?;
    let appearance_menu = SubmenuBuilder::new(app, t("menu.preferences.appearance"))
        .item(&color_scheme_light)
        .item(&color_scheme_dark)
        .build()?;
    let lang_fr_item = CheckMenuItemBuilder::with_id("lang-fr", t("menu.preferences.language.fr"))
        .checked(lang != "en")
        .build(app)?;
    let lang_en_item = CheckMenuItemBuilder::with_id("lang-en", t("menu.preferences.language.en"))
        .checked(lang == "en")
        .build(app)?;
    let language_menu = SubmenuBuilder::new(app, t("menu.preferences.language"))
        .item(&lang_fr_item)
        .item(&lang_en_item)
        .build()?;
    let preferences_menu = SubmenuBuilder::new(app, t("menu.preferences"))
        .item(&board_colors_menu)
        .item(&appearance_menu)
        .separator()
        .item(&language_menu)
        .build()?;

    let about_item = MenuItemBuilder::with_id("about", t("menu.help.about")).accelerator("F1").build(app)?;
    let help_menu = SubmenuBuilder::new(app, t("menu.help")).item(&about_item).build()?;

    Menu::with_items(app, &[&file_menu, &display_menu, &tools_menu, &preferences_menu, &help_menu])
}

/// Reconstruit le menu dans la langue donnee et le rattache a la fenetre
/// principale et, si elle est ouverte, a la fenetre Partie.
fn rebuild_and_attach_menu(app: &AppHandle, lang: &str) -> Result<(), String> {
    let menu = build_menu(app, true, true, lang, "main").map_err(|e| e.to_string())?;
    app.set_menu(menu).map_err(|e| e.to_string())?;
    if let Some(game_window) = app.get_webview_window("game") {
        let game_menu = build_menu(app, true, true, lang, "game").map_err(|e| e.to_string())?;
        game_window.set_menu(game_menu).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// La fenetre "game" est creee cote JS (voir openTabInGameWindow) et n'herite
// pas automatiquement du menu partage defini au demarrage : on le rattache
// explicitement une fois la fenetre montee. Contrairement a la creation de
// fenetre depuis une commande (qui plante de facon fiable, voir les
// tentatives precedentes), attacher un menu a une fenetre DEJA existante ne
// pose pas ce probleme.
#[tauri::command]
fn attach_game_menu(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("game")
        .ok_or_else(|| "fenêtre game introuvable".to_string())?;
    let lang = state.language.lock().unwrap().clone();
    let menu = build_menu(&app, true, true, &lang, "game").map_err(|e| e.to_string())?;
    window.set_menu(menu).map_err(|e| e.to_string())?;
    Ok(())
}

/// Petites fenetres utilitaires (Aide, editeur de position, choix de partie
/// PGN) : pas de menu natif souhaite (pas d'onglets/panneaux a piloter
/// depuis un menu, juste une boite de dialogue simple). Cote Windows, une
/// fenetre creee sans menu explicite peut malgre tout heriter du menu
/// partage de l'appli (voir app.set_menu dans setup) -- de facon
/// intermittente, rapporte par l'utilisateur (present a la premiere
/// ouverture de la fenetre Aide via F1, absent aux suivantes) plutot que
/// systematique, ce qui pointe vers une histoire de timing cote Tauri/WebView2
/// plutot qu'un comportement garanti dans un sens ou l'autre -- on force donc
/// explicitement son retrait plutot que de compter sur son absence par
/// defaut. Sans effet (pas d'erreur) si la fenetre n'en avait deja pas.
#[tauri::command]
fn remove_window_menu(window: tauri::Window) -> Result<(), String> {
    window.remove_menu().map_err(|e| e.to_string())?;
    Ok(())
}

/// Met a jour une case a cocher du menu Affichage sans reconstruire tout le
/// menu, pour qu'elle reflete l'etat reel d'un panneau -- y compris quand il
/// a ete ferme via son propre bouton plutot que via ce menu (voir
/// panel_menu_items). `window` est injecte par Tauri d'apres la fenetre
/// appelante : chaque fenetre (Accueil, Partie) ne peut mettre a jour que ses
/// propres cases.
#[tauri::command]
fn set_panel_checked(window: tauri::Window, state: State<AppState>, id: String, checked: bool) -> Result<(), String> {
    let key = format!("{}:{}", window.label(), id);
    if let Some(item) = state.panel_menu_items.lock().unwrap().get(&key) {
        item.set_checked(checked).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_language(state: State<AppState>) -> String {
    state.language.lock().unwrap().clone()
}

#[tauri::command]
fn set_language(app: AppHandle, lang: String) -> Result<(), String> {
    apply_language_change(&app, &lang)
}

/// Change la langue courante : persiste le choix, reconstruit et rattache le
/// menu natif a toutes les fenetres ouvertes, previent le cote React (menu
/// natif et interface React doivent rester synchronises quelle que soit la
/// source du changement).
fn apply_language_change(app: &AppHandle, lang: &str) -> Result<(), String> {
    let lang = if lang == "en" { "en".to_string() } else { "fr".to_string() };
    *app.state::<AppState>().language.lock().unwrap() = lang.clone();
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = i18n::save_language(&dir, &lang);
    }
    rebuild_and_attach_menu(app, &lang)?;
    let _ = app.emit("language-changed", &lang);
    Ok(())
}

// WebView2 traite par defaut certains raccourcis clavier comme des
// "accelerateurs navigateur" (Ctrl+P imprime, Ctrl+N/O/F/G/R... sont avales
// silencieusement) AVANT meme qu'ils atteignent le tableau d'accelerateurs du
// menu natif Tauri -- rapporte par l'utilisateur : Ctrl+N/O/I/E (voir
// build_menu) ne declenchaient rien du tout, Ctrl+P ouvrait l'impression
// Windows a la place. Seule accroche disponible cote Tauri pour toucher aux
// reglages bruts de WebView2 (pas de config declarative pour ca) : on_page_load,
// via PlatformWebview::controller() -> COM WebView2 direct. Appele a chaque
// chargement de page de chaque fenetre (idempotent, cout negligeable).
#[cfg(windows)]
fn disable_browser_accelerator_keys(webview: &tauri::Webview<tauri::Wry>) {
    let _ = webview.with_webview(|platform_webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
        use windows_core::Interface;
        unsafe {
            let Ok(core_webview) = platform_webview.controller().CoreWebView2() else { return };
            let Ok(settings) = core_webview.Settings() else { return };
            let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() else { return };
            let _ = settings3.SetAreBrowserAcceleratorKeysEnabled(false);
        }
    });
}

#[cfg(not(windows))]
fn disable_browser_accelerator_keys(_webview: &tauri::Webview<tauri::Wry>) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_page_load(|webview, _payload| disable_browser_accelerator_keys(webview))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            // migration : si une ancienne base par defaut existe mais qu'aucun
            // registre de bases n'a encore ete cree, on l'enregistre pour ne
            // pas perdre l'acces aux parties deja sauvegardees
            if !bases::registry_exists(&data_dir) {
                let legacy = data_dir.join("chesscraft.sqlite");
                if legacy.exists() {
                    let _ = bases::register_base(
                        &data_dir,
                        "Ma base",
                        &legacy.to_string_lossy(),
                    );
                }
            }

            let initial_lang = app
                .path()
                .app_data_dir()
                .map(|dir| i18n::load_language(&dir))
                .unwrap_or_else(|_| "fr".to_string());

            app.manage(AppState {
                sessions: Mutex::new(HashMap::new()),
                next_tab_id: Mutex::new(0),
                bulk_import_cancel: Mutex::new(false),
                bulk_import_running: Mutex::new(false),
                fide_refresh_running: Mutex::new(false),
                check_suffixes_cleanup_running: Mutex::new(false),
                tool_job_running: Mutex::new(false),
                game_report_running: Mutex::new(false),
                reference_conn: Mutex::new(None),
                explorer_cache: Mutex::new(HashMap::new()),
                language: Mutex::new(initial_lang.clone()),
                panel_menu_items: Mutex::new(HashMap::new()),
            });

            // menu unique partage par toutes les fenetres (voir open_game_window) :
            // toutes les cases Affichage restent activees, chaque fenetre ignore
            // simplement cote JS les bascules qui ne la concernent pas.
            let shared_menu = build_menu(&app.handle(), true, true, &initial_lang, "main")?;
            app.set_menu(shared_menu)?;

            app.on_menu_event(move |app_handle, event| {
                let id = event.id().as_ref();
                if let Some(theme) = id.strip_prefix("board-theme-") {
                    let _ = app_handle.emit("menu-set-board-theme", theme);
                    return;
                }
                if let Some(scheme) = id.strip_prefix("color-scheme-") {
                    let _ = app_handle.emit("menu-set-color-scheme", scheme);
                    return;
                }
                if let Some(lang) = id.strip_prefix("lang-") {
                    let _ = apply_language_change(app_handle, lang);
                    return;
                }
                let event_name = match id {
                    "new_game" => "menu-new-game",
                    "new_position" => "menu-new-position",
                    "import_pgn" => "menu-import-pgn",
                    "bulk_import_pgn" => "menu-bulk-import-pgn",
                    "import_lichess" => "menu-import-lichess",
                    "import_chesscom" => "menu-import-chesscom",
                    "rebuild_explorer_stats" => "menu-rebuild-explorer-stats",
                    "deduplicate_base" => "menu-deduplicate-base",
                    "backfill_fide_ids" => "menu-backfill-fide-ids",
                    "export_pgn" => "menu-export-pgn",
                    "save_game" => "menu-save-game",
                    "about" => "menu-show-about",
                    "panel_bases" => "menu-toggle-panel-bases",
                    "panel_pgn" => "menu-toggle-panel-pgn",
                    "panel_parties" => "menu-toggle-panel-parties",
                    "panel_players" => "menu-toggle-panel-players",
                    "panel_fide" => "menu-toggle-panel-fide",
                    "panel_board" => "menu-toggle-panel-board",
                    "panel_notation" => "menu-toggle-panel-notation",
                    "panel_explorer" => "menu-toggle-panel-explorer",
                    "panel_engine" => "menu-toggle-panel-engine",
                    _ => return,
                };
                let _ = app_handle.emit(event_name, ());
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            attach_game_menu,
            remove_window_menu,
            set_panel_checked,
            get_language,
            set_language,
            new_game,
            new_game_from_fen,
            new_chess960_game,
            refresh_fide_players,
            get_fide_status,
            list_fide_players,
            open_player_prep_tab,
            get_active_base_path,
            close_tab,
            get_tree,
            legal_moves,
            play_move,
            set_comment,
            set_nags,
            promote_variation,
            delete_variation,
            set_shapes,
            set_tags,
            list_pgn_games,
            import_pgn,
            bulk_import_pgn,
            import_lichess_games,
            import_chesscom_games,
            get_lichess_perf_types,
            cancel_bulk_import,
            rebuild_explorer_stats,
            count_duplicate_games,
            deduplicate_base,
            backfill_fide_ids,
            export_pgn,
            start_engine,
            stop_engine,
            analyze_position,
            start_game_report,
            get_game_report,
            pause_engine,
            classify_opening,
            opening_explorer,
            opening_explorer_games,
            opening_explorer_preview_game,
            logical_cpu_count,
            list_bases,
            create_base,
            add_existing_base,
            list_games_in_base,
            games_performance,
            list_players_in_base,
            open_game,
            save_game_to,
            list_recent_pgns,
            set_base_reference,
            delete_base,
            delete_game,
            move_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
