use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, Position};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct EngineHandle {
    child: Child,
    stdin: ChildStdin,
    current_fen: Arc<Mutex<String>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfoPayload {
    pub depth: u32,
    pub multipv: u32,
    pub score_cp: Option<i32>,
    pub score_mate: Option<i32>,
    pub pv_san: Vec<String>,
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
/// depuis la position analysee. Ramene aussi le score a la perspective des
/// Blancs (l'UCI le donne du point de vue du joueur au trait).
fn resolve_line(fen: &str, info: &ParsedInfo) -> Option<EngineInfoPayload> {
    let depth = info.depth?;
    let setup = Fen::from_ascii(fen.as_bytes()).ok()?;
    let mut position: Chess = setup.into_position(CastlingMode::Standard).ok()?;
    let white_to_move = position.turn() == Color::White;

    let mut pv_san = Vec::new();
    for uci_str in &info.pv {
        let Ok(uci) = UciMove::from_ascii(uci_str.as_bytes()) else {
            break;
        };
        let Ok(mv) = uci.to_move(&position) else {
            break;
        };
        pv_san.push(San::from_move(&position, mv.clone()).to_string());
        match position.play(mv) {
            Ok(next) => position = next,
            Err(_) => break,
        }
    }

    let sign = if white_to_move { 1 } else { -1 };
    Some(EngineInfoPayload {
        depth,
        multipv: info.multipv,
        score_cp: info.score_cp.map(|v| v * sign),
        score_mate: info.score_mate.map(|v| v * sign),
        pv_san,
    })
}

fn write_line(stdin: &mut ChildStdin, line: &str) -> Result<(), String> {
    writeln!(stdin, "{line}").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

pub fn start(app: AppHandle, path: &str) -> Result<EngineHandle, String> {
    let mut child = Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("impossible de lancer le moteur : {e}"))?;

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
                                let _ = app.emit("engine-info", payload);
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
            Ok(0) => return Err("le moteur s'est arrete de maniere inattendue".to_string()),
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
    pub fn analyze(&mut self, fen: &str, depth: u32) -> Result<(), String> {
        *self.current_fen.lock().unwrap() = fen.to_string();
        write_line(&mut self.stdin, "stop")?;
        write_line(&mut self.stdin, &format!("position fen {fen}"))?;
        write_line(&mut self.stdin, &format!("go depth {depth}"))
    }

    pub fn stop(&mut self) {
        let _ = write_line(&mut self.stdin, "quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
