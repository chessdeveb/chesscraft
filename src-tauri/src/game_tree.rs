use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, EnPassantMode, Position, Role, Square};
use std::str::FromStr;

pub struct Node {
    pub id: u32,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub san: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub promotion: Option<String>,
    pub position: Chess,
}

pub struct GameTree {
    nodes: Vec<Node>,
}

#[derive(Serialize, Clone)]
pub struct MoveDto {
    pub from: String,
    pub to: String,
    pub promotion: Option<String>,
    pub san: String,
}

#[derive(Serialize, Clone)]
pub struct NodeDto {
    pub id: u32,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub san: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub fen: String,
    pub turn: String,
}

#[derive(Serialize, Clone)]
pub struct TreeDto {
    pub nodes: Vec<NodeDto>,
    pub root: u32,
}

fn role_to_str(role: Role) -> String {
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
        };
        GameTree { nodes: vec![root] }
    }

    fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
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
        });
        self.nodes[node_id as usize].children.push(new_id);

        Ok(new_id)
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
                })
                .collect(),
        }
    }
}
