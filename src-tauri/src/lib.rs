mod db;
mod game_tree;
mod pgn;

use game_tree::{GameTree, MoveDto, ShapeDto, TreeDto};
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    tree: Mutex<GameTree>,
    db: Mutex<Connection>,
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
fn import_pgn(state: State<AppState>, path: String) -> Result<TreeDto, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let parsed = pgn::parse_game(&text)?;
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
            import_pgn,
            export_pgn
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
