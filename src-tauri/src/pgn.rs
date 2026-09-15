use crate::game_tree::{role_to_str, GameTree, Node, STANDARD_TAGS};
use pgn_reader::{Nag, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Position};
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

    // Trait/numero de coup a la racine (position de depart standard --
    // Blancs au trait, coup 1 -- ou celle d'un tag FEN personnalise, voir
    // GameVisitor::begin_movetext) : sans ca, une partie/etude demarrant
    // d'un FEN a un numero de coup > 1 (ou au trait des Noirs) se
    // renumerotait a tort a partir de "1." en la resauvegardant.
    let root_white = tree.node(0).map(|root| root.position.turn().is_white()).unwrap_or(true);
    let root_fullmove = tree.node(0).map(|root| root.position.fullmoves().get()).unwrap_or(1);

    // Demarre a la racine elle-meme (pas root.children[0]) : la racine n'a
    // pas de coup propre (write_line l'ignore, voir has_move plus bas) mais
    // peut avoir plusieurs enfants (coups 1 alternatifs) -- s'arreter a
    // children[0] les aurait tous perdus a l'export/sauvegarde, meme
    // correctif que Notation.tsx::renderLine cote frontend.
    write_line(tree, 0, true, root_white, root_fullmove, &mut out);

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

/// (est-ce le trait des Blancs, numero de coup PGN) pour un noeud a `depth`
/// demi-coups de la racine, sachant que la racine elle-meme est au trait de
/// `root_white` et porte le numero `root_fullmove` (1/Blancs pour une partie
/// standard ; ce que porte le tag FEN sinon). Le numero de coup n'avance
/// qu'apres le coup des Noirs, exactement comme la convention PGN/FEN.
fn move_number_at(depth: u32, root_white: bool, root_fullmove: u32) -> (bool, u32) {
    if root_white {
        (depth % 2 == 1, root_fullmove + (depth - 1) / 2)
    } else {
        (depth % 2 == 0, root_fullmove + depth / 2)
    }
}

