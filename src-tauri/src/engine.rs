use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, EnPassantMode, Position};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct EngineHandle {
    child: Child,
    stdin: ChildStdin,
    current_fen: Arc<Mutex<String>>,
}

/// Un coup de la ligne principale, avec la position qui en resulte -- pour
/// le petit echiquier au survol d'un coup dans le panneau Moteur (voir
/// EnginePanel.tsx), sans avoir a rejouer les coups cote frontend (aucune
/// logique d'echecs cote JS dans cette appli, toujours cote Rust).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PvMove {
    pub san: String,
    pub fen: String,
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfoPayload {
    pub depth: u32,
    pub multipv: u32,
    pub score_cp: Option<i32>,
    pub score_mate: Option<i32>,
    pub pv: Vec<PvMove>,
}

struct ParsedInfo {
    depth: Option<u32>,
    multipv: u32,
    score_cp: Option<i32>,
    score_mate: Option<i32>,
    pv: Vec<String>,
}

fn parse_info_line(line: &str) -> Option<ParsedInfo> {
    if !line.starts_with("info") {
        return None;
    }
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut depth = None;
    let mut multipv = 1u32;
    let mut score_cp = None;
    let mut score_mate = None;
    let mut pv = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "multipv" => {
                multipv = tokens
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                i += 2;
            }
            "score" => match tokens.get(i + 1).copied() {
                Some("cp") => {
                    score_cp = tokens.get(i + 2).and_then(|s| s.parse().ok());
                    i += 3;
                }
                Some("mate") => {
                    score_mate = tokens.get(i + 2).and_then(|s| s.parse().ok());
                    i += 3;
                }
                _ => i += 1,
            },
            "pv" => {
                pv = tokens[i + 1..].iter().map(|s| s.to_string()).collect();
                break;
            }
            _ => i += 1,
        }
    }
    if depth.is_none() && score_cp.is_none() && score_mate.is_none() && pv.is_empty() {
        return None;
    }
    Some(ParsedInfo {
        depth,
        multipv,
        score_cp,
        score_mate,
        pv,
    })
}

/// Convertit la ligne principale (coups UCI) en SAN lisible, en rejouant
/// depuis la position analysee -- capture au passage le FEN et les cases
/// origine/destination apres chaque coup (voir PvMove), pour le petit
/// echiquier au survol d'un coup dans le panneau Moteur, sans que le
/// frontend ait besoin de rejouer les coups lui-meme. Ramene aussi le score
/// a la perspective des Blancs (l'UCI le donne du point de vue du joueur au
/// trait).
fn resolve_line(fen: &str, info: &ParsedInfo) -> Option<EngineInfoPayload> {
    let depth = info.depth?;
    let setup = Fen::from_ascii(fen.as_bytes()).ok()?;
    let mut position: Chess = setup.into_position(CastlingMode::Standard).ok()?;
    let white_to_move = position.turn() == Color::White;

    let mut pv = Vec::new();
    for uci_str in &info.pv {
        let Ok(uci) = UciMove::from_ascii(uci_str.as_bytes()) else {
            break;
        };
        let Ok(mv) = uci.to_move(&position) else {
            break;
        };
        let san = San::from_move(&position, mv.clone()).to_string();
        let from = uci.from().map(|s| s.to_string()).unwrap_or_default();
        let to = uci.to().map(|s| s.to_string()).unwrap_or_default();
        let Ok(next) = position.play(mv) else {
            break;
        };
        position = next;
        pv.push(PvMove {
            san,
            fen: Fen::from_position(&position, EnPassantMode::Legal).to_string(),
            from,
            to,
        });
    }

    let sign = if white_to_move { 1 } else { -1 };
    Some(EngineInfoPayload {
        depth,
        multipv: info.multipv,
        score_cp: info.score_cp.map(|v| v * sign),
        score_mate: info.score_mate.map(|v| v * sign),
        pv,
    })
}

fn write_line(stdin: &mut ChildStdin, line: &str) -> Result<(), String> {
    writeln!(stdin, "{line}").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

// Stockfish est une application console ; chesscraft (GUI, sans console
// propre en version installee) le lancerait sinon avec une console visible
// creee par Windows a cote de la fenetre de l'appli -- la fermer tue le
// processus moteur, cassant le pipe stdin/stdout ("Le canal de communication
// est sur le point d'etre ferme.", os error 232, cote panneau Moteur). En
// dev, cargo run/tauri dev a deja sa propre console attachee au terminal,
// donc Windows n'en recree pas pour l'enfant -- le bug n'apparait qu'en
// version installee.
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

pub fn start(app: AppHandle, path: &str, tab_id: u32) -> Result<EngineHandle, String> {
    let mut command = Command::new(path);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_console_window(&mut command);
    let mut child = command.spawn().map_err(|e| format!("impossible de lancer le moteur : {e}"))?;

    let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
    let stdout = child.stdout.take().ok_or("stdout indisponible")?;
    let mut reader = BufReader::new(stdout);

    write_line(&mut stdin, "uci")?;
    wait_for(&mut reader, "uciok")?;
    write_line(&mut stdin, "isready")?;
    wait_for(&mut reader, "readyok")?;
    write_line(&mut stdin, "ucinewgame")?;

    let current_fen = Arc::new(Mutex::new(String::new()));
    let thread_fen = current_fen.clone();

    let event_name = format!("engine-info-{tab_id}");
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(info) = parse_info_line(line.trim_end()) {
                        let fen = thread_fen.lock().unwrap().clone();
                        if !fen.is_empty() {
                            if let Some(payload) = resolve_line(&fen, &info) {
                                let _ = app.emit(&event_name, payload);
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(EngineHandle {
        child,
        stdin,
        current_fen,
    })
}

fn wait_for(reader: &mut BufReader<std::process::ChildStdout>, token: &str) -> Result<(), String> {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err("le moteur s'est arrêté de manière inattendue".to_string()),
            Ok(_) => {
                if line.trim() == token {
                    return Ok(());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

impl EngineHandle {
    pub fn analyze(&mut self, fen: &str, depth: u32, multipv: u32, threads: u32) -> Result<(), String> {
        *self.current_fen.lock().unwrap() = fen.to_string();
        write_line(&mut self.stdin, "stop")?;
        write_line(
            &mut self.stdin,
            &format!("setoption name MultiPV value {multipv}"),
        )?;
        write_line(
            &mut self.stdin,
            &format!("setoption name Threads value {threads}"),
        )?;
        write_line(&mut self.stdin, &format!("position fen {fen}"))?;
        write_line(&mut self.stdin, &format!("go depth {depth}"))
    }

    /// Arrete la recherche en cours sans tuer le process (reprise rapide possible).
    pub fn pause(&mut self) -> Result<(), String> {
        write_line(&mut self.stdin, "stop")
    }

    pub fn stop(&mut self) {
        let _ = write_line(&mut self.stdin, "quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
