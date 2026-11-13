use serde::{Deserialize, Serialize};
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
    /// Le moteur annonce une option "Hash" (voir read_uci_options).
    supports_hash: bool,
    /// Derniere taille de table envoyee : "setoption name Hash" vide la table
    /// de hachage, a ne renvoyer que si la valeur change vraiment.
    current_hash_mb: Option<u32>,
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
    /// "q"/"r"/"b"/"n", ou absent pour un coup qui n'est pas une promotion --
    /// meme format que le `promotion` attendu par la commande play_move,
    /// pour pouvoir rejouer un coup de la ligne principale tel quel (voir
    /// GameTab.tsx::handlePvMoveClick).
    pub promotion: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfoPayload {
    pub depth: u32,
    pub multipv: u32,
    pub score_cp: Option<i32>,
    pub score_mate: Option<i32>,
    pub pv: Vec<PvMove>,
    /// Probabilites gain / nulle / perte pour mille, du point de vue des
    /// BLANCS (comme le score) -- absent si le moteur ne connait pas
    /// UCI_ShowWDL (voir start).
    pub wdl: Option<[u32; 3]>,
    /// Statistiques de recherche de la ligne d'etat du panneau Moteur.
    /// Chacune est absente si le moteur ne l'envoie pas.
    pub seldepth: Option<u32>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    /// Remplissage de la table de hachage, pour mille.
    pub hashfull: Option<u32>,
}

struct ParsedInfo {
    depth: Option<u32>,
    multipv: u32,
    score_cp: Option<i32>,
    score_mate: Option<i32>,
    /// Score "lowerbound"/"upperbound" : borne provisoire d'une recherche
    /// qui echoue haut ou bas, pas une evaluation definitive.
    bound: bool,
    pv: Vec<String>,
    /// gain / nulle / perte pour mille, du point de vue du camp au trait
    wdl: Option<[u32; 3]>,
    seldepth: Option<u32>,
    nodes: Option<u64>,
    nps: Option<u64>,
    hashfull: Option<u32>,
}

