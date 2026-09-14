use serde::{Deserialize, Serialize};
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode, Position, Role, Square};
use std::collections::BTreeMap;
use std::str::FromStr;

pub struct Shape {
    pub orig: String,
    pub dest: Option<String>,
    pub brush: String,
}

pub struct Node {
    pub id: u32,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub san: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub promotion: Option<String>,
    pub position: Chess,
    pub comment_before: Option<String>,
    pub comment_after: Option<String>,
    pub nags: Vec<u8>,
    pub shapes: Vec<Shape>,
}

pub struct GameTree {
    nodes: Vec<Node>,
    pub tags: BTreeMap<String, String>,
}

pub const STANDARD_TAGS: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

fn default_tags() -> BTreeMap<String, String> {
    let mut tags = BTreeMap::new();
    tags.insert("Event".to_string(), "?".to_string());
    tags.insert("Site".to_string(), "?".to_string());
    tags.insert("Date".to_string(), "????.??.??".to_string());
    tags.insert("Round".to_string(), "?".to_string());
    tags.insert("White".to_string(), "?".to_string());
    tags.insert("Black".to_string(), "?".to_string());
    tags.insert("Result".to_string(), "*".to_string());
    tags
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MoveDto {
    pub from: String,
    pub to: String,
    pub promotion: Option<String>,
    pub san: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShapeDto {
    pub orig: String,
    pub dest: Option<String>,
    pub brush: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    pub id: u32,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub san: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub fen: String,
    pub turn: String,
    pub comment_before: Option<String>,
    pub comment_after: Option<String>,
    pub nags: Vec<u8>,
    pub shapes: Vec<ShapeDto>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TreeDto {
    pub nodes: Vec<NodeDto>,
    pub root: u32,
    pub tags: BTreeMap<String, String>,
}

pub(crate) fn role_to_str(role: Role) -> String {
    match role {
        Role::Pawn => "p",
        Role::Knight => "n",
        Role::Bishop => "b",
        Role::Rook => "r",
        Role::Queen => "q",
        Role::King => "k",
    }
    .to_string()
}

fn str_to_role(s: &str) -> Option<Role> {
    match s {
        "n" => Some(Role::Knight),
        "b" => Some(Role::Bishop),
        "r" => Some(Role::Rook),
        "q" => Some(Role::Queen),
        _ => None,
    }
}

fn position_fen(position: &Chess) -> String {
    Fen::from_position(position, EnPassantMode::Legal).to_string()
}

impl GameTree {
    pub fn new() -> Self {
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
        GameTree {
            nodes: vec![root],
            tags: default_tags(),
        }
    }

    pub fn from_nodes(nodes: Vec<Node>, tags: BTreeMap<String, String>) -> Self {
        GameTree { nodes, tags }
    }

    /// Nouvelle partie dont la racine est une position choisie (editeur de
    /// position, menu Fichier > Nouvelle position), plutot que le depart
    /// standard. `fen` est la position seule (Chess::default() reste la
    /// reference pour une partie normale) ; on rejette a la fois un FEN mal
    /// forme et une position illegale (roi manquant, pions sur la 1ere/8eme
    /// rangee...) plutot que de laisser l'echiquier planter plus tard sur un
    /// coup.
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let setup: Fen = fen.trim().parse().map_err(|e| format!("FEN invalide : {e}"))?;
        let position: Chess = setup
            .into_position(CastlingMode::Standard)
            .map_err(|e| format!("Position illégale : {e}"))?;
        let root = Node {
            id: 0,
            parent: None,
            children: Vec::new(),
            san: None,
            from: None,
            to: None,
            promotion: None,
            position,
            comment_before: None,
            comment_after: None,
            nags: Vec::new(),
            shapes: Vec::new(),
        };
        Ok(GameTree { nodes: vec![root], tags: default_tags() })
    }

    pub(crate) fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// La suite de coups (SAN) de la ligne principale, du debut a la fin de la partie.
    pub fn mainline_sans(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = self.node(0).and_then(|root| root.children.first().copied());
        while let Some(id) = current {
            let node = self.node(id).expect("noeud connu");
            out.push(node.san.clone().unwrap_or_default());
            current = node.children.first().copied();
        }
        out
    }

    /// Les identifiants de noeuds de la ligne principale, racine incluse (la
    /// racine represente la position avant le premier coup -- necessaire au
    /// rapport de partie, voir lib.rs::start_game_report, pour evaluer aussi
    /// bien la position de depart que chaque position apres coup).
    pub fn mainline_node_ids(&self) -> Vec<u32> {
        let mut out = vec![0u32];
        let mut current = self.node(0).and_then(|root| root.children.first().copied());
        while let Some(id) = current {
            out.push(id);
            current = self.node(id).and_then(|n| n.children.first().copied());
        }
        out
    }

    pub fn node_fen(&self, id: u32) -> Option<String> {
        self.node(id).map(|n| position_fen(&n.position))
    }

    pub fn legal_moves(&self, node_id: u32) -> Result<Vec<MoveDto>, String> {
        let node = self.node(node_id).ok_or_else(|| "nœud introuvable".to_string())?;
        Ok(node
            .position
            .legal_moves()
            .iter()
            .map(|m| {
                // conversion "standard" : un roque s'exprime comme un deplacement du roi
                // vers sa case visuelle d'arrivee (e1g1), pas vers la case de la tour
                let uci = UciMove::from_standard(m.clone());
                MoveDto {
                    from: uci.from().map(|s| s.to_string()).unwrap_or_default(),
                    to: uci.to().map(|s| s.to_string()).unwrap_or_default(),
                    promotion: m.promotion().map(role_to_str),
                    san: San::from_move(&node.position, m.clone()).to_string(),
                }
            })
            .collect())
    }

    pub fn play_move(
        &mut self,
        node_id: u32,
        from: &str,
        to: &str,
        promotion: Option<&str>,
    ) -> Result<u32, String> {
        let node = self.node(node_id).ok_or_else(|| "nœud introuvable".to_string())?;
        let position = node.position.clone();

        let from_sq = Square::from_str(from).map_err(|_| "case de départ invalide".to_string())?;
        let to_sq = Square::from_str(to).map_err(|_| "case d'arrivée invalide".to_string())?;
        let requested_promotion = match promotion {
            Some(p) => Some(str_to_role(p).ok_or_else(|| "promotion invalide".to_string())?),
            None => None,
        };

        // to_move() interprete correctement "e1g1" comme un roque (petit ou grand,
        // notation standard ou chess960) dans le contexte de la position donnee
        let attempt = UciMove::Normal {
            from: from_sq,
            to: to_sq,
            promotion: requested_promotion,
        }
        .to_move(&position);

        let chosen = match attempt {
            Ok(m) => m,
            // si aucune promotion n'a ete precisee mais que le coup ne passe pas,
            // on retente avec une promotion en dame par defaut
            Err(_) if requested_promotion.is_none() => UciMove::Normal {
                from: from_sq,
                to: to_sq,
                promotion: Some(Role::Queen),
            }
            .to_move(&position)
            .map_err(|_| "coup illégal".to_string())?,
            Err(_) => return Err("coup illégal".to_string()),
        };

        let chosen_promotion = chosen.promotion().map(role_to_str);
        let san = San::from_move(&position, chosen.clone()).to_string();

        // reutiliser un enfant existant si ce coup a deja ete joue depuis ce noeud
        if let Some(existing) = self.node(node_id).unwrap().children.iter().find(|&&child_id| {
            let child = &self.nodes[child_id as usize];
            child.from.as_deref() == Some(from) && child.to.as_deref() == Some(to) && child.promotion == chosen_promotion
        }) {
            return Ok(*existing);
        }

        let new_position = position
            .play(chosen)
            .map_err(|_| "coup illégal".to_string())?;

        let new_id = self.nodes.len() as u32;
        self.nodes.push(Node {
            id: new_id,
            parent: Some(node_id),
            children: Vec::new(),
            san: Some(san),
            from: Some(from.to_string()),
            to: Some(to.to_string()),
            promotion: chosen_promotion,
            position: new_position,
            comment_before: None,
            comment_after: None,
            nags: Vec::new(),
            shapes: Vec::new(),
        });
        self.nodes[node_id as usize].children.push(new_id);

        Ok(new_id)
    }

    pub fn set_comment(
        &mut self,
        node_id: u32,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "nœud introuvable".to_string())?;
        node.comment_before = before.filter(|s| !s.is_empty());
        node.comment_after = after.filter(|s| !s.is_empty());
        Ok(())
    }

    /// Fait de la branche menant a `node_id` la ligne principale, a chaque
    /// embranchement traverse depuis la racine (pas seulement le dernier) --
    /// l'inverse de jouer un coup alternatif qui cree une variante. Bouton
    /// "promouvoir la variante" du panneau Notation.
    pub fn promote_variation(&mut self, node_id: u32) -> Result<(), String> {
        let mut chain = Vec::new();
        let mut current = node_id;
        while let Some(parent_id) = self.node(current).ok_or_else(|| "nœud introuvable".to_string())?.parent {
            chain.push((parent_id, current));
            current = parent_id;
        }
        if chain.is_empty() {
            return Err("rien à promouvoir".to_string());
        }
        for (parent_id, child_id) in chain {
            let parent = &mut self.nodes[parent_id as usize];
            if let Some(pos) = parent.children.iter().position(|&c| c == child_id) {
                if pos > 0 {
                    parent.children.remove(pos);
                    parent.children.insert(0, child_id);
                }
            }
        }
        Ok(())
    }

    /// Retire `node_id` (et toute sa descendance, restee dans self.nodes
    /// mais desormais inatteignable depuis la racine -- ni to_dto ni
    /// pgn::write_game ne la reserialisent, tous deux parcourent l'arbre par
    /// les liens children a partir de la racine) de la liste des enfants de
    /// son parent. Bouton "supprimer la variante" du panneau Notation.
    /// Renvoie l'id du parent, pour que l'appelant y repositionne la
    /// selection courante (node_id n'existe plus dans l'arbre affiche).
    pub fn delete_variation(&mut self, node_id: u32) -> Result<u32, String> {
        let parent_id = self
            .node(node_id)
            .ok_or_else(|| "nœud introuvable".to_string())?
            .parent
            .ok_or_else(|| "impossible de supprimer le début de la partie".to_string())?;
        self.nodes[parent_id as usize].children.retain(|&c| c != node_id);
        Ok(parent_id)
    }

    pub fn set_tags(&mut self, tags: BTreeMap<String, String>) {
        for (key, value) in tags {
            if value.is_empty() {
                self.tags.remove(&key);
            } else {
                self.tags.insert(key, value);
            }
        }
    }

    pub fn set_nags(&mut self, node_id: u32, nags: Vec<u8>) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "nœud introuvable".to_string())?;
        node.nags = nags;
        Ok(())
    }

    pub fn set_shapes(&mut self, node_id: u32, shapes: Vec<ShapeDto>) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "nœud introuvable".to_string())?;
        node.shapes = shapes
            .into_iter()
            .map(|s| Shape {
                orig: s.orig,
                dest: s.dest,
                brush: s.brush,
            })
            .collect();
        Ok(())
    }

    pub fn to_dto(&self) -> TreeDto {
        TreeDto {
            root: 0,
            tags: self.tags.clone(),
            nodes: self
                .nodes
                .iter()
                .map(|n| NodeDto {
                    id: n.id,
                    parent: n.parent,
                    children: n.children.clone(),
                    san: n.san.clone(),
                    from: n.from.clone(),
                    to: n.to.clone(),
                    fen: position_fen(&n.position),
                    turn: if n.position.turn() == shakmaty::Color::White {
                        "white".to_string()
                    } else {
                        "black".to_string()
                    },
                    comment_before: n.comment_before.clone(),
                    comment_after: n.comment_after.clone(),
                    nags: n.nags.clone(),
                    shapes: n
                        .shapes
                        .iter()
                        .map(|s| ShapeDto {
                            orig: s.orig.clone(),
                            dest: s.dest.clone(),
                            brush: s.brush.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_fen_accepts_a_valid_custom_position() {
        // position apres 1. e4 e5 2. Nf3, trait aux noirs
        let tree = GameTree::from_fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2").unwrap();
        let dto = tree.to_dto();
        assert_eq!(dto.nodes.len(), 1);
        assert_eq!(dto.nodes[0].turn, "black");
        assert!(dto.nodes[0].san.is_none());
        assert!(dto.nodes[0].parent.is_none());
    }

    #[test]
    fn from_fen_rejects_malformed_syntax() {
        assert!(GameTree::from_fen("pas un fen").is_err());
    }

    #[test]
    fn from_fen_rejects_an_illegal_position() {
        // aucun roi noir : position illegale, pas seulement mal ecrite
        assert!(GameTree::from_fen("8/8/8/8/8/8/8/4K3 w - - 0 1").is_err());
    }

    // Cas reel qui a revele le bug du plafond de balayage (voir
    // db::SHALLOW_SCAN_ROW_CAP) : une capture en passant depuis une position
    // personnalisee (editeur de position) produit un premier coup ("dxe3")
    // qui n'a de sens pour aucune vraie partie -- verifie ici que jouer ce
    // coup et interroger l'explorateur dessus ne panique pas et reste rapide
    // (ce test servait a l'origine a confirmer/infirmer un blocage signale
    // par l'utilisateur ; le vrai probleme etait cote db::games_at_position,
    // voir ses propres tests).
    #[test]
    fn plays_and_queries_an_en_passant_capture_from_a_custom_position() {
        let mut tree = GameTree::from_fen("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1").unwrap();
        let new_id = tree.play_move(0, "d4", "e3", None).unwrap();
        assert_eq!(tree.node(new_id).unwrap().san.as_deref(), Some("dxe3"));
        assert!(!tree.legal_moves(new_id).unwrap().is_empty());

        let moves = vec!["dxe3".to_string()];
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        assert!(crate::explorer::explore(&conn, &moves, &crate::explorer::ExplorerFilters::default()).is_ok());
        assert!(crate::db::games_at_position(&conn, &moves, &crate::explorer::ExplorerFilters::default(), "", "", 20, 0).is_ok());
    }

    #[test]
    fn promote_variation_moves_the_whole_branch_to_the_front_at_every_fork_crossed() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        let d4 = tree.play_move(0, "d2", "d4", None).unwrap();
        let e5 = tree.play_move(e4, "e7", "e5", None).unwrap();
        let c5 = tree.play_move(e4, "c7", "c5", None).unwrap();
        assert_eq!(tree.node(0).unwrap().children, vec![e4, d4]);
        assert_eq!(tree.node(e4).unwrap().children, vec![e5, c5]);

        // variante profonde (c5, deuxieme enfant de e4 lui-meme deja premier
        // enfant de la racine) : ne doit reordonner que le niveau ou elle
        // n'est pas deja en tete
        tree.promote_variation(c5).unwrap();
        assert_eq!(tree.node(0).unwrap().children, vec![e4, d4]);
        assert_eq!(tree.node(e4).unwrap().children, vec![c5, e5]);

        // variante de premier coup (d4) : devient la ligne principale a la racine
        tree.promote_variation(d4).unwrap();
        assert_eq!(tree.node(0).unwrap().children, vec![d4, e4]);
    }

    #[test]
    fn promote_variation_on_the_root_fails() {
        let mut tree = GameTree::new();
        assert!(tree.promote_variation(0).is_err());
    }

    #[test]
    fn delete_variation_removes_the_branch_and_returns_its_parent() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        let d4 = tree.play_move(0, "d2", "d4", None).unwrap();

        let parent = tree.delete_variation(d4).unwrap();
        assert_eq!(parent, 0);
        assert_eq!(tree.node(0).unwrap().children, vec![e4]);
    }

    #[test]
    fn delete_variation_on_the_root_fails() {
        let mut tree = GameTree::new();
        assert!(tree.delete_variation(0).is_err());
    }
}
