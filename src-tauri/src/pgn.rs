use crate::game_tree::{role_to_str, GameTree, Node, STANDARD_TAGS};
use pgn_reader::{Nag, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
use serde::Serialize;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Position};
use std::collections::BTreeMap;
use std::ops::ControlFlow;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PgnGameSummary {
    pub index: u32,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub event: String,
}

/// Serialise l'arbre de partie en PGN standard (tags + movetext).
pub fn write_game(tree: &GameTree) -> String {
    let mut out = String::new();

    for key in STANDARD_TAGS {
        let value = tree
            .tags
            .get(key)
            .map(|s| s.as_str())
            .unwrap_or(if key == "Result" { "*" } else { "?" });
        out.push_str(&format!("[{key} \"{}\"]\n", escape_tag(value)));
    }
    for (key, value) in &tree.tags {
        if !STANDARD_TAGS.contains(&key.as_str()) {
            out.push_str(&format!("[{key} \"{}\"]\n", escape_tag(value)));
        }
    }
    out.push('\n');

    if let Some(first) = tree.node(0).and_then(|root| root.children.first().copied()) {
        write_line(tree, first, true, &mut out);
    }

    out.push_str(tree.tags.get("Result").map(|s| s.as_str()).unwrap_or("*"));
    out.push('\n');
    out
}

fn escape_tag(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn depth_of(tree: &GameTree, id: u32) -> u32 {
    let mut depth = 0;
    let mut current = tree.node(id);
    while let Some(node) = current {
        match node.parent {
            Some(parent_id) => {
                depth += 1;
                current = tree.node(parent_id);
            }
            None => break,
        }
    }
    depth
}

fn write_line(tree: &GameTree, start_id: u32, force_number: bool, out: &mut String) {
    let mut current_id = Some(start_id);
    let mut need_number = force_number;

    while let Some(id) = current_id {
        let node = tree.node(id).expect("noeud connu");
        let depth = depth_of(tree, id);
        let is_white = depth % 2 == 1;
        let move_number = (depth + 1) / 2;

        if let Some(before) = node.comment_before.as_deref().filter(|s| !s.is_empty()) {
            out.push_str("{ ");
            out.push_str(before);
            out.push_str(" } ");
        }

        if is_white {
            out.push_str(&format!("{move_number}. "));
        } else if need_number {
            out.push_str(&format!("{move_number}... "));
        }

        out.push_str(node.san.as_deref().unwrap_or("?"));
        for nag in &node.nags {
            out.push_str(&format!(" ${nag}"));
        }
        out.push(' ');

        if let Some(after) = node.comment_after.as_deref().filter(|s| !s.is_empty()) {
            out.push_str("{ ");
            out.push_str(after);
            out.push_str(" } ");
        }

        let main_child = node.children.first().copied();
        for &variation_id in node.children.iter().skip(1) {
            out.push_str("( ");
            write_line(tree, variation_id, true, out);
            out.push_str(") ");
        }

        need_number = node.children.len() > 1;
        current_id = main_child;
    }
}

/// Reconstruit un arbre de partie a partir d'un texte PGN (la premiere partie du fichier).
/// Reserve au PGN que l'appli genere elle-meme (write_game, toujours UTF-8
/// valide) -- pour du PGN venant d'un fichier importe, voir parse_game_at
/// qui prend des octets bruts.
pub fn parse_game(pgn_text: &str) -> Result<GameTree, String> {
    parse_game_at(pgn_text.as_bytes(), 0)
}

/// Reconstruit l'arbre de la partie a l'index donne (0 = premiere partie du fichier).
/// Prend des octets bruts, pas un &str : les fichiers PGN importes ne sont
/// pas forcement en UTF-8 (Latin-1/Windows-1252 tres courant, notamment pour
/// les noms accentues) -- forcer une conversion en String en amont (comme le
/// faisait auparavant l'appelant via std::fs::read_to_string) plantait sur
/// tout fichier ainsi encode ("stream did not contain valid UTF-8"). Ici,
/// seules les VALEURS de tag sont decodees (en lossy, voir
/// SummaryVisitor/GameVisitor::tag), jamais le flux entier d'un coup.
/// Protege par catch_unwind : du texte PGN inhabituel (notamment issu d'un
/// import de masse non valide coup par coup) ne doit jamais faire planter
/// l'application entiere, juste renvoyer une erreur.
pub fn parse_game_at(pgn_bytes: &[u8], index: u32) -> Result<GameTree, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse_game_at_inner(pgn_bytes, index)))
        .unwrap_or_else(|_| Err("partie mal formée (erreur interne du parseur)".to_string()))
}

