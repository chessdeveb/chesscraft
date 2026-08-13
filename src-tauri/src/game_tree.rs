use serde::{Deserialize, Serialize};
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, EnPassantMode, Position, Role, Square};
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

    pub fn legal_moves(&self, node_id: u32) -> Result<Vec<MoveDto>, String> {
        let node = self.node(node_id).ok_or_else(|| "noeud introuvable".to_string())?;
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
        let node = self.node(node_id).ok_or_else(|| "noeud introuvable".to_string())?;
        let position = node.position.clone();

        let from_sq = Square::from_str(from).map_err(|_| "case de depart invalide".to_string())?;
        let to_sq = Square::from_str(to).map_err(|_| "case d'arrivee invalide".to_string())?;
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
            .map_err(|_| "coup illegal".to_string())?,
            Err(_) => return Err("coup illegal".to_string()),
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
            .map_err(|_| "coup illegal".to_string())?;

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
            .ok_or_else(|| "noeud introuvable".to_string())?;
        node.comment_before = before.filter(|s| !s.is_empty());
        node.comment_after = after.filter(|s| !s.is_empty());
        Ok(())
    }

    pub fn set_nags(&mut self, node_id: u32, nags: Vec<u8>) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "noeud introuvable".to_string())?;
        node.nags = nags;
        Ok(())
    }

    pub fn set_shapes(&mut self, node_id: u32, shapes: Vec<ShapeDto>) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "noeud introuvable".to_string())?;
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
