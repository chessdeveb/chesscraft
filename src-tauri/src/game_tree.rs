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
    pub promotion: Option<String>,
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

/// Les annotations de pendule "[%clk ...]" d'un commentaire, telles quelles.
fn clock_tags(comment: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = comment;
    while let Some(start) = rest.find("[%clk") {
        let Some(len) = rest[start..].find(']') else { break };
        tags.push(rest[start..=start + len].to_string());
        rest = &rest[start + len + 1..];
    }
    tags
}

/// FEN de depart pour la position Chess960 (Fischer Random) numero `n`
/// (0..=959), selon le schema de numerotation standard (Scharnagl) --
/// utilise aussi par lichess/chess.com/FIDE, donc un numero donne toujours
/// la meme position d'un logiciel a l'autre. Droits de roque exprimes en
/// notation X-FEN classique ("KQkq") : le parseur FEN de shakmaty resout
/// deja K/Q vers la tour la plus exterieure de chaque cote, quel que soit
/// son fichier (voir shakmaty::fen), donc pas besoin de la notation
/// Shredder-FEN (lettres de colonne).
pub fn chess960_start_fen(n: u16) -> Result<String, String> {
    if n >= 960 {
        return Err(format!("numéro de position Chess960 invalide : {n} (attendu 0-959)"));
    }
    let mut rest = n;
    let b_light = (rest % 4) as usize;
    rest /= 4;
    let b_dark = (rest % 4) as usize;
    rest /= 4;
    let q = (rest % 6) as usize;
    rest /= 6;
    const KNIGHT_PAIRS: [(usize, usize); 10] =
        [(0, 1), (0, 2), (0, 3), (0, 4), (1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)];
    let (k1, k2) = KNIGHT_PAIRS[rest as usize];

    const LIGHT_FILES: [usize; 4] = [1, 3, 5, 7]; // b, d, f, h (cases claires sur la 1ere rangee)
    const DARK_FILES: [usize; 4] = [0, 2, 4, 6]; // a, c, e, g (cases sombres)
    let mut squares: [Option<char>; 8] = [None; 8];
    squares[LIGHT_FILES[b_light]] = Some('b');
    squares[DARK_FILES[b_dark]] = Some('b');

    let empty: Vec<usize> = (0..8).filter(|&i| squares[i].is_none()).collect();
    squares[empty[q]] = Some('q');

    let empty: Vec<usize> = (0..8).filter(|&i| squares[i].is_none()).collect();
    squares[empty[k1]] = Some('n');
    squares[empty[k2]] = Some('n');

    // 3 cases restantes, triees par colonne croissante : Tour, Roi, Tour --
    // seul ordre possible puisque le roi doit se trouver entre les deux tours.
    let empty: Vec<usize> = (0..8).filter(|&i| squares[i].is_none()).collect();
    squares[empty[0]] = Some('r');
    squares[empty[1]] = Some('k');
    squares[empty[2]] = Some('r');

    let black_rank: String = squares.iter().map(|c| c.expect("8 cases toutes remplies")).collect();
    let white_rank = black_rank.to_uppercase();
    Ok(format!("{black_rank}/pppppppp/8/8/8/8/PPPPPPPP/{white_rank} w KQkq - 0 1"))
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
        // detect() plutot que Standard fige : une position Chess960 (roi hors
        // colonne e, ou tours hors colonnes a/h) a des droits de roque valides
        // uniquement sous ce mode -- voir chess960_start_fen. Une position
        // classique detecte toujours Standard, donc ne change rien au
        // comportement existant.
        let mode = CastlingMode::detect(setup.as_setup());
        let position: Chess = setup
            .into_position(mode)
            .map_err(|e| format!("Position illégale : {e}"))?;
        // Forme canonique (position_fen), pas le texte `fen` recu tel quel :
        // un FEN Chess960 en notation Shredder ("HBhb", lettres de colonne des
        // tours) se round-trippe en X-FEN standard ("KQkq") des que shakmaty
        // le reecrit (voir Fen::from_position, meme constat que
        // pgn::parse_game_at_starts_from_a_chess960_fen_tag_with_shredder_castling) --
        // stocker la forme brute laisserait le tag FEN et node.fen (calcule
        // via position_fen partout ailleurs, voir to_dto) diverger
        // textuellement pour la MEME position, cassant toute comparaison
        // ulterieure (db::normalize_start_fen, explorateur).
        let canonical_fen = position_fen(&position);
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
        let mut tags = default_tags();
        // Sans ce tag, une partie a depart personnalise (Nouvelle position,
        // Chess960) perdait sa position de depart des la sauvegarde : ni le
        // PGN exporte (voir write_game, qui n'ecrit QUE les tags presents
        // ici) ni la colonne start_fen (voir db::save_game, qui lit ce meme
        // tag) ne savaient plus qu'elle existait -- la reouverture rejouait
        // alors les coups depuis la position standard, la plupart du temps
        // illegaux (silencieusement ignores, voir pgn::GameVisitor::san) et
        // l'explorateur ne pouvait jamais la retrouver (son hash Zobrist,
        // recalcule depuis la position standard, ne correspondait plus a
        // rien). SetUp "1" est la
        // convention PGN habituelle qui accompagne un tag FEN (jamais
        // verifiee par cette appli, voir GameVisitor::begin_movetext, mais
        // attendue par d'autres logiciels PGN).
        tags.insert("FEN".to_string(), canonical_fen);
        tags.insert("SetUp".to_string(), "1".to_string());
        Ok(GameTree { nodes: vec![root], tags })
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

    /// Comme play_move, mais purement a partir d'un FEN, sans GameTree ni
    /// mutation : utilise par le mode entrainement (judge_training_move,
    /// lib.rs) pour essayer un coup de l'utilisateur sans jamais alterer la
    /// partie reelle (pas question d'inserer chaque essai, bon ou mauvais,
    /// comme variation dans la partie enregistree). Duplique volontairement
    /// la resolution du coup de play_move (meme repli promotion en dame par
    /// defaut) plutot que de refactoriser cette derniere : play_move reste
    /// une fonction coeur deja eprouvee, pas de raison de la faire dependre
    /// d'un chemin FEN-texte supplementaire pour un besoin a part.
    pub fn apply_move_to_fen(fen: &str, from: &str, to: &str, promotion: Option<&str>) -> Result<(String, String), String> {
        let setup = Fen::from_ascii(fen.as_bytes()).map_err(|_| "FEN invalide".to_string())?;
        let position: Chess = setup.into_position(CastlingMode::Standard).map_err(|_| "position invalide".to_string())?;

        let from_sq = Square::from_str(from).map_err(|_| "case de départ invalide".to_string())?;
        let to_sq = Square::from_str(to).map_err(|_| "case d'arrivée invalide".to_string())?;
        let requested_promotion = match promotion {
            Some(p) => Some(str_to_role(p).ok_or_else(|| "promotion invalide".to_string())?),
            None => None,
        };

        let attempt = UciMove::Normal { from: from_sq, to: to_sq, promotion: requested_promotion }.to_move(&position);
        let chosen = match attempt {
            Ok(m) => m,
            Err(_) if requested_promotion.is_none() => UciMove::Normal { from: from_sq, to: to_sq, promotion: Some(Role::Queen) }
                .to_move(&position)
                .map_err(|_| "coup illégal".to_string())?,
            Err(_) => return Err("coup illégal".to_string()),
        };

        let san = San::from_move(&position, chosen.clone()).to_string();
        let new_position = position.play(chosen).map_err(|_| "coup illégal".to_string())?;
        Ok((position_fen(&new_position), san))
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

    /// Retire tout ce qui suit `node_id` (suite de la ligne ET variantes
    /// partant de ce coup) : il devient le dernier coup de sa ligne. Bouton
    /// "supprimer les coups suivants" du panneau Notation. Comme pour
    /// delete_variation, les noeuds detaches restent dans self.nodes mais ne
    /// sont plus atteignables depuis la racine.
    pub fn delete_following_moves(&mut self, node_id: u32) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id as usize)
            .ok_or_else(|| "nœud introuvable".to_string())?;
        node.children.clear();
        Ok(())
    }

    /// Retire tout ce qui precede `node_id` : la partie commence desormais a
    /// la position atteinte apres ce coup. Bouton "supprimer les coups
    /// precedents" du panneau Notation.
    ///
    /// La racine doit rester le noeud 0 (voir to_dto, mainline_sans,
    /// pgn::write_game...) et l'id d'un noeud son indice dans self.nodes :
    /// l'arbre est donc reconstruit a partir du sous-arbre de `node_id`,
    /// renumerote dans l'ordre de parcours. La nouvelle racine reprend la
    /// position du coup, son commentaire "apres" (il decrit la position
    /// atteinte, pas le coup) et ses fleches ; le coup lui-meme, son
    /// commentaire "avant" et ses annotations disparaissent avec ce qui
    /// precede. Elle porte le NAG $220 (diagramme) : la notation affiche a la
    /// place des coups supprimes un echiquier de la position de depart (voir
    /// Notation.tsx). Les tags FEN/SetUp suivent (meme raison que from_fen),
    /// et avec eux le trait et le numero du premier coup restant.
    pub fn delete_previous_moves(&mut self, node_id: u32) -> Result<(), String> {
        const DIAGRAM_NAG: u8 = 220;
        let start = self.node(node_id).ok_or_else(|| "nœud introuvable".to_string())?;
        if start.parent.is_none() {
            return Err("aucun coup précédent à supprimer".to_string());
        }

        // parcours en largeur du sous-arbre : anciens ids dans leur nouvel ordre
        let mut order: Vec<u32> = vec![node_id];
        let mut index = 0;
        while index < order.len() {
            let children = self.nodes[order[index] as usize].children.clone();
            order.extend(children);
            index += 1;
        }
        let new_id: std::collections::HashMap<u32, u32> =
            order.iter().enumerate().map(|(i, &old)| (old, i as u32)).collect();

        let mut old_nodes: Vec<Option<Node>> = std::mem::take(&mut self.nodes).into_iter().map(Some).collect();
        let mut nodes: Vec<Node> = Vec::with_capacity(order.len());
        for (i, &old_id) in order.iter().enumerate() {
            let mut node = old_nodes[old_id as usize].take().expect("chaque noeud n'est visite qu'une fois");
            node.id = i as u32;
            node.children = node.children.iter().map(|c| new_id[c]).collect();
            if i == 0 {
                node.parent = None;
                node.san = None;
                node.from = None;
                node.to = None;
                node.promotion = None;
                node.comment_before = None;
                node.nags = vec![DIAGRAM_NAG];
            } else {
                node.parent = node.parent.map(|p| new_id[&p]);
            }
            nodes.push(node);
        }

        self.tags.insert("FEN".to_string(), position_fen(&nodes[0].position));
        self.tags.insert("SetUp".to_string(), "1".to_string());
        self.nodes = nodes;
        Ok(())
    }

    /// Retire toutes les annotations de la partie, en gardant tous les coups
    /// et toutes les variantes : commentaires, symboles (NAG) et fleches /
    /// cases colorees. Bouton "supprimer toutes les annotations" du panneau
    /// Notation. Deux choses restent, parce que ce ne sont pas des
    /// annotations de l'utilisateur :
    /// - les temps de pendule ("[%clk ...]", ranges dans les commentaires
    ///   par lichess/chess.com, voir Notation.tsx::parseClock) ;
    /// - le diagramme de depart de la racine, qui tient lieu des coups
    ///   retires par delete_previous_moves.
    pub fn clear_annotations(&mut self) {
        for node in &mut self.nodes {
            let clocks = [node.comment_before.take(), node.comment_after.take()]
                .iter()
                .flatten()
                .flat_map(|comment| clock_tags(comment))
                .collect::<Vec<String>>()
                .join(" ");
            node.comment_after = if clocks.is_empty() { None } else { Some(clocks) };
            if node.parent.is_none() {
                node.nags.retain(|nag| matches!(*nag, 220 | 221));
            } else {
                node.nags.clear();
            }
            node.shapes.clear();
        }
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
                    promotion: n.promotion.clone(),
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

    // apply_move_to_fen (mode entrainement, voir judge_training_move dans
    // lib.rs) duplique volontairement la resolution de coup de play_move :
    // ces tests verifient qu'elle se comporte de facon identique (meme
    // repli promotion en dame par defaut, meme gestion du roque) sans passer
    // par un GameTree.
    #[test]
    fn apply_move_to_fen_plays_a_normal_move_and_returns_the_resulting_fen_and_san() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let (fen, san) = GameTree::apply_move_to_fen(start, "e2", "e4", None).unwrap();
        assert_eq!(san, "e4");
        // pas de case en passant dans le FEN : EnPassantMode::Legal (voir
        // position_fen) ne la mentionne que si une capture en passant est
        // reellement jouable pour l'adversaire, pas juste des qu'un pion a
        // avance de deux cases -- ici aucun pion noir n'est adjacent a e4.
        assert_eq!(fen, "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    }

    #[test]
    fn apply_move_to_fen_castles_kingside_from_e1g1() {
        // position de reference pour les droits de roque (chemins degages
        // des deux cotes) : peu importe qu'elle soit atteignable par une
        // vraie partie, seule sa legalite compte pour from_fen/apply_move_to_fen.
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        let (new_fen, san) = GameTree::apply_move_to_fen(fen, "e1", "g1", None).unwrap();
        assert_eq!(san, "O-O");
        assert!(new_fen.starts_with("r3k2r/8/8/8/8/8/8/R4RK1"));
    }

    #[test]
    fn apply_move_to_fen_defaults_to_queen_promotion_when_unspecified() {
        let fen = "8/4P1k1/8/8/8/8/6K1/8 w - - 0 1";
        let (_, san) = GameTree::apply_move_to_fen(fen, "e7", "e8", None).unwrap();
        assert_eq!(san, "e8=Q");
    }

    #[test]
    fn apply_move_to_fen_honours_an_explicit_underpromotion() {
        let fen = "8/4P1k1/8/8/8/8/6K1/8 w - - 0 1";
        let (_, san) = GameTree::apply_move_to_fen(fen, "e7", "e8", Some("n")).unwrap();
        assert_eq!(san, "e8=N");
    }

    #[test]
    fn apply_move_to_fen_rejects_an_illegal_move() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert!(GameTree::apply_move_to_fen(start, "e2", "e5", None).is_err());
    }

    #[test]
    fn apply_move_to_fen_never_mutates_a_game_tree() {
        // le point central du mode entrainement : essayer un coup depuis un
        // FEN n'a besoin d'aucun GameTree et ne peut donc jamais s'inserer
        // comme variation dans une partie reelle.
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let mut tree = GameTree::from_fen(start).unwrap();
        let before = tree.to_dto().nodes.len();
        let _ = GameTree::apply_move_to_fen(start, "e2", "e4", None).unwrap();
        assert_eq!(tree.to_dto().nodes.len(), before);
        // le tree est toujours jouable normalement ensuite (pas de borrow
        // conserve a tort par apply_move_to_fen).
        tree.play_move(0, "e2", "e4", None).unwrap();
        assert_eq!(tree.to_dto().nodes.len(), before + 1);
    }

    #[test]
    fn chess960_start_fen_rejects_out_of_range_numbers() {
        assert!(chess960_start_fen(960).is_err());
    }

    #[test]
    fn chess960_start_fen_position_0_is_bbqnnrkr() {
        // position #0 du schema de numerotation standard (Scharnagl), valeur
        // de reference connue -- meme position que sur lichess.org/chess.com.
        let fen = chess960_start_fen(0).unwrap();
        assert_eq!(fen, "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1");
    }

    #[test]
    fn chess960_start_fen_position_518_is_the_standard_start() {
        // position #518 du meme schema correspond a la partie classique --
        // verifie que le generateur retombe bien sur l'ordinaire au bon numero.
        let fen = chess960_start_fen(518).unwrap();
        assert_eq!(fen, "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    }

    #[test]
    fn chess960_start_fen_builds_into_a_legal_tree() {
        // Roi/tours hors des colonnes e/a/h : ne doit pas etre rejete par
        // from_fen (detect() doit y reconnaitre Chess960, pas echouer comme
        // le ferait CastlingMode::Standard fige).
        let fen = chess960_start_fen(0).unwrap();
        let tree = GameTree::from_fen(&fen).unwrap();
        let dto = tree.to_dto();
        assert_eq!(dto.nodes[0].fen, fen);
        assert!(tree.legal_moves(0).is_ok());
    }

    #[test]
    fn from_fen_rejects_an_illegal_position() {
        // aucun roi noir : position illegale, pas seulement mal ecrite
        assert!(GameTree::from_fen("8/8/8/8/8/8/8/4K3 w - - 0 1").is_err());
    }

    // Cas reel qui a motive le passage a une recherche par hash Zobrist :
    // une capture en passant depuis une position personnalisee (editeur de
    // position) produit un premier coup ("dxe3") qui n'a de sens pour aucune
    // vraie partie demarree normalement -- verifie ici que jouer ce coup et
    // interroger l'explorateur dessus (a partir du depart standard, donc
    // sans aucun sens) ne panique pas : replay_position echoue proprement
    // (coup illegal depuis cette position), explore() renvoie une liste
    // vide et current_zobrist renvoie None plutot que de planter.
    #[test]
    fn plays_and_queries_an_en_passant_capture_from_a_custom_position() {
        let mut tree = GameTree::from_fen("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1").unwrap();
        let new_id = tree.play_move(0, "d4", "e3", None).unwrap();
        assert_eq!(tree.node(new_id).unwrap().san.as_deref(), Some("dxe3"));
        assert!(!tree.legal_moves(new_id).unwrap().is_empty());

        let moves = vec!["dxe3".to_string()];
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        assert!(crate::explorer::explore(&conn, &moves, &crate::explorer::ExplorerFilters::default(), None).unwrap().is_empty());
        assert_eq!(crate::explorer::current_zobrist(&moves, None), None);
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

    #[test]
    fn delete_following_moves_cuts_the_line_and_its_variations_after_the_move() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        let d4 = tree.play_move(0, "d2", "d4", None).unwrap();
        let e5 = tree.play_move(e4, "e7", "e5", None).unwrap();
        tree.play_move(e4, "c7", "c5", None).unwrap();
        tree.play_move(e5, "g1", "f3", None).unwrap();

        tree.delete_following_moves(e4).unwrap();
        assert!(tree.node(e4).unwrap().children.is_empty());
        // le coup lui-meme et les variantes qui le precedent restent
        assert_eq!(tree.node(0).unwrap().children, vec![e4, d4]);
        assert_eq!(tree.mainline_sans(), vec!["e4".to_string()]);
    }

    #[test]
    fn delete_previous_moves_restarts_the_game_from_the_position_after_the_move() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        tree.play_move(0, "d2", "d4", None).unwrap();
        let e5 = tree.play_move(e4, "e7", "e5", None).unwrap();
        let nf3 = tree.play_move(e5, "g1", "f3", None).unwrap();
        let nc3 = tree.play_move(e5, "b1", "c3", None).unwrap();
        let nc6 = tree.play_move(nf3, "b8", "c6", None).unwrap();
        tree.set_comment(e5, Some("avant".to_string()), Some("position critique".to_string())).unwrap();
        tree.set_nags(e5, vec![1]).unwrap();
        tree.set_comment(nc6, None, Some("fin".to_string())).unwrap();
        let fen_after_e5 = tree.node_fen(e5).unwrap();
        let _ = nc3;

        tree.delete_previous_moves(e5).unwrap();

        // nouvelle racine : la position apres 1...e5, sans coup, avec le
        // commentaire qui decrivait cette position et le diagramme
        let root = tree.node(0).unwrap();
        assert_eq!(root.parent, None);
        assert_eq!(root.san, None);
        assert_eq!(root.comment_before, None);
        assert_eq!(root.comment_after.as_deref(), Some("position critique"));
        assert_eq!(root.nags, vec![220]);
        assert_eq!(tree.node_fen(0).unwrap(), fen_after_e5);
        assert_eq!(tree.tags.get("FEN"), Some(&fen_after_e5));
        assert_eq!(tree.tags.get("SetUp").map(|s| s.as_str()), Some("1"));

        // la suite et ses variantes sont conservees, renumerotees depuis 0
        assert_eq!(tree.mainline_sans(), vec!["Nf3".to_string(), "Nc6".to_string()]);
        let children = root.children.clone();
        assert_eq!(children.len(), 2);
        assert_eq!(tree.node(children[1]).unwrap().san.as_deref(), Some("Nc3"));
        for (index, id) in tree.mainline_node_ids().into_iter().enumerate() {
            let node = tree.node(id).unwrap();
            assert_eq!(node.id, id);
            if index > 0 {
                assert!(tree.node(node.parent.unwrap()).unwrap().children.contains(&id));
            }
        }
        let last = *tree.mainline_node_ids().last().unwrap();
        assert_eq!(tree.node(last).unwrap().comment_after.as_deref(), Some("fin"));

        // l'arbre reste jouable et s'exporte avec la bonne numerotation
        let reply = tree.play_move(last, "f1", "b5", None).unwrap();
        assert_eq!(tree.node(reply).unwrap().san.as_deref(), Some("Bb5"));
        let pgn = crate::pgn::write_game(&tree);
        assert!(pgn.contains("2. Nf3"), "{pgn}");
        let reparsed = crate::pgn::parse_game(&pgn).unwrap();
        assert_eq!(reparsed.mainline_sans(), tree.mainline_sans());
        assert_eq!(reparsed.node(0).unwrap().nags, vec![220], "le diagramme de depart survit a l'enregistrement : {pgn}");
        assert_eq!(reparsed.node(0).unwrap().comment_after.as_deref(), Some("position critique"));
    }

    #[test]
    fn clear_annotations_keeps_every_move_and_variation_but_no_comment_nag_or_shape() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        let d4 = tree.play_move(0, "d2", "d4", None).unwrap();
        let e5 = tree.play_move(e4, "e7", "e5", None).unwrap();
        tree.set_comment(0, None, Some("introduction".to_string())).unwrap();
        tree.set_comment(e4, Some("avant".to_string()), Some("[%clk 1:29:55] le meilleur".to_string())).unwrap();
        tree.set_comment(d4, None, Some("variante".to_string())).unwrap();
        tree.set_nags(e4, vec![1, 14]).unwrap();
        tree.set_nags(e5, vec![220]).unwrap();
        tree.set_shapes(e5, vec![ShapeDto { orig: "e5".to_string(), dest: None, brush: "green".to_string() }]).unwrap();

        tree.clear_annotations();

        assert_eq!(tree.node(0).unwrap().children, vec![e4, d4]);
        assert_eq!(tree.mainline_sans(), vec!["e4".to_string(), "e5".to_string()]);
        for id in [0, e4, d4, e5] {
            let node = tree.node(id).unwrap();
            assert_eq!(node.comment_before, None);
            assert!(node.nags.is_empty());
            assert!(node.shapes.is_empty());
        }
        assert_eq!(tree.node(0).unwrap().comment_after, None);
        assert_eq!(tree.node(d4).unwrap().comment_after, None);
        // le temps de pendule reste, sans le texte qui l'accompagnait
        assert_eq!(tree.node(e4).unwrap().comment_after.as_deref(), Some("[%clk 1:29:55]"));
    }

    #[test]
    fn clear_annotations_keeps_the_start_diagram_left_by_delete_previous_moves() {
        let mut tree = GameTree::new();
        let e4 = tree.play_move(0, "e2", "e4", None).unwrap();
        let e5 = tree.play_move(e4, "e7", "e5", None).unwrap();
        tree.set_comment(e4, None, Some("position critique".to_string())).unwrap();
        tree.delete_previous_moves(e4).unwrap();
        let _ = e5;

        tree.clear_annotations();

        assert_eq!(tree.node(0).unwrap().nags, vec![220]);
        assert_eq!(tree.node(0).unwrap().comment_after, None);
        assert_eq!(tree.mainline_sans(), vec!["e5".to_string()]);
    }

    #[test]
    fn delete_previous_moves_on_the_root_fails() {
        let mut tree = GameTree::new();
        tree.play_move(0, "e2", "e4", None).unwrap();
        assert!(tree.delete_previous_moves(0).is_err());
        assert_eq!(tree.mainline_sans(), vec!["e4".to_string()]);
    }
}
