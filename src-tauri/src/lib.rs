mod bases;
mod bulk_import;
mod db;
mod engine;
mod explorer;
mod game_tree;
mod i18n;
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
}

impl GameSession {
    fn new() -> Self {
        GameSession {
            tree: GameTree::new(),
            db: None,
            active_base_path: None,
            active_game_id: None,
            engine: None,
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
}


#[tauri::command]
fn new_game(state: State<AppState>) -> TabDto {
    let tab_id = allocate_tab_id(&state);
    let tree = insert_session(&state, tab_id, GameSession::new());
    TabDto { tab_id, tree }
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
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let games = pgn::list_games(&text)?;
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = recent_pgns::add_recent(&dir, &path);
    }
    Ok(games)
}

#[tauri::command]
fn import_pgn(state: State<AppState>, path: String, index: u32) -> Result<TabDto, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let parsed = pgn::parse_game_at(&text, index)?;
    let mut session = GameSession::new();
    session.tree = parsed;
    let tab_id = allocate_tab_id(&state);
    let tree = insert_session(&state, tab_id, session);
    Ok(TabDto { tab_id, tree })
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

#[tauri::command]
fn bulk_import_pgn(app: AppHandle, state: State<AppState>, db_path: String, pgn_path: String) -> Result<(), String> {
    {
        let mut running = state.bulk_import_running.lock().unwrap();
        if *running {
            return Err("un import en masse est deja en cours".to_string());
        }
        *running = true;
    }
    *state.bulk_import_cancel.lock().unwrap() = false;
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let cancel_app = app.clone();
        let result = (|| -> Result<bulk_import::BulkImportSummary, String> {
            let pgn_text = std::fs::read_to_string(&pgn_path).map_err(|e| e.to_string())?;
            let mut conn = db::open(Path::new(&db_path))?;
            let summary = bulk_import::bulk_import(
                &mut conn,
                &pgn_text,
                move |summary| {
                    let _ = progress_app.emit("bulk-import-progress", summary.clone());
                },
                move || *cancel_app.state::<AppState>().bulk_import_cancel.lock().unwrap(),
            )?;
            // recalcule les stats precalculees de l'explorateur d'ouvertures
            // (voir explorer::rebuild_prefix_stats) : cout marginal a cote de
            // l'import lui-meme, evite de retomber sur le calcul a la volee,
            // lent sur les tout premiers coups, au prochain jeu ouvert.
            // Echec non bloquant : l'explorateur retombe alors sur le calcul
            // a la volee (voir explorer::explore).
            let _ = explorer::rebuild_prefix_stats(&mut conn);
            Ok(summary)
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

#[tauri::command]
fn cancel_bulk_import(state: State<AppState>) {
    *state.bulk_import_cancel.lock().unwrap() = true;
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
        let games = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| pgn::list_games(&text).ok())
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
        None => Err("le moteur n'est pas demarre".to_string()),
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
        None => Err("le moteur n'est pas demarre".to_string()),
    }
}

#[tauri::command]
// L'explorateur s'appuie toujours sur la base marquee "reference", pas sur
// la base d'origine de la partie affichee (qui peut tres bien ne pas en
// avoir une, ex. une partie ouverte depuis un fichier PGN).
fn opening_explorer(app: AppHandle, state: State<AppState>, moves: Vec<String>) -> Result<Vec<OpeningMoveStat>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let reference = bases::list_bases(&dir)
        .into_iter()
        .find(|b| b.is_reference && !b.missing)
        .ok_or_else(|| "aucune base de reference definie (Accueil > clic sur une base > Marquer comme reference)".to_string())?;

    let mut conn_cache = state.reference_conn.lock().unwrap();
    let needs_reopen = !matches!(conn_cache.as_ref(), Some((path, _)) if *path == reference.path);
    if needs_reopen {
        let conn = db::open(Path::new(&reference.path))?;
        *conn_cache = Some((reference.path.clone(), conn));
        state.explorer_cache.lock().unwrap().clear();
    }

    let key = moves.join(" ");
    if let Some(cached) = state.explorer_cache.lock().unwrap().get(&key) {
        return Ok(cached.clone());
    }
    let conn = &conn_cache.as_ref().unwrap().1;
    let stats = explorer::explore(conn, &moves)?;
    state.explorer_cache.lock().unwrap().insert(key, stats.clone());
    Ok(stats)
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
    Ok(TabDto { tab_id, tree })
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
fn build_menu(app: &AppHandle, home_panels: bool, game_panels: bool, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let t = i18n::translations(lang);
    let t = |key: &str| t.get(key).cloned().unwrap_or_else(|| key.to_string());

    let new_game_item = MenuItemBuilder::with_id("new_game", t("menu.file.newGame")).build(app)?;
    let import_item = MenuItemBuilder::with_id("import_pgn", t("menu.file.openPgn")).build(app)?;
    let bulk_import_item = MenuItemBuilder::with_id("bulk_import_pgn", t("menu.file.importPgn")).build(app)?;
    let export_item = MenuItemBuilder::with_id("export_pgn", t("menu.file.exportPgn")).build(app)?;
    let save_item = MenuItemBuilder::with_id("save_game", t("menu.file.save"))
        .accelerator("CmdOrCtrl+S")
        .build(app)?;
    let file_menu = SubmenuBuilder::new(app, t("menu.file"))
        .item(&new_game_item)
        .separator()
        .item(&import_item)
        .item(&bulk_import_item)
        .item(&export_item)
        .separator()
        .item(&save_item)
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
        .separator()
        .item(&panel_notation)
        .item(&panel_explorer)
        .item(&panel_engine)
        .build()?;

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
    let show_bases_paths_item =
        MenuItemBuilder::with_id("show_bases_paths", t("menu.preferences.basesPaths")).build(app)?;
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
        .separator()
        .item(&show_bases_paths_item)
        .separator()
        .item(&language_menu)
        .build()?;

    Menu::with_items(app, &[&file_menu, &display_menu, &preferences_menu])
}

/// Reconstruit le menu dans la langue donnee et le rattache a la fenetre
/// principale et, si elle est ouverte, a la fenetre Partie.
fn rebuild_and_attach_menu(app: &AppHandle, lang: &str) -> Result<(), String> {
    let menu = build_menu(app, true, true, lang).map_err(|e| e.to_string())?;
    app.set_menu(menu).map_err(|e| e.to_string())?;
    if let Some(game_window) = app.get_webview_window("game") {
        let game_menu = build_menu(app, true, true, lang).map_err(|e| e.to_string())?;
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
        .ok_or_else(|| "fenetre game introuvable".to_string())?;
    let lang = state.language.lock().unwrap().clone();
    let menu = build_menu(&app, true, true, &lang).map_err(|e| e.to_string())?;
    window.set_menu(menu).map_err(|e| e.to_string())?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
                reference_conn: Mutex::new(None),
                explorer_cache: Mutex::new(HashMap::new()),
                language: Mutex::new(initial_lang.clone()),
            });

            // menu unique partage par toutes les fenetres (voir open_game_window) :
            // toutes les cases Affichage restent activees, chaque fenetre ignore
            // simplement cote JS les bascules qui ne la concernent pas.
            let shared_menu = build_menu(&app.handle(), true, true, &initial_lang)?;
            app.set_menu(shared_menu)?;

            app.on_menu_event(move |app_handle, event| {
                let id = event.id().as_ref();
                if let Some(theme) = id.strip_prefix("board-theme-") {
                    let _ = app_handle.emit("menu-set-board-theme", theme);
                    return;
                }
                if let Some(lang) = id.strip_prefix("lang-") {
                    let _ = apply_language_change(app_handle, lang);
                    return;
                }
                let event_name = match id {
                    "new_game" => "menu-new-game",
                    "import_pgn" => "menu-import-pgn",
                    "bulk_import_pgn" => "menu-bulk-import-pgn",
                    "export_pgn" => "menu-export-pgn",
                    "save_game" => "menu-save-game",
                    "show_bases_paths" => "menu-show-bases-paths",
                    "panel_bases" => "menu-toggle-panel-bases",
                    "panel_pgn" => "menu-toggle-panel-pgn",
                    "panel_parties" => "menu-toggle-panel-parties",
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
            get_language,
            set_language,
            new_game,
            get_active_base_path,
            close_tab,
            get_tree,
            legal_moves,
            play_move,
            set_comment,
            set_nags,
            set_shapes,
            set_tags,
            list_pgn_games,
            import_pgn,
            bulk_import_pgn,
            cancel_bulk_import,
            export_pgn,
            start_engine,
            stop_engine,
            analyze_position,
            pause_engine,
            opening_explorer,
            logical_cpu_count,
            list_bases,
            create_base,
            add_existing_base,
            list_games_in_base,
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