fn parse_game_at_inner(pgn_bytes: &[u8], index: u32) -> Result<GameTree, String> {
    let mut reader = Reader::new(pgn_bytes);
    for _ in 0..index {
        let mut skip_visitor = SummaryVisitor::new();
        reader
            .read_game(&mut skip_visitor)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "partie introuvable".to_string())?;
    }
    let mut visitor = GameVisitor::new();
    reader
        .read_game(&mut visitor)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "partie introuvable".to_string())?;
    Ok(GameTree::from_nodes(visitor.nodes, visitor.tags))
}

/// Liste legere (tags seulement, sans construire l'arbre) de toutes les
/// parties d'un fichier PGN, pour proposer un choix a l'import.
pub fn list_games(pgn_bytes: &[u8]) -> Result<Vec<PgnGameSummary>, String> {
    let mut reader = Reader::new(pgn_bytes);
    let mut summaries = Vec::new();
    let mut index = 0u32;
    while let Some(tags) = {
        let mut visitor = SummaryVisitor::new();
        reader.read_game(&mut visitor).map_err(|e| e.to_string())?
    } {
        let get = |key: &str, default: &str| tags.get(key).cloned().unwrap_or_else(|| default.to_string());
        summaries.push(PgnGameSummary {
            index,
            white: get("White", "?"),
            black: get("Black", "?"),
            date: get("Date", "????.??.??"),
            result: get("Result", "*"),
            event: get("Event", "?"),
        });
        index += 1;
    }
    Ok(summaries)
}

struct SummaryVisitor {
    tags: BTreeMap<String, String>,
}

impl SummaryVisitor {
    fn new() -> Self {
        SummaryVisitor { tags: BTreeMap::new() }
    }
}

impl Visitor for SummaryVisitor {
    type Tags = ();
    type Movetext = ();
    type Output = BTreeMap<String, String>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(())
    }

    fn tag(
        &mut self,
        _tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        if let Ok(name) = std::str::from_utf8(name) {
            self.tags
                .insert(name.to_string(), value.decode_utf8_lossy().into_owned());
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        std::mem::take(&mut self.tags)
    }
}

struct GameVisitor {
    nodes: Vec<Node>,
    current: u32,
    stack: Vec<u32>,
    pending_comment: String,
    tags: BTreeMap<String, String>,
}

impl GameVisitor {
    fn new() -> Self {
        let root = Node {
            id: 0,
            parent: None,
            children: Vec::new(),
            san: None,
            from: None,
            to: None,
            promotion: None,
            position: Chess::default(),
            comment_before: None,
            comment_after: None,
            nags: Vec::new(),
            shapes: Vec::new(),
        };
        GameVisitor {
            nodes: vec![root],
            current: 0,
            stack: Vec::new(),
            pending_comment: String::new(),
            tags: BTreeMap::new(),
        }
    }

    fn flush_comment(&mut self) {
        if self.pending_comment.trim().is_empty() {
            self.pending_comment.clear();
            return;
        }
        let text = self.pending_comment.trim().to_string();
        let node = &mut self.nodes[self.current as usize];
        node.comment_after = Some(match node.comment_after.take() {
            Some(existing) => format!("{existing} {text}"),
            None => text,
        });
        self.pending_comment.clear();
    }
}

