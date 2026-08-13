mod game_tree;

use game_tree::{GameTree, MoveDto, TreeDto};
use std::sync::Mutex;
use tauri::State;

struct AppState {
    tree: Mutex<GameTree>,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            tree: Mutex::new(GameTree::new()),
        })
        .invoke_handler(tauri::generate_handler![
            new_game,
            get_tree,
            legal_moves,
            play_move
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
