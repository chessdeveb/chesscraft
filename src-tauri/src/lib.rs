mod bases;
mod bulk_import;
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
            // en flux (BufReader), pas charge entierement en memoire : des
            // bases source comme LumbrasGigaBase depassent 8 Go, et
            // read_to_string() les chargeait integralement avant meme de
            // commencer -- gros pic memoire, pouvant epuiser la RAM
            // disponible et interrompre l'import en cours de route (voir
            // bulk_import::bulk_import).
            let pgn_file = std::fs::File::open(&pgn_path).map_err(|e| e.to_string())?;
            let pgn_reader = std::io::BufReader::new(pgn_file);
            let mut conn = db::open(Path::new(&db_path))?;

            // Reecrit Blancs/Noirs vers leur forme canonique FIDE quand elle
            // est resolue sans ambiguite (voir fide::canonicalize,
            // departage par Elo en cas d'homonymie -- ex. "Giri, Anish" vs
            // "Giri, Andrea"), et recupere au passage l'identifiant FIDE
            // (pose sur games.white_fide_id/black_fide_id, voir
            // bulk_import::process_one_game) -- memorise par (nom, indice
            // Elo) deja vu (fide_name_cache) : une base source rejoue tres
            // souvent les memes joueurs a un Elo proche, inutile de refaire
            // la recherche FIDE a chaque partie (meme principe que
            // rebuild_player_stats/rebuild_prefix_stats). Base FIDE absente
            // ou pas encore telechargee (voir refresh_fide_players) :
            // fide_conn est alors None et canonicalize_name ne fait rien,
            // l'import se deroule normalement.
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

            let summary = bulk_import::bulk_import(
                &mut conn,
                pgn_reader,
                move |summary| {
                    let _ = progress_app.emit("bulk-import-progress", summary.clone());
                },
                move || *cancel_app.state::<AppState>().bulk_import_cancel.lock().unwrap(),
                canonicalize_name,
            )?;
            // recalcule les stats precalculees de l'explorateur d'ouvertures
            // (voir explorer::rebuild_prefix_stats) : cout marginal a cote de
            // l'import lui-meme, evite de retomber sur le calcul a la volee,
            // lent sur les tout premiers coups, au prochain jeu ouvert.
            // Echec non bloquant : l'explorateur retombe alors sur le calcul
            // a la volee (voir explorer::explore).
            let _ = explorer::rebuild_prefix_stats(&mut conn, |_| {});
            let _ = db::rebuild_player_stats(&mut conn, |_| {});
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

/// Progression d'un rebuild_prefix_stats/rebuild_player_stats, avec le nom
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
/// opening_prefix_stats/player_stats sur une base choisie manuellement (voir
/// explorer::rebuild_prefix_stats, db::rebuild_player_stats). Sert a rattraper
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
            explorer::rebuild_prefix_stats(&mut conn, |progress| {
                let _ = progress_app.emit("tool-rebuild-stats-progress", ToolRebuildProgress { stage: "openings", progress });
            })?;
            db::rebuild_player_stats(&mut conn, |progress| {
                let _ = progress_app.emit("tool-rebuild-stats-progress", ToolRebuildProgress { stage: "players", progress });
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
                explorer::rebuild_prefix_stats(&mut conn, |progress| {
                    let _ = progress_app.emit("tool-deduplicate-rebuild-progress", ToolRebuildProgress { stage: "openings", progress });
                })?;
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

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn opening_explorer(
    app: AppHandle,
    state: State<AppState>,
    tab_id: u32,
    moves: Vec<String>,
    player_name: Option<String>,
    player_white: Option<bool>,
    rating_min: Option<i32>,
    rating_max: Option<i32>,
    year_min: Option<i32>,
    year_max: Option<i32>,
) -> Result<Vec<OpeningMoveStat>, String> {
    ensure_reference_conn(&app, &state)?;
    let filters = build_explorer_filters(&state, tab_id, player_name, player_white, rating_min, rating_max, year_min, year_max);

    // le cache partage (explorer_cache) ne couvre que la vue non filtree,
    // commune a tous les onglets ; des que le moindre filtre (preparation ou
    // ad-hoc) est actif, la reponse est propre a cette combinaison precise
    // et reste rapide sans avoir besoin d'etre mise en cache (voir
    // explorer::ExplorerFilters).
    if filters.is_empty() {
        let key = moves.join(" ");
        if let Some(cached) = state.explorer_cache.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let conn_cache = state.reference_conn.lock().unwrap();
        let conn = &conn_cache.as_ref().unwrap().1;
        let stats = explorer::explore(conn, &moves, &filters)?;
        state.explorer_cache.lock().unwrap().insert(key, stats.clone());
        return Ok(stats);
    }
    let conn_cache = state.reference_conn.lock().unwrap();
    let conn = &conn_cache.as_ref().unwrap().1;
    explorer::explore(conn, &moves, &filters)
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
    let conn_cache = state.reference_conn.lock().unwrap();
    let (base_path, conn) = conn_cache.as_ref().unwrap();
    let total = db::count_games_at_position(conn, &moves, &filters)?;
    let games = db::games_at_position(conn, &moves, &filters, sort_by.as_deref().unwrap_or(""), sort_dir.as_deref().unwrap_or(""), limit, offset)?;
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
    let bulk_import_item = MenuItemBuilder::with_id("bulk_import_pgn", t("menu.file.importPgn"))
        .accelerator("CmdOrCtrl+I")
        .build(app)?;
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
        .item(&bulk_import_item)
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
            set_shapes,
            set_tags,
            list_pgn_games,
            import_pgn,
            bulk_import_pgn,
            cancel_bulk_import,
            rebuild_explorer_stats,
            count_duplicate_games,
            deduplicate_base,
            backfill_fide_ids,
            export_pgn,
            start_engine,
            stop_engine,
            analyze_position,
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