impl Visitor for GameVisitor {
    type Tags = ();
    type Movetext = ();
    type Output = ();

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(())
    }

    fn tag(
        &mut self,
        _tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        if let Ok(name) = std::str::from_utf8(name) {
            self.tags
                .insert(name.to_string(), value.decode_utf8_lossy().into_owned());
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        self.flush_comment();
        let position = self.nodes[self.current as usize].position.clone();
        let Ok(mv) = san_plus.san.to_move(&position) else {
            return ControlFlow::Continue(());
        };

        let uci = UciMove::from_standard(mv.clone());
        let from = uci.from().map(|s| s.to_string()).unwrap_or_default();
        let to = uci.to().map(|s| s.to_string()).unwrap_or_default();
        let promotion = mv.promotion().map(role_to_str);
        let san = San::from_move(&position, mv.clone()).to_string();

        let Ok(new_position) = position.play(mv) else {
            return ControlFlow::Continue(());
        };

        let new_id = self.nodes.len() as u32;
        self.nodes.push(Node {
            id: new_id,
            parent: Some(self.current),
            children: Vec::new(),
            san: Some(san),
            from: Some(from),
            to: Some(to),
            promotion,
            position: new_position,
            comment_before: None,
            comment_after: None,
            nags: Vec::new(),
            shapes: Vec::new(),
        });
        self.nodes[self.current as usize].children.push(new_id);
        self.current = new_id;
        ControlFlow::Continue(())
    }

    fn nag(&mut self, _movetext: &mut Self::Movetext, nag: Nag) -> ControlFlow<Self::Output> {
        self.nodes[self.current as usize].nags.push(nag.0);
        ControlFlow::Continue(())
    }

    fn comment(
        &mut self,
        _movetext: &mut Self::Movetext,
        comment: RawComment<'_>,
    ) -> ControlFlow<Self::Output> {
        self.pending_comment
            .push_str(&String::from_utf8_lossy(comment.as_bytes()));
        ControlFlow::Continue(())
    }

    fn begin_variation(&mut self, _movetext: &mut Self::Movetext) -> ControlFlow<Self::Output, Skip> {
        self.flush_comment();
        self.stack.push(self.current);
        self.current = self.nodes[self.current as usize].parent.unwrap_or(self.current);
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, _movetext: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        self.flush_comment();
        if let Some(previous) = self.stack.pop() {
            self.current = previous;
        }
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        self.flush_comment();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un fichier PGN importe n'est pas forcement en UTF-8 (Latin-1/Windows-1252
    // tres courant pour les noms accentues, notamment sur des exports TWIC) --
    // 0xE9 seul (ex. "Giri" suivi d'un octet Latin-1 pour "é") n'est pas une
    // sequence UTF-8 valide. list_games/parse_game_at prennent des octets
    // bruts precisement pour ne jamais planter sur ce cas ("stream did not
    // contain valid UTF-8", leve auparavant par read_to_string en amont) --
    // le nom en resultera simplement decode en lossy (voir GameVisitor::tag),
    // pas parfaitement mais sans crash.
    // Fichiers PGN "vides" au sens large : 0 octet, juste des tags sans
    // coup, fichier tronque juste apres les tags -- aucun de ces cas ne doit
    // planter (catch_unwind pour parse_game_at, aucun panic naturel dans
    // list_games/pgn-reader constate ici), juste renvoyer 0 partie ou une
    // erreur propre ("partie introuvable").
    #[test]
    fn list_games_and_parse_game_at_handle_empty_or_moveless_pgn_without_panicking() {
        assert_eq!(list_games(&[]).unwrap().len(), 0);
        assert!(parse_game_at(&[], 0).is_err());
        assert_eq!(list_games(b"\n").unwrap().len(), 0);

        let no_moves: &[u8] = b"[White \"A\"]\n[Black \"B\"]\n\n";
        assert_eq!(list_games(no_moves).unwrap().len(), 1);
        assert!(parse_game_at(no_moves, 0).is_ok());

        let truncated: &[u8] = b"[White \"A\"]\n[Black \"B\"]";
        assert!(parse_game_at(truncated, 0).is_ok());
    }

    #[test]
    fn list_games_handles_non_utf8_bytes_without_erroring() {
        let pgn: &[u8] = b"[White \"Gir\xE9,A\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n";
        let games = list_games(pgn).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].black, "Bob");
    }

    #[test]
    fn parse_game_at_handles_non_utf8_bytes_without_erroring() {
        let pgn: &[u8] = b"[White \"Gir\xE9,A\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n";
        let tree = parse_game_at(pgn, 0).unwrap();
        assert_eq!(tree.tags.get("Black").map(String::as_str), Some("Bob"));
    }
}
