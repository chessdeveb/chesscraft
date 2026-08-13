mod db;
mod engine;
mod explorer;
mod game_tree;
mod pgn;

use engine::EngineHandle;
use explorer::OpeningMoveStat;
use game_tree::{GameTree, MoveDto, ShapeDto, TreeDto};
use pgn::PgnGameSummary;
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    tree: Mutex<GameTree>,
    db: Mutex<Connection>,
    engine: Mutex<Option<EngineHandle>>,
}

#[tauri::command]
fn new_game(state: State<AppState>) -> TreeDto {
    let mut tree = state.tree.lock().unwrap();
    *tree = GameTree::new();
    tree.to_dto()
}

#[tauri::command]
fn get_tree(state: State<AppState>) -> TreeDto {
    state.tree.lock().unwrap().to_dto()
}

#[tauri::command]
fn legal_moves(state: State<AppState>, node_id: u32) -> Result<Vec<MoveDto>, String> {
    state.tree.lock().unwrap().legal_moves(node_id)
}

#[tauri::command]
fn play_move(
    state: State<AppState>,
    node_id: u32,
    from: &str,
    to: &str,
    promotion: Option<&str>,
) -> Result<u32, String> {
    state.tree.lock().unwrap().play_move(node_id, from, to, promotion)
}

#[tauri::command]
fn set_comment(
    state: State<AppState>,
    node_id: u32,
    before: Option<String>,
    after: Option<String>,
) -> Result<(), String> {
    state.tree.lock().unwrap().set_comment(node_id, before, after)
}

#[tauri::command]
fn set_nags(state: State<AppState>, node_id: u32, nags: Vec<u8>) -> Result<(), String> {
    state.tree.lock().unwrap().set_nags(node_id, nags)
}

#[tauri::command]
fn set_shapes(state: State<AppState>, node_id: u32, shapes: Vec<ShapeDto>) -> Result<(), String> {
    state.tree.lock().unwrap().set_shapes(node_id, shapes)
}

#[tauri::command]
fn save_game(state: State<AppState>) -> Result<i64, String> {
    let tree = state.tree.lock().unwrap();
    let pgn = pgn::write_game(&tree);
    let conn = state.db.lock().unwrap();
    db::save_game(&conn, &pgn, &tree.tags)
}

#[tauri::command]
fn list_pgn_games(path: String) -> Result<Vec<PgnGameSummary>, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    pgn::list_games(&text)
}

#[tauri::command]
fn import_pgn(state: State<AppState>, path: String, index: u32) -> Result<TreeDto, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let parsed = pgn::parse_game_at(&text, index)?;
    let mut tree = state.tree.lock().unwrap();
    *tree = parsed;
    Ok(tree.to_dto())
}

#[tauri::command]
fn export_pgn(state: State<AppState>, path: String) -> Result<(), String> {
    let tree = state.tree.lock().unwrap();
    let pgn = pgn::write_game(&tree);
    std::fs::write(&path, pgn).map_err(|e| e.to_string())
}

#[tauri::command]
fn start_engine(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    let mut slot = state.engine.lock().unwrap();
    if let Some(mut existing) = slot.take() {
        existing.stop();
    }
    *slot = Some(engine::start(app, &path)?);
    Ok(())
}

#[tauri::command]
fn stop_engine(state: State<AppState>) -> Result<(), String> {
    let mut slot = state.engine.lock().unwrap();
    if let Some(mut existing) = slot.take() {
        existing.stop();
    }
    Ok(())
}

#[tauri::command]
fn analyze_position(
    state: State<AppState>,
    fen: String,
    depth: u32,
    multipv: u32,
) -> Result<(), String> {
    let mut slot = state.engine.lock().unwrap();
    match slot.as_mut() {
        Some(handle) => handle.analyze(&fen, depth, multipv),
        None => Err("le moteur n'est pas demarre".to_string()),
    }
}

#[tauri::command]
fn pause_engine(state: State<AppState>) -> Result<(), String> {
    let mut slot = state.engine.lock().unwrap();
    match slot.as_mut() {
        Some(handle) => handle.pause(),
        None => Err("le moteur n'est pas demarre".to_string()),
    }
}

#[tauri::command]
fn opening_explorer(state: State<AppState>, moves: Vec<String>) -> Result<Vec<OpeningMoveStat>, String> {
    let conn = state.db.lock().unwrap();
    explorer::explore(&conn, &moves)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("chesscraft.sqlite"))?;

            let tree = match db::load_latest_game(&conn) {
                Ok(Some(pgn_text)) => pgn::parse_game(&pgn_text).unwrap_or_else(|_| GameTree::new()),
                _ => GameTree::new(),
            };

            app.manage(AppState {
                tree: Mutex::new(tree),
                db: Mutex::new(conn),
                engine: Mutex::new(None),
            });

            let new_game_item = MenuItemBuilder::with_id("new_game", "Nouvelle partie").build(app)?;
            let import_item = MenuItemBuilder::with_id("import_pgn", "Importer PGN...").build(app)?;
            let export_item = MenuItemBuilder::with_id("export_pgn", "Exporter PGN...").build(app)?;
            let save_item = MenuItemBuilder::with_id("save_game", "Sauvegarder")
                .accelerator("CmdOrCtrl+S")
                .build(app)?;
            let file_menu = SubmenuBuilder::new(app, "Fichier")
                .item(&new_game_item)
                .separator()
                .item(&import_item)
                .item(&export_item)
                .separator()
                .item(&save_item)
                .build()?;

            let start_engine_item =
                MenuItemBuilder::with_id("start_engine", "Demarrer Stockfish").build(app)?;
            let stop_engine_item =
                MenuItemBuilder::with_id("stop_engine", "Arreter Stockfish").build(app)?;
            let engine_menu = SubmenuBuilder::new(app, "Moteur")
                .item(&start_engine_item)
                .item(&stop_engine_item)
                .build()?;

            let menu = Menu::with_items(app, &[&file_menu, &engine_menu])?;
            app.set_menu(menu)?;

            app.on_menu_event(move |app_handle, event| {
                let event_name = match event.id().as_ref() {
                    "new_game" => "menu-new-game",
                    "import_pgn" => "menu-import-pgn",
                    "export_pgn" => "menu-export-pgn",
                    "save_game" => "menu-save-game",
                    "start_engine" => "menu-start-engine",
                    "stop_engine" => "menu-stop-engine",
                    _ => return,
                };
                let _ = app_handle.emit(event_name, ());
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            new_game,
            get_tree,
            legal_moves,
            play_move,
            set_comment,
            set_nags,
            set_shapes,
            save_game,
            list_pgn_games,
            import_pgn,
            export_pgn,
            start_engine,
            stop_engine,
            analyze_position,
            pause_engine,
            opening_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