fn write_line(tree: &GameTree, start_id: u32, force_number: bool, root_white: bool, root_fullmove: u32, out: &mut String) {
    let mut current_id = Some(start_id);
    let mut need_number = force_number;
    // Variantes rattachees au noeud qu'on s'apprete a ecrire (ses propres
    // freres et soeurs cote parent, donc des alternatives a CE noeud) --
    // reportees d'une iteration : la norme PGN (§8.2.5) place une RAV juste
    // apres le coup auquel elle sert d'alternative, jamais avant. Meme
    // correctif que Notation.tsx::renderLine cote frontend.
    let mut pending_variations: Vec<u32> = Vec::new();

    while let Some(id) = current_id {
        let node = tree.node(id).expect("noeud connu");
        // false uniquement pour la racine (position de depart, avant le
        // premier coup) : pas de numero/SAN a ecrire pour elle, seulement
        // un eventuel commentaire d'introduction et ses variantes (coups 1
        // alternatifs), qui suivent le meme traitement que tout le reste.
        let has_move = node.san.is_some();

        if let Some(before) = node.comment_before.as_deref().filter(|s| !s.is_empty()) {
            out.push_str("{ ");
            out.push_str(before);
            out.push_str(" } ");
        }

        if has_move {
            let depth = depth_of(tree, id);
            let (is_white, move_number) = move_number_at(depth, root_white, root_fullmove);
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
        }

        if let Some(after) = node.comment_after.as_deref().filter(|s| !s.is_empty()) {
            out.push_str("{ ");
            out.push_str(after);
            out.push_str(" } ");
        }

        let printed_variation = !pending_variations.is_empty();
        for &variation_id in &pending_variations {
            out.push_str("( ");
            write_line(tree, variation_id, true, root_white, root_fullmove, out);
            out.push_str(") ");
        }

        let main_child = node.children.first().copied();
        pending_variations = node.children.iter().skip(1).copied().collect();
        // Le prochain coup a besoin du "N... " s'il suit une variante qu'on
        // vient d'ecrire (convention PGN), ou si ce noeud-ci n'avait de
        // toute facon aucun coup a ecrire (la racine) : dans ce dernier cas
        // rien n'a consomme le besoin recu en entree, donc on le propage
        // tel quel au premier vrai coup de la ligne.
        need_number = printed_variation || (!has_move && need_number);
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

    // Une partie demarrant d'une position personnalisee (etude, extrait de
    // livre...) porte un tag FEN (par convention accompagne de SetUp "1",
    // mais SetUp n'est pas verifie ici : sa valeur exacte ne change rien,
    // seule la presence d'un FEN exploitable importe). Sans ce correctif, la
    // racine restait Chess::default() (voir GameVisitor::new) : les coups du
    // fichier source, illegaux depuis le depart standard, echouaient tous
    // silencieusement au parsing (voir san() plus bas, `let Ok(mv) = ...
    // else` ignore un coup illegal) et la partie s'ouvrait vide. Un FEN
    // invalide/illegal est ignore silencieusement (retombe sur le depart
    // standard) plutot que de faire echouer tout l'import pour une seule
    // donnee de tag douteuse.
    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        if let Some(fen) = self.tags.get("FEN") {
            if let Some(position) = fen
                .trim()
                .parse::<Fen>()
                .ok()
                .and_then(|setup| setup.into_position::<Chess>(CastlingMode::Standard).ok())
            {
                self.nodes[0].position = position;
            }
        }
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

    // Cas reel rapporte par l'utilisateur : une partie/etude commencant
    // d'une position personnalisee (tag FEN, generalement accompagne de
    // SetUp "1") s'ouvrait vide -- les coups du fichier, illegaux depuis le
    // depart standard (racine figee a Chess::default() avant ce correctif),
    // echouaient tous silencieusement a se rejouer.
    #[test]
    fn parse_game_at_starts_from_the_fen_tag_when_present() {
        let pgn: &[u8] = b"[White \"Perelshteyn\"]\n[Black \"Elbilia\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"3r1r1k/pp3p1p/2p1pn2/5p2/3P3R/1BP4P/PP3PP1/2K1R3 w - - 0 23\"]\n\n23. g4 fxg4 24. hxg4 *\n";
        let tree = parse_game_at(pgn, 0).unwrap();
        let root = tree.node(0).unwrap();
        assert!(root.san.is_none());
        // depart au trait blanc (w), donc le premier coup joue (g4) doit
        // etre celui des Blancs -- sur Chess::default() (partie standard),
        // "g4" serait illegal comme tout premier coup et la partie resterait
        // sans aucun enfant.
        let first_move_id = root.children.first().copied().expect("g4 doit avoir ete rejoue");
        assert_eq!(tree.node(first_move_id).unwrap().san.as_deref(), Some("g4"));
        assert_eq!(tree.mainline_sans(), vec!["g4", "fxg4", "hxg4"]);
    }

    // Reexporter une partie demarree d'un FEN (voir le test precedent) doit
    // garder son numero de coup d'origine (23), pas repartir de "1." -- la
    // numerotation d'export se base desormais sur le trait/numero de la
    // racine (write_game::root_white/root_fullmove, lus depuis la position
    // deja reconstruite, la meme que celle affichee) plutot que de supposer
    // a tort un depart standard.
    #[test]
    fn write_game_keeps_the_fen_starting_move_number() {
        let pgn: &[u8] = b"[White \"Perelshteyn\"]\n[Black \"Elbilia\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"3r1r1k/pp3p1p/2p1pn2/5p2/3P3R/1BP4P/PP3PP1/2K1R3 w - - 0 23\"]\n\n23. g4 fxg4 24. hxg4 *\n";
        let tree = parse_game_at(pgn, 0).unwrap();
        let exported = write_game(&tree);
        assert!(exported.contains("23. g4 fxg4 24. hxg4"), "numerotation inattendue :\n{exported}");
    }

    // Meme chose quand le FEN de depart est au trait des Noirs : le tout
    // premier coup ecrit doit porter le suffixe "N... " (convention PGN pour
    // "on reprend en cours de coup"), pas "N. " comme s'il s'agissait des
    // Blancs.
    #[test]
    fn write_game_uses_ellipsis_notation_when_the_fen_starts_on_black_to_move() {
        let pgn: &[u8] =
            b"[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2\"]\n\n2... Nc6 3. Bb5 *\n";
        let tree = parse_game_at(pgn, 0).unwrap();
        let exported = write_game(&tree);
        assert!(exported.contains("2... Nc6 3. Bb5"), "numerotation inattendue :\n{exported}");
    }

    // Reproduit le bug signale : promouvoir/creer une variante au tout
    // premier coup la faisait disparaitre completement de l'export/
    // sauvegarde (write_game ne parcourait que root.children[0]) ; verifie
    // au passage l'ordre (la variante suit le coup dont elle est
    // l'alternative, jamais l'inverse -- norme PGN §8.2.5).
    #[test]
    fn write_game_exports_root_level_variations_after_the_move_they_replace() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        tree.play_move(e4, "e7", "e5", None).unwrap();
        let d4 = tree.play_move(0, "d2", "d4", None).unwrap();
        tree.play_move(d4, "d7", "d5", None).unwrap();

        let exported = write_game(&tree);
        assert!(
            exported.contains("1. e4 ( 1. d4 d5 ) 1... e5"),
            "variante du premier coup absente ou mal placee :\n{exported}"
        );
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