impl ParsedInfo {
    /// Ligne de variante complete et definitive, la seule a afficher dans le
    /// panneau Moteur. Passe ~3 s de recherche (donc a profondeur elevee),
    /// Stockfish envoie aussi "info depth N currmove X currmovenumber K" --
    /// sans score, sans variante et sans multipv (donc compte comme ligne 1) :
    /// ces lignes remplacaient la meilleure variante par une ligne vide au
    /// score "…" (rapporte par l'utilisateur). Les lignes "bound" (score
    /// provisoire, variante souvent reduite a un coup) faisaient de meme
    /// clignoter la ligne 1 : on garde la derniere ligne exacte a la place.
    fn is_complete_line(&self) -> bool {
        self.depth.is_some() && !self.pv.is_empty() && (self.score_cp.is_some() || self.score_mate.is_some()) && !self.bound
    }
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
    let mut bound = false;
    let mut pv = Vec::new();
    let mut wdl = None;
    let mut seldepth = None;
    let mut nodes = None;
    let mut nps = None;
    let mut hashfull = None;
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "seldepth" => {
                seldepth = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "nodes" => {
                nodes = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "nps" => {
                nps = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "hashfull" => {
                hashfull = tokens.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "wdl" => {
                let part = |k: usize| tokens.get(i + k).and_then(|s| s.parse::<u32>().ok());
                if let (Some(w), Some(d), Some(l)) = (part(1), part(2), part(3)) {
                    wdl = Some([w, d, l]);
                }
                i += 4;
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
            "lowerbound" | "upperbound" => {
                bound = true;
                i += 1;
            }
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
        bound,
        pv,
        wdl,
        seldepth,
        nodes,
        nps,
        hashfull,
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
        let promotion = match &uci {
            UciMove::Normal { promotion, .. } => promotion.map(crate::game_tree::role_to_str),
            _ => None,
        };
        let Ok(next) = position.play(mv) else {
            break;
        };
        position = next;
        pv.push(PvMove {
            san,
            fen: Fen::from_position(&position, EnPassantMode::Legal).to_string(),
            from,
            to,
            promotion,
        });
    }

    let sign = if white_to_move { 1 } else { -1 };
    Some(EngineInfoPayload {
        depth,
        multipv: info.multipv,
        score_cp: info.score_cp.map(|v| v * sign),
        score_mate: info.score_mate.map(|v| v * sign),
        pv,
        wdl: info.wdl.map(|[w, d, l]| if white_to_move { [w, d, l] } else { [l, d, w] }),
        seldepth: info.seldepth,
        nodes: info.nodes,
        nps: info.nps,
        hashfull: info.hashfull,
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
    // Options annoncees par CE moteur : UCI_ShowWDL et Hash ne sont demandes
    // que s'il les connait (versions anciennes de Stockfish, autre moteur UCI).
    let options = read_uci_options(&mut reader)?;
    if options.iter().any(|o| o == "UCI_ShowWDL") {
        write_line(&mut stdin, "setoption name UCI_ShowWDL value true")?;
    }
    let supports_hash = options.iter().any(|o| o == "Hash");
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
                    if let Some(info) = parse_info_line(line.trim_end()).filter(ParsedInfo::is_complete_line) {
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
        supports_hash,
        current_hash_mb: None,
    })
}

/// Nom d'une option dans une ligne "option name <nom> type ..." de la
/// reponse a "uci" (le nom peut contenir des espaces).
fn uci_option_name(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("option name ")?;
    let name = rest.split(" type ").next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Lit la reponse a "uci" jusqu'a "uciok" et renvoie les noms des options
/// que le moteur declare.
fn read_uci_options(reader: &mut BufReader<std::process::ChildStdout>) -> Result<Vec<String>, String> {
    let mut options = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err("le moteur s'est arrêté de manière inattendue".to_string()),
            Ok(_) => {
                if line.trim() == "uciok" {
                    return Ok(options);
                }
                if let Some(name) = uci_option_name(&line) {
                    options.push(name);
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
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

/// Evaluation finale d'une position (score ramene a la perspective des
/// Blancs, comme EngineInfoPayload) -- voir analyze_positions.
/// Serialize/Deserialize (pas seulement Serialize comme le reste de ce
/// fichier) : le mode entrainement renvoie une evaluation precalculee au
/// frontend (prefetch_training_judgement, lib.rs) puis la reprend telle
/// quelle dans l'appel suivant (judge_training_move), pour ne plus avoir a
/// la recalculer une fois le coup essaye connu.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PositionEval {
    pub score_cp: Option<i32>,
    pub score_mate: Option<i32>,
}

/// Analyse une liste de positions les unes apres les autres avec un process
/// moteur dedie (distinct de l'EngineHandle du panneau interactif) :
/// EngineHandle::analyze est concu pour du flux (go depth D rendu la main
/// immediatement, les resultats arrivant au fil de l'eau via les evenements
/// "engine-info-{tab}") -- inadapte ici, ou il faut la derniere evaluation
/// *finale* de chaque position, une a la fois, pour le rapport de partie
/// (voir start_game_report dans lib.rs). Bloquant : attend "bestmove" avant
/// de passer a la position suivante.
pub fn analyze_positions(
    path: &str,
    fens: &[String],
    depth: u32,
    threads: u32,
    mut on_progress: impl FnMut(usize),
) -> Result<Vec<PositionEval>, String> {
    let mut command = Command::new(path);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_console_window(&mut command);
    let mut child = command.spawn().map_err(|e| format!("impossible de lancer le moteur : {e}"))?;

    let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
    let stdout = child.stdout.take().ok_or("stdout indisponible")?;
    let mut reader = BufReader::new(stdout);

    write_line(&mut stdin, "uci")?;
    wait_for(&mut reader, "uciok")?;
    write_line(&mut stdin, &format!("setoption name Threads value {threads}"))?;
    write_line(&mut stdin, "isready")?;
    wait_for(&mut reader, "readyok")?;
    write_line(&mut stdin, "ucinewgame")?;

    let mut results = Vec::with_capacity(fens.len());
    let mut line = String::new();
    for (i, fen) in fens.iter().enumerate() {
        write_line(&mut stdin, &format!("position fen {fen}"))?;
        write_line(&mut stdin, &format!("go depth {depth}"))?;

        let white_to_move = fen.split_whitespace().nth(1) != Some("b");
        let mut last_score_cp = None;
        let mut last_score_mate = None;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => return Err("le moteur s'est arrêté de manière inattendue".to_string()),
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if let Some(info) = parse_info_line(trimmed) {
                        if info.multipv <= 1 {
                            if info.score_cp.is_some() {
                                last_score_cp = info.score_cp;
                                last_score_mate = None;
                            } else if info.score_mate.is_some() {
                                last_score_mate = info.score_mate;
                                last_score_cp = None;
                            }
                        }
                    } else if trimmed.starts_with("bestmove") {
                        break;
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }

        let sign = if white_to_move { 1 } else { -1 };
        results.push(PositionEval {
            score_cp: last_score_cp.map(|v| v * sign),
            score_mate: last_score_mate.map(|v| v * sign),
        });
        on_progress(i + 1);
    }

    let _ = write_line(&mut stdin, "quit");
    let _ = child.kill();
    let _ = child.wait();

    Ok(results)
}

/// Process moteur garde VIVANT entre plusieurs analyses (contrairement a
/// analyze_positions, qui relance un process neuf a chaque appel -- toujours
/// adapte au rapport de partie, une seule rafale d'evaluations par partie
/// analysee) -- utilise par le mode entrainement
/// (prefetch_training_judgement/judge_training_move, lib.rs), ou une
/// evaluation est demandee a chaque position de l'exercice, pas une seule
/// fois pour toute une partie. Relancer tout le process (spawn + poignee de
/// main UCI "uci"/"uciok"/"isready"/"readyok"/"ucinewgame") a chaque appel
/// s'est avere dominer largement le temps meme a faible profondeur --
/// rapporte comme "trop lent meme en profondeur 10" -- la ou cette poignee
/// de main ne coute plus qu'une fois pour toute la session d'entrainement.
pub struct BatchEngine {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl BatchEngine {
    pub fn start(path: &str, threads: u32) -> Result<Self, String> {
        let mut command = Command::new(path);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        hide_console_window(&mut command);
        let mut child = command.spawn().map_err(|e| format!("impossible de lancer le moteur : {e}"))?;
        let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
        let stdout = child.stdout.take().ok_or("stdout indisponible")?;
        let mut reader = BufReader::new(stdout);

        write_line(&mut stdin, "uci")?;
        wait_for(&mut reader, "uciok")?;
        write_line(&mut stdin, &format!("setoption name Threads value {threads}"))?;
        write_line(&mut stdin, "isready")?;
        wait_for(&mut reader, "readyok")?;
        write_line(&mut stdin, "ucinewgame")?;

        Ok(Self { child, stdin, reader })
    }

    /// Une analyse de plus sur ce MEME process (pas de nouvelle poignee de
    /// main) : seul le cout reel de "position fen ... ; go depth ..." est
    /// paye a chaque appel.
    pub fn analyze_with_pv(&mut self, fen: &str, depth: u32) -> Result<(PositionEval, Vec<PvMove>), String> {
        write_line(&mut self.stdin, &format!("position fen {fen}"))?;
        write_line(&mut self.stdin, &format!("go depth {depth}"))?;

        let white_to_move = fen.split_whitespace().nth(1) != Some("b");
        let mut last_info: Option<ParsedInfo> = None;
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return Err("le moteur s'est arrêté de manière inattendue".to_string()),
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if let Some(info) = parse_info_line(trimmed) {
                        if info.multipv <= 1 && (info.score_cp.is_some() || info.score_mate.is_some()) {
                            last_info = Some(info);
                        }
                    } else if trimmed.starts_with("bestmove") {
                        break;
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }

        let sign = if white_to_move { 1 } else { -1 };
        match &last_info {
            Some(info) => {
                let eval = PositionEval { score_cp: info.score_cp.map(|v| v * sign), score_mate: info.score_mate.map(|v| v * sign) };
                let pv = resolve_line(fen, info).map(|payload| payload.pv).unwrap_or_default();
                Ok((eval, pv))
            }
            None => Ok((PositionEval { score_cp: None, score_mate: None }, Vec::new())),
        }
    }
}

impl Drop for BatchEngine {
    fn drop(&mut self) {
        let _ = write_line(&mut self.stdin, "quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl EngineHandle {
    pub fn analyze(&mut self, fen: &str, depth: u32, multipv: u32, threads: u32, hash_mb: u32) -> Result<(), String> {
        *self.current_fen.lock().unwrap() = fen.to_string();
        write_line(&mut self.stdin, "stop")?;
        if self.supports_hash && self.current_hash_mb != Some(hash_mb) {
            write_line(&mut self.stdin, &format!("setoption name Hash value {hash_mb}"))?;
            self.current_hash_mb = Some(hash_mb);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn full_pv_line_is_displayed() {
        let info = parse_info_line(
            "info depth 32 seldepth 45 multipv 1 score cp 28 nodes 812345678 nps 2500000 hashfull 999 tbhits 0 time 324000 pv e2e4 e7e5 g1f3",
        )
        .unwrap();
        assert!(info.is_complete_line());
        let payload = resolve_line(START_FEN, &info).unwrap();
        assert_eq!(payload.multipv, 1);
        assert_eq!(payload.score_cp, Some(28));
        assert_eq!(payload.pv.iter().map(|m| m.san.as_str()).collect::<Vec<_>>(), vec!["e4", "e5", "Nf3"]);
    }

    // Cas reel rapporte par l'utilisateur : a profondeur elevee (apres ~3 s
    // de recherche), Stockfish envoie ces lignes sans score ni variante ni
    // multipv -- elles remplacaient la meilleure variante par "…".
    #[test]
    fn currmove_progress_line_is_not_displayed() {
        let info = parse_info_line("info depth 32 currmove e2e4 currmovenumber 1").unwrap();
        assert!(!info.is_complete_line());
    }

    #[test]
    fn bound_lines_are_not_displayed() {
        for line in [
            "info depth 33 seldepth 50 multipv 1 score cp 35 lowerbound nodes 900000000 nps 2500000 time 360000 pv e2e4",
            "info depth 33 seldepth 50 multipv 1 score cp 20 upperbound nodes 900000000 nps 2500000 time 360000 pv e2e4",
        ] {
            assert!(!parse_info_line(line).unwrap().is_complete_line(), "{line}");
        }
    }

    #[test]
    fn search_statistics_and_wdl_are_read() {
        let info = parse_info_line(
            "info depth 28 seldepth 41 multipv 1 score cp 35 wdl 120 850 30 nodes 812345678 nps 2500000 hashfull 453 tbhits 0 time 324000 pv e2e4 e7e5",
        )
        .unwrap();
        assert!(info.is_complete_line());
        let payload = resolve_line(START_FEN, &info).unwrap();
        assert_eq!(payload.score_cp, Some(35));
        assert_eq!(payload.wdl, Some([120, 850, 30]));
        assert_eq!((payload.seldepth, payload.nodes, payload.nps, payload.hashfull), (Some(41), Some(812345678), Some(2500000), Some(453)));
        assert_eq!(payload.pv.len(), 2);
    }

    // comme le score, le WDL est donne par le moteur du point de vue du camp
    // au trait : ramene aux Blancs quand ce sont les Noirs qui jouent.
    #[test]
    fn wdl_is_given_from_whites_point_of_view() {
        let black_to_move = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        let info = parse_info_line("info depth 20 multipv 1 score cp -30 wdl 40 800 160 pv e7e5").unwrap();
        let payload = resolve_line(black_to_move, &info).unwrap();
        assert_eq!(payload.score_cp, Some(30));
        assert_eq!(payload.wdl, Some([160, 800, 40]));
    }

    #[test]
    fn engine_without_wdl_leaves_it_empty() {
        let info = parse_info_line("info depth 20 multipv 1 score cp 12 pv e2e4").unwrap();
        assert_eq!(resolve_line(START_FEN, &info).unwrap().wdl, None);
    }

    #[test]
    fn option_names_are_read_from_the_uci_reply() {
        assert_eq!(uci_option_name("option name Hash type spin default 16 min 1 max 33554432"), Some("Hash".to_string()));
        assert_eq!(uci_option_name("option name UCI_ShowWDL type check default false"), Some("UCI_ShowWDL".to_string()));
        assert_eq!(uci_option_name("option name Clear Hash type button"), Some("Clear Hash".to_string()));
        assert_eq!(uci_option_name("id name Stockfish 17"), None);
    }

    #[test]
    fn mate_score_line_is_displayed() {
        let info = parse_info_line("info depth 40 seldepth 12 multipv 2 score mate 5 nodes 1000 time 10 pv e2e4").unwrap();
        assert!(info.is_complete_line());
        assert_eq!(info.multipv, 2);
    }
}
