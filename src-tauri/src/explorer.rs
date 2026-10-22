use rusqlite::{params, Connection, ToSql};
use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::zobrist::Zobrist64;
use shakmaty::{CastlingMode, Chess, EnPassantMode, Position};
use std::collections::{BTreeMap, HashMap};

/// Restreint l'explorateur aux parties d'un seul joueur, dans une seule
/// couleur (preparation d'adversaire : voir lib.rs::open_player_prep_tab).
#[derive(Clone)]
pub struct PlayerFilter {
    pub name: String,
    pub white: bool,
    /// true (preparation d'adversaire, voir lib.rs::open_player_prep_tab) :
    /// egalite exacte, `name` vient d'un joueur deja resolu dans player_stats.
    /// false (filtre ad-hoc du panneau explorateur, saisi librement au
    /// clavier) : prefixe insensible a la casse, comme partout ailleurs dans
    /// l'appli ou l'utilisateur tape un nom (recherche de parties, champs
    /// Blancs/Noirs...) -- une egalite exacte y renvoyait "aucune partie"
    /// pour a peu pres n'importe quelle saisie (le nom stocke est rarement
    /// tape caractere pour caractere en entier).
    pub exact: bool,
}

impl PlayerFilter {
    /// Fragment de condition SQL pour ce filtre (egalite exacte ou prefixe
    /// insensible a la casse, voir `exact`), poussant sur `params` les valeurs
    /// necessaires (une pour l'egalite, deux -- bornes -- pour le prefixe) et
    /// referencant leurs `?N` d'apres la position qu'elles y prennent.
    /// Partagee par explorer::stats_at_zobrist et db::zobrist_from_and_where,
    /// qui construisent chacune leur propre requete autour.
    pub fn where_clause(&self, params: &mut Vec<Box<dyn ToSql>>) -> String {
        let column = if self.white { "white" } else { "black" };
        if self.exact {
            params.push(Box::new(self.name.clone()));
            format!("{column} = ?{}", params.len())
        } else {
            let lo = self.name.clone();
            let hi = format!("{lo}\u{ffff}");
            params.push(Box::new(lo));
            params.push(Box::new(hi));
            format!("{column} COLLATE NOCASE >= ?{} AND {column} COLLATE NOCASE < ?{}", params.len() - 1, params.len())
        }
    }
}

/// Filtres combinables de l'explorateur d'ouvertures (panneau principal, pas
/// la preparation d'adversaire qui a son propre PlayerFilter dedie -- voir
/// plus haut -- mais peut aussi en fournir un ici si l'onglet est en mode
/// preparation, voir lib.rs::opening_explorer).
#[derive(Clone, Default)]
pub struct ExplorerFilters {
    pub player: Option<PlayerFilter>,
    /// Classement (Elo standard) : ne garde que les parties ou LES DEUX
    /// joueurs sont dans l'intervalle -- une partie ou un seul camp est
    /// classe dans la fourchette demandee n'est pas ce qu'on cherche quand on
    /// etudie "les parties entre joueurs de ce niveau".
    pub rating_range: Option<(i32, i32)>,
    /// Annee de la partie (date_played, format "AAAA.MM.JJ" ou partiellement
    /// inconnu, ex. "2015.??.??") : la comparaison lexicographique existante
    /// sur cette colonne (voir idx_games_date) gere deja correctement les
    /// dates partiellement inconnues (une annee connue avec mois/jour "??"
    /// reste incluse ; une date entierement inconnue "????.??.??" trie apres
    /// toute annee reelle et est donc naturellement exclue de tout intervalle).
    pub year_range: Option<(i32, i32)>,
}

impl ExplorerFilters {
    pub fn is_empty(&self) -> bool {
        self.player.is_none() && self.rating_range.is_none() && self.year_range.is_none()
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpeningMoveStat {
    pub san: String,
    pub games: u32,
    pub white_wins: u32,
    pub draws: u32,
    pub black_wins: u32,
}

#[derive(Default)]
struct Tally {
    games: u32,
    white_wins: u32,
    draws: u32,
    black_wins: u32,
}

/// Le "+"/"#" de fin de coup (echec/mat), quand il est present dans la
/// donnee stockee, est retire ici avant de grouper : tant que le nettoyage
/// db::strip_check_suffixes n'est pas passe sur toute la base, une meme
/// suite de coups peut coexister sous deux formes ("Qxf7#" et "Qxf7") selon
/// que la partie source a deja ete nettoyee -- sans ce retrait elles
/// compteraient comme deux coups distincts. Le suffixe correct est
/// recalcule ensuite dans explore() en rejouant reellement la position
/// (voir annotate_check_suffixes), plutot que de faire confiance a celui
/// (potentiellement absent/incoherent) de la donnee source.
fn tally_rows_to_stats(rows: impl Iterator<Item = rusqlite::Result<(String, Option<String>, u32)>>) -> Result<Vec<OpeningMoveStat>, String> {
    let mut stats: BTreeMap<String, Tally> = BTreeMap::new();
    for row in rows {
        let (san, result, cnt) = row.map_err(|e| e.to_string())?;
        let san = san.trim_end_matches(['+', '#']).to_string();
        if san.is_empty() {
            continue;
        }
        let entry = stats.entry(san).or_default();
        entry.games += cnt;
        match result.as_deref() {
            Some("1-0") => entry.white_wins += cnt,
            Some("1/2-1/2") => entry.draws += cnt,
            Some("0-1") => entry.black_wins += cnt,
            _ => {}
        }
    }
    let mut out: Vec<OpeningMoveStat> = stats
        .into_iter()
        .map(|(san, tally)| OpeningMoveStat {
            san,
            games: tally.games,
            white_wins: tally.white_wins,
            draws: tally.draws,
            black_wins: tally.black_wins,
        })
        .collect();
    out.sort_by(|a, b| b.games.cmp(&a.games));
    Ok(out)
}

/// Position atteinte en rejouant `moves` (SAN sans suffixe, tel qu'envoye
/// par le frontend) depuis `start_fen` (None = position de depart standard,
/// Some(fen) = Nouvelle position/Chess960, voir db::normalize_start_fen).
/// None si `start_fen` est invalide ou si un coup ne se rejoue pas (ne
/// devrait pas arriver avec des donnees internes a l'appli, mais mieux vaut
/// renvoyer "aucun resultat" que de planter l'explorateur pour ca).
fn replay_position(moves: &[String], start_fen: Option<&str>) -> Option<Chess> {
    let mut position = match start_fen {
        Some(fen) => {
            let setup: Fen = fen.trim().parse().ok()?;
            let mode = CastlingMode::detect(setup.as_setup());
            setup.into_position(mode).ok()?
        }
        None => Chess::default(),
    };
    for m in moves {
        let san: San = m.parse().ok()?;
        let mv = san.to_move(&position).ok()?;
        position = position.play(mv).ok()?;
    }
    Some(position)
}

/// Empreinte (hash de Zobrist, 64 bits) d'une position -- identifie
/// l'ARRANGEMENT DU PLATEAU (pieces, trait, roque, prise en passant
/// legale), independamment de la maniere dont on y est arrive : deux
/// parties qui transposent (meme position par un ordre de coups different)
/// ou qui ne partagent meme pas la meme position de depart (Chess960,
/// Nouvelle position) mais se rejoignent plus tard sur exactement la meme
/// disposition obtiennent la MEME empreinte. C'est ce qui remplace l'ancien
/// systeme (comparaison du texte des coups depuis un prefixe commun,
/// aveugle aux transpositions -- voir l'historique de ce fichier) : voir
/// position_index (db.rs), index_game/rebuild_position_index plus bas.
///
/// Converti en i64 (SQLite n'a pas d'entier 64 bits non signe) : seule
/// l'egalite bit a bit compte pour un hash, jamais d'arithmetique dessus, la
/// conversion est donc strictement une reinterpretation sans perte.
fn zobrist_of(position: &Chess) -> i64 {
    position.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0 as i64
}

/// Rejoue `mainline_sans` (SAN complet, "+"/"#" eventuels tolerés -- donnee
/// source potentiellement anterieure au nettoyage db::strip_check_suffixes)
/// depuis `start_fen` et retourne, pour chaque position traversee (racine
/// incluse), son empreinte et le coup joue ensuite (None a la derniere
/// position, ou toute position atteinte apres un coup illisible/illegal --
/// s'arrete la plutot que de planter). C'est le coeur de l'indexation :
/// chaque ligne produite ici devient une ligne de position_index.
fn positions_for_mainline(mainline_sans: &str, start_fen: Option<&str>) -> Vec<(i64, Option<String>)> {
    let mut position = match start_fen {
        Some(fen) => match fen.trim().parse::<Fen>() {
            Ok(setup) => {
                let mode = CastlingMode::detect(setup.as_setup());
                match setup.into_position(mode) {
                    Ok(pos) => pos,
                    Err(_) => return Vec::new(),
                }
            }
            Err(_) => return Vec::new(),
        },
        None => Chess::default(),
    };
    let tokens: Vec<&str> = if mainline_sans.is_empty() { Vec::new() } else { mainline_sans.split(' ').collect() };
    let mut out = Vec::with_capacity(tokens.len() + 1);
    for token in tokens {
        let clean = token.trim_end_matches(['+', '#']);
        let Ok(san) = clean.parse::<San>() else { break };
        let Ok(mv) = san.to_move(&position) else { break };
        out.push((zobrist_of(&position), Some(clean.to_string())));
        let Ok(next) = position.clone().play(mv) else { break };
        position = next;
    }
    out.push((zobrist_of(&position), None));
    out
}

/// Tables de l'index de position, creees a l'ouverture de toute base (vides,
/// instantane) :
/// - `positions` : UNE ligne par (position, partie), table WITHOUT ROWID triee
///   par (zobrist, game_id) -- elle EST l'index (pas de table + index
///   couvrant, qui dupliquaient les donnees : 100 Go sur 10 M de parties).
///   Toutes les requetes de l'explorateur se font sur un intervalle de cette
///   cle : stats, compteur et liste (les plus recentes d'abord = parcours a
///   l'envers de game_id). `next_id` = coup joue ensuite (NULL a la derniere
///   position), `result` = code du resultat (voir result_code). Une position
///   repetee dans une meme partie n'est comptee qu'une fois (premiere
///   occurrence).
/// - `san_dict` : dictionnaire coup SAN -> entier, pour ne pas repeter le
///   texte du coup sur des centaines de millions de lignes.
pub const POSITION_TABLES_SQL: &str = "
    CREATE TABLE IF NOT EXISTS san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS positions (
        zobrist INTEGER NOT NULL,
        game_id INTEGER NOT NULL,
        next_id INTEGER,
        result INTEGER NOT NULL,
        PRIMARY KEY (zobrist, game_id)
    ) WITHOUT ROWID;";

/// Code compact du resultat stocke dans positions.result : 1 = 1-0, 2 = 0-1,
/// 3 = nulle, 0 = inconnu/en cours ("*", absent, vide).
fn result_code(result: Option<&str>) -> i64 {
    match result {
        Some("1-0") => 1,
        Some("0-1") => 2,
        Some("1/2-1/2") => 3,
        _ => 0,
    }
}

fn result_from_code(code: i64) -> Option<&'static str> {
    match code {
        1 => Some("1-0"),
        2 => Some("0-1"),
        3 => Some("1/2-1/2"),
        _ => None,
    }
}

/// Cache local du dictionnaire san_dict (coup -> identifiant).
#[derive(Default)]
struct SanIds {
    ids: HashMap<String, i64>,
}

impl SanIds {
    fn load(conn: &Connection) -> Result<Self, String> {
        let mut stmt = conn.prepare("SELECT san, id FROM san_dict").map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(SanIds { ids })
    }

    fn id(&mut self, conn: &Connection, san: &str) -> Result<i64, String> {
        if let Some(id) = self.ids.get(san) {
            return Ok(*id);
        }
        conn.execute("INSERT OR IGNORE INTO san_dict (san) VALUES (?1)", params![san]).map_err(|e| e.to_string())?;
        let id: i64 = conn.query_row("SELECT id FROM san_dict WHERE san = ?1", params![san], |row| row.get(0)).map_err(|e| e.to_string())?;
        self.ids.insert(san.to_string(), id);
        Ok(id)
    }
}

/// Retire de `positions` les lignes d'une partie, a appeler quand elle est
/// supprimee ou que son contenu change -- avec son ANCIEN contenu
/// (mainline_sans/start_fen tels qu'ils etaient avant modification) : la table
/// est triee par position, on retrouve donc les lignes en rejouant l'ancienne
/// partie plutot qu'avec un index supplementaire sur game_id (8 Go de plus).
pub fn deindex_game(conn: &Connection, game_id: i64, mainline_sans: &str, start_fen: Option<&str>) -> Result<(), String> {
    let mut stmt = conn.prepare_cached("DELETE FROM positions WHERE zobrist = ?1 AND game_id = ?2").map_err(|e| e.to_string())?;
    for (zobrist, _) in positions_for_mainline(mainline_sans, start_fen) {
        stmt.execute(params![zobrist, game_id]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Indexe une partie dans `positions` : a appeler juste apres l'avoir
/// enregistree/mise a jour (voir db::save_game/update_game), avec exactement
/// les memes mainline_sans/start_fen/result que ceux venant d'y etre ecrits.
/// Pour une partie deja indexee dont le contenu change, appeler d'abord
/// deindex_game avec l'ancien contenu.
pub fn index_game(conn: &Connection, game_id: i64, mainline_sans: &str, start_fen: Option<&str>, result: Option<&str>) -> Result<(), String> {
    let code = result_code(result);
    let mut ids = SanIds::default();
    let mut stmt = conn
        .prepare_cached("INSERT OR IGNORE INTO positions (zobrist, game_id, next_id, result) VALUES (?1, ?2, ?3, ?4)")
        .map_err(|e| e.to_string())?;
    for (zobrist, next_san) in positions_for_mainline(mainline_sans, start_fen) {
        let next_id = match next_san {
            Some(san) => Some(ids.id(conn, &san)?),
            None => None,
        };
        stmt.execute(params![zobrist, game_id, next_id, code]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Progression d'un rebuild_* : lecture (parties rejouees) ou ecriture. Une
/// ecriture avec `total` = 0 signale une etape longue sans avancement
/// mesurable (un seul ordre SQL, ex. le tri final de l'index) : l'interface
/// affiche alors un message "en cours" plutot qu'un compteur bloque a 0.
#[derive(Serialize, Clone, Copy)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum RebuildProgress {
    Reading { examined: u64 },
    Writing { written: u64, total: u64 },
}

const REBUILD_POSITION_INDEX_PAGE_SIZE: i64 = 10_000;

type ReplayedGame = (i64, i64, Vec<(i64, Option<String>)>);

/// Rejoue une page de parties sur tous les coeurs disponibles : le rejeu
/// (analyse SAN + generation des coups legaux) est la partie CPU du
/// rebuild, l'ecriture SQLite, elle, reste mono-thread dans l'appelant.
fn replay_page(page: &[(i64, String, Option<String>, Option<String>)]) -> Vec<ReplayedGame> {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let chunk_size = page.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = page
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter(|(_, mainline, _, _)| !mainline.is_empty())
                        .map(|(id, mainline, start_fen, result)| {
                            (*id, result_code(result.as_deref()), positions_for_mainline(mainline, start_fen.as_deref()))
                        })
                        .collect::<Vec<ReplayedGame>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("thread de rejeu")).collect()
    })
}

/// Reconstruit entierement `positions` a partir des parties actuelles :
/// necessaire pour une base qui existait avant l'ajout de cette table (voir
/// "Recalculer les stats de l'explorateur", menu Outils), ou apres une
/// modification massive faite hors de l'appli. PAS appelee automatiquement
/// a l'ouverture d'une base -- rejouer chaque partie avec les vraies regles
/// d'echecs est couteux, et une premiere version de ce genre de rattrapage,
/// executee silencieusement en tache de fond a chaque lancement, s'est deja
/// revelee interminable sur une grosse base de reference (rapporte par
/// l'utilisateur comme "beaucoup trop long") -- reservee a un declenchement
/// explicite avec progression, comme le reste de cette maintenance.
///
/// Deux phases : (1) rejeu de toutes les parties, dans l'ordre, vers une table
/// temporaire sans index (ecriture sequentielle, rapide) -- paginee par id
/// (empreinte memoire bornee, un commit par page), rejeu sur tous les coeurs
/// (voir replay_page) ; (2) un seul INSERT ... SELECT ... ORDER BY vers
/// `positions`, dont le tri SQLite se fait hors memoire et dont l'insertion
/// est sequentielle (une insertion dans l'ordre des parties se ferait au
/// contraire au hasard dans un arbre de dizaines de Go : ~60 parties/s mesure).
/// La synchronisation disque est desactivee le temps de la reconstruction
/// (rien a perdre : elle recommence de zero si elle est interrompue).
pub fn rebuild_position_index(
    conn: &mut Connection,
    mut on_progress: impl FnMut(RebuildProgress),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<(), String> {
    let previous_sync: i64 = conn.query_row("PRAGMA synchronous", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA synchronous = OFF; PRAGMA cache_size = -524288; PRAGMA threads = 4;").map_err(|e| e.to_string())?;
    let result = rebuild_position_index_inner(conn, &mut on_progress, &mut should_cancel);
    let _ = conn.execute_batch("DROP TABLE IF EXISTS position_stage;");
    let _ = conn.execute_batch(&format!("PRAGMA synchronous = {previous_sync}; PRAGMA cache_size = -2000;"));
    result
}

fn rebuild_position_index_inner(
    conn: &mut Connection,
    on_progress: &mut impl FnMut(RebuildProgress),
    should_cancel: &mut impl FnMut() -> bool,
) -> Result<(), String> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS position_stage;
         CREATE TABLE position_stage (zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    // DELETE sans WHERE : SQLite reconnait ce cas ("truncate optimization")
    // et vide la table sans journaliser chaque ligne individuellement.
    conn.execute("DELETE FROM positions", []).map_err(|e| e.to_string())?;

    let mut ids = SanIds::load(conn)?;
    let mut examined = 0u64;
    let mut last_id = 0i64;
    loop {
        if should_cancel() {
            return Ok(());
        }
        let page: Vec<(i64, String, Option<String>, Option<String>)> = {
            let mut stmt = conn
                .prepare("SELECT id, mainline_sans, start_fen, result FROM games WHERE id > ?1 ORDER BY id LIMIT ?2")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![last_id, REBUILD_POSITION_INDEX_PAGE_SIZE], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default(), row.get(2)?, row.get(3)?))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let replayed = replay_page(&page);
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut insert = tx
                .prepare_cached("INSERT INTO position_stage (zobrist, game_id, next_id, result) VALUES (?1, ?2, ?3, ?4)")
                .map_err(|e| e.to_string())?;
            for (id, code, positions) in &replayed {
                for (zobrist, next_san) in positions {
                    let next_id = match next_san {
                        Some(san) => Some(ids.id(&tx, san)?),
                        None => None,
                    };
                    insert.execute(params![zobrist, id, next_id, code]).map_err(|e| e.to_string())?;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        examined += page.len() as u64;
        on_progress(RebuildProgress::Reading { examined });
    }

    // Phase 2 : tri + insertion sequentielle. `rowid` en dernier critere garde
    // la PREMIERE occurrence d'une position repetee dans une meme partie
    // (INSERT OR IGNORE ignore les suivantes), comme index_game.
    on_progress(RebuildProgress::Writing { written: 0, total: 0 });
    conn.execute_batch(
        "INSERT OR IGNORE INTO positions (zobrist, game_id, next_id, result)
         SELECT zobrist, game_id, next_id, result FROM position_stage ORDER BY zobrist, game_id, rowid;
         DROP TABLE position_stage;",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Delai maximum d'une requete de l'explorateur : la connexion de la base de
/// reference est partagee (verrou), une requete interminable -- position tres
/// frequentee sans index couvrant, filtre tres large -- geleait toute l'appli.
/// Au-dela, la requete est interrompue et renvoie une erreur lisible.
pub fn with_time_limit<T>(conn: &Connection, limit: std::time::Duration, run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let handle = conn.get_interrupt_handle();
    let (done, waiting) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if waiting.recv_timeout(limit) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            handle.interrupt();
        }
    });
    let result = run();
    let _ = done.send(());
    let _ = watchdog.join();
    result.map_err(|e| {
        if e.to_lowercase().contains("interrupt") {
            format!("requete trop longue (plus de {} s), interrompue", limit.as_secs())
        } else {
            e
        }
    })
}

/// Recalcule le "+"/"#" de chaque coup suivant en le rejouant reellement
/// depuis `position`, plutot que de dependre de position_index.next_san (deja
/// sans suffixe, voir positions_for_mainline) -- utile pour retrouver mat vs
/// echec, perdu a l'indexation. Ignore silencieusement un coup qui ne se
/// rejoue pas (ne devrait pas arriver, next_san vient d'un coup deja valide).
fn annotate_check_suffixes(stats: &mut [OpeningMoveStat], position: &Chess) {
    for stat in stats.iter_mut() {
        let Ok(san) = stat.san.parse::<San>() else { continue };
        let Ok(mv) = san.to_move(position) else { continue };
        let Ok(new_position) = position.clone().play(mv) else { continue };
        if new_position.is_checkmate() {
            stat.san.push('#');
        } else if new_position.is_check() {
            stat.san.push('+');
        }
    }
}

/// Agrege les coups joues depuis la position atteinte par `moves` (prefixe
/// de la ligne principale de l'onglet affiche, en SAN, sans suffixe) depuis
/// `start_fen` -- TOUTES les parties de la base qui sont un jour passees par
/// cette position EXACTE (transpositions et departs differents compris,
/// voir zobrist_of), pas seulement celles qui partagent le meme texte de
/// coups depuis le meme depart. Vide (pas d'erreur) si `moves`/`start_fen`
/// ne se rejouent pas.
pub fn explore(conn: &Connection, moves: &[String], filters: &ExplorerFilters, start_fen: Option<&str>) -> Result<Vec<OpeningMoveStat>, String> {
    let Some(position) = replay_position(moves, start_fen) else { return Ok(Vec::new()) };
    let mut stats = stats_at_zobrist(conn, zobrist_of(&position), filters)?;
    annotate_check_suffixes(&mut stats, &position);
    Ok(stats)
}

/// Empreinte de la position atteinte par `moves` depuis `start_fen` -- pour
/// interroger position_index (voir db::games_at_zobrist/count_at_zobrist,
/// utilisees par le panneau "parties dans cette position"). None si
/// `moves`/`start_fen` ne se rejouent pas.
pub fn current_zobrist(moves: &[String], start_fen: Option<&str>) -> Option<i64> {
    replay_position(moves, start_fen).map(|p| zobrist_of(&p))
}

/// Premier noeud de la ligne principale (racine -- premier enfant a chaque
/// noeud, meme convention que game_tree::promote_variation) dont la position
/// correspond a `zobrist` -- pour ouvrir une partie trouvee par l'explorateur
/// directement a la position recherchee (transpositions et departs
/// personnalises compris, voir current_zobrist) plutot qu'a son debut, qui
/// n'a aucune raison de coincider une fois qu'on compare des POSITIONS et non
/// plus des prefixes de texte. None si aucune position de la ligne
/// principale ne correspond (ne devrait pas arriver pour une partie que
/// l'explorateur vient de trouver via cette meme empreinte).
pub fn mainline_node_at_zobrist(tree: &crate::game_tree::GameTree, zobrist: i64) -> Option<u32> {
    let mut id = 0u32;
    loop {
        let node = tree.node(id)?;
        if zobrist_of(&node.position) == zobrist {
            return Some(id);
        }
        id = *node.children.first()?;
    }
}

/// Coup suivant + resultat de chaque occurrence de `zobrist` dans
/// position_index, agreges en SQL (GROUP BY, comme l'ancien systeme) puis
/// regroupes cote Rust par tally_rows_to_stats (fusionne par ex. "Qxf7" et
/// "Qxf7#" au cas ou une ligne plus ancienne aurait garde le suffixe).
fn stats_at_zobrist(conn: &Connection, zobrist: i64, filters: &ExplorerFilters) -> Result<Vec<OpeningMoveStat>, String> {
    let (sql, params) = stats_query(zobrist, filters);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            let san: String = row.get(0)?;
            let result: Option<String> = result_from_code(row.get(1)?).map(|r| r.to_string());
            let cnt: u32 = row.get(2)?;
            Ok((san, result, cnt))
        })
        .map_err(|e| e.to_string())?;

    tally_rows_to_stats(rows)
}

fn stats_query(zobrist: i64, filters: &ExplorerFilters) -> (String, Vec<Box<dyn ToSql>>) {
    let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(zobrist)];
    let mut clauses = vec!["p.zobrist = ?1".to_string(), "p.next_id IS NOT NULL".to_string()];
    // Le filtre (joueur/classement/annee) porte sur des colonnes de `games`,
    // absentes de positions -- jointure seulement si necessaire (le cas
    // courant, sans filtre, se lit dans positions seule).
    //
    // Avec un filtre joueur, on part de `games` (index sur le nom : quelques
    // milliers de parties) et on cherche chacune dans positions par sa cle
    // primaire (zobrist, game_id) -- CROSS JOIN force cet ordre. Partir de
    // positions parcourait, sur une position tres frequentee (depart : 10 M
    // de lignes), toutes ses lignes pour ne garder que celles du joueur
    // (38 s mesure pour un joueur de 4 000 parties).
    let from = if filters.is_empty() {
        "positions p".to_string()
    } else if filters.player.is_some() {
        "games g CROSS JOIN positions p ON p.game_id = g.id AND p.zobrist = ?1".to_string()
    } else {
        "positions p JOIN games g ON g.id = p.game_id".to_string()
    };
    if let Some(p) = &filters.player {
        clauses.push(p.where_clause(&mut params));
    }
    if let Some((min, max)) = filters.rating_range {
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("white_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
        params.push(Box::new(min));
        params.push(Box::new(max));
        clauses.push(format!("black_elo BETWEEN ?{} AND ?{}", params.len() - 1, params.len()));
    }
    if let Some((year_min, year_max)) = filters.year_range {
        params.push(Box::new(format!("{year_min:04}")));
        params.push(Box::new(format!("{year_max:04}\u{ffff}")));
        clauses.push(format!("date_played >= ?{} AND date_played < ?{}", params.len() - 1, params.len()));
    }
    let where_sql = clauses.join(" AND ");
    // Agregation d'abord (sur les entiers), decodage des coups ensuite : le
    // dictionnaire ne se lit que pour les quelques dizaines de coups distincts.
    let sql = format!(
        "SELECT d.san, t.result, t.cnt
         FROM (SELECT p.next_id AS next_id, p.result AS result, COUNT(*) AS cnt FROM {from} WHERE {where_sql} GROUP BY p.next_id, p.result) t
         JOIN san_dict d ON d.id = t.next_id"
    );
    (sql, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::collections::BTreeMap;
    use std::path::Path;

    // Coeur du correctif "cliquer une partie de l'explorateur ne s'ouvre pas
    // a la position en cours" : la partie a pu atteindre la position par
    // transposition (ordre de coups different), il ne suffit donc pas de
    // compter des demi-coups depuis l'onglet d'origine -- il faut retrouver
    // le noeud dont la POSITION correspond reellement.
    #[test]
    fn mainline_node_at_zobrist_finds_the_matching_node_even_via_transposition() {
        let tree = crate::pgn::parse_game("[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. c4 Nf6 2. d4 e6 *").unwrap();
        let target = current_zobrist(&["d4".to_string(), "Nf6".to_string(), "c4".to_string()], None).unwrap();
        let node_id = mainline_node_at_zobrist(&tree, target).unwrap();
        let node = tree.node(node_id).unwrap();
        assert_eq!(zobrist_of(&node.position), target);
        // ce noeud est bien apres "c4 Nf6", pas la racine (id 0)
        assert_ne!(node_id, 0);
    }

    #[test]
    fn mainline_node_at_zobrist_returns_none_when_the_position_never_occurs() {
        let tree = crate::pgn::parse_game("[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 e5 *").unwrap();
        assert_eq!(mainline_node_at_zobrist(&tree, 123_456_789), None);
    }

    fn base_tags_for_explorer(white: &str, black: &str, date: &str, result: &str) -> BTreeMap<String, String> {
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), white.to_string());
        tags.insert("Black".to_string(), black.to_string());
        tags.insert("Date".to_string(), date.to_string());
        tags.insert("Result".to_string(), result.to_string());
        tags
    }

    fn conn_with_games(games: &[(&str, &str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT, start_fen TEXT, tournament_id INTEGER,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE san_dict (id INTEGER PRIMARY KEY, san TEXT NOT NULL UNIQUE);
            CREATE TABLE positions (
                zobrist INTEGER NOT NULL, game_id INTEGER NOT NULL, next_id INTEGER, result INTEGER NOT NULL,
                PRIMARY KEY (zobrist, game_id)
            ) WITHOUT ROWID;
            CREATE TABLE tournaments (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL, site TEXT NOT NULL, time_control TEXT,
                games INTEGER NOT NULL, date_start TEXT, date_end TEXT, created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX idx_tournaments_name_site ON tournaments(name, site);",
        )
        .unwrap();
        for (white, mainline, result) in games {
            let mut tags = BTreeMap::new();
            tags.insert("White".to_string(), white.to_string());
            tags.insert("Result".to_string(), result.to_string());
            db::save_game(&conn, "1. e4 *", &tags, mainline).unwrap();
        }
        conn
    }

    #[test]
    fn aggregates_next_move_after_prefix() {
        let conn = conn_with_games(&[
            ("Alice", "e4 e5 Nf3", "1-0"),
            ("Bob", "e4 e5 Nf3", "0-1"),
            ("Carol", "e4 e5 Bc4", "1/2-1/2"),
            ("Dave", "d4 d5", "1-0"), // ne correspond pas au prefixe
        ]);

        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(stats.len(), 2);

        let nf3 = stats.iter().find(|s| s.san == "Nf3").unwrap();
        assert_eq!(nf3.games, 2);
        assert_eq!(nf3.white_wins, 1);
        assert_eq!(nf3.black_wins, 1);

        let bc4 = stats.iter().find(|s| s.san == "Bc4").unwrap();
        assert_eq!(bc4.games, 1);
        assert_eq!(bc4.draws, 1);
    }

    // Cas reel rapporte par l'utilisateur : apres "e4 c5 Nf3 d6 Bb5+", les
    // reponses Nc6/Bd7 manquaient completement du tableau de stats (tout en
    // restant trouvables via games_at_zobrist, moins strict) des que la
    // partie de reference gardait encore le "+" de Bb5+ dans mainline_sans
    // (import anterieur a la correction de bulk_import, ou nettoyage
    // db::strip_check_suffixes pas encore passe sur cette ligne).
    #[test]
    fn finds_continuations_when_the_last_prefix_move_still_has_a_check_suffix() {
        let conn = conn_with_games(&[
            ("Alice", "e4 c5 Nf3 d6 Bb5+ Nd7", "1-0"), // deja nettoyee
            ("Bob", "e4 c5 Nf3 d6 Bb5+ Nc6", "0-1"),   // pas encore nettoyee
            ("Carol", "e4 c5 Nf3 d6 Bb5+ Bd7", "1/2-1/2"), // pas encore nettoyee
        ]);

        let stats = explore(
            &conn,
            &["e4".to_string(), "c5".to_string(), "Nf3".to_string(), "d6".to_string(), "Bb5".to_string()],
            &ExplorerFilters::default(),
            None,
        )
        .unwrap();

        assert_eq!(stats.iter().map(|s| s.san.as_str()).collect::<std::collections::HashSet<_>>(), ["Nd7", "Nc6", "Bd7"].into());
    }

    // Meme scenario que ci-dessus, mais sur le coup RETOURNE (pas celui du
    // prefixe deja joue) : deux parties qui continuent par le meme coup
    // Qxf7, l'une encore annotee "Qxf7#" (pas nettoyee -- "#" fictif ici,
    // juste pour simuler une annotation source perimee/incoherente, cette
    // position n'est pas reellement mat), l'autre deja "Qxf7" (nettoyee), ne
    // doivent pas apparaitre comme deux coups distincts dans le tableau --
    // sans quoi le nombre de parties/l'ordre d'affichage deviennent
    // incoherents tant que le nettoyage n'est pas termine partout. Le "+"
    // affiche est recalcule (voir annotate_check_suffixes), pas copie de la
    // donnee source -- Qxf7 met bien le roi noir en echec ici, sans etre mat.
    #[test]
    fn merges_check_and_clean_variants_of_the_same_next_move() {
        let conn = conn_with_games(&[
            ("Alice", "e4 e5 Qh5 Nc6 Qxf7#", "1-0"), // pas encore nettoyee
            ("Bob", "e4 e5 Qh5 Nc6 Qxf7", "1-0"),    // deja nettoyee
        ]);

        let stats =
            explore(&conn, &["e4".to_string(), "e5".to_string(), "Qh5".to_string(), "Nc6".to_string()], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qxf7+");
        assert_eq!(stats[0].games, 2);
    }

    // Position reellement mat (le "Fool's Mate"), pour verifier que le
    // recalcul distingue bien "#" de "+" plutot que d'apposer bêtement un
    // suffixe generique.
    #[test]
    fn annotates_real_checkmate_with_hash_not_plus() {
        let conn = conn_with_games(&[("Alice", "f3 e5 g4 Qh4", "0-1")]);

        let stats = explore(&conn, &["f3".to_string(), "e5".to_string(), "g4".to_string()], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qh4#");
    }

    #[test]
    fn explore_with_player_filter_only_considers_that_player() {
        // db::open (pas conn_with_games) : le filtre joueur force
        // idx_games_white/idx_games_black, absents du schema minimal utilise
        // par les autres tests de ce module.
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        let filters = ExplorerFilters { player: Some(PlayerFilter { name: "Alice".to_string(), white: true, exact: true }), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters, None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");

        // meme prefixe precalcule (profondeur 2) mais sans filtre : les deux parties
        let all = explore(&conn, &["e4".to_string(), "e5".to_string()], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(all.len(), 2);
    }

    // Cas reel rapporte par l'utilisateur : le filtre joueur ad-hoc du
    // panneau explorateur (exact: false) est tape librement au clavier, pas
    // choisi dans une liste deja resolue comme pour la preparation
    // d'adversaire (exact: true) -- une egalite exacte n'y matchait donc
    // presque jamais rien ("n'importe quel nom -> aucune partie").
    #[test]
    fn explore_with_non_exact_player_filter_matches_by_case_insensitive_prefix() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Carlsen, Magnus".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        // recherche partielle, casse differente : doit tout de meme trouver
        // "Carlsen, Magnus" (exact: true echouerait ici).
        let filters =
            ExplorerFilters { player: Some(PlayerFilter { name: "carlsen".to_string(), white: true, exact: false }), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters, None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");
    }

    #[test]
    fn explore_with_rating_range_only_considers_games_where_both_players_are_in_range() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("Black".to_string(), "Carol".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "2450".to_string());
        tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        // un seul camp dans l'intervalle demande : ne doit pas compter
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Bob".to_string());
        tags.insert("Black".to_string(), "Dave".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "1500".to_string());
        tags.insert("Result".to_string(), "0-1".to_string());
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();

        let filters = ExplorerFilters { rating_range: Some((2300, 2500)), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters, None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Nf3");
    }

    #[test]
    fn explore_with_year_range_only_considers_games_in_that_range() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags_for_explorer("Alice", "Carol", "2015.06.01", "1-0");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Nf3").unwrap();
        tags = base_tags_for_explorer("Bob", "Dave", "1990.01.01", "0-1");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Bc4").unwrap();
        // date partiellement inconnue mais annee connue : doit rester incluse
        tags = base_tags_for_explorer("Eve", "Frank", "2018.??.??", "1/2-1/2");
        db::save_game(&conn, "1. e4 *", &tags, "e4 e5 Qh5").unwrap();

        let filters = ExplorerFilters { year_range: Some((2010, 2020)), ..Default::default() };
        let stats = explore(&conn, &["e4".to_string(), "e5".to_string()], &filters, None).unwrap();
        assert_eq!(stats.iter().map(|s| s.san.as_str()).collect::<std::collections::HashSet<_>>(), ["Nf3", "Qh5"].into());
    }

    #[test]
    fn rebuild_position_index_matches_on_the_fly_computation() {
        let mut conn = conn_with_games(&[
            ("Alice", "e4 e5 Nf3", "1-0"),
            ("Bob", "e4 e5 Nf3", "0-1"),
            ("Carol", "e4 c5 Nf3", "1/2-1/2"),
            ("Dave", "d4 d5", "1-0"),
        ]);

        rebuild_position_index(&mut conn, |_| {}, || false).unwrap();

        // profondeur 0 (position de depart) : precalcule
        let opening = explore(&conn, &[], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(opening.len(), 2);
        let e4 = opening.iter().find(|s| s.san == "e4").unwrap();
        assert_eq!(e4.games, 3);
        let d4 = opening.iter().find(|s| s.san == "d4").unwrap();
        assert_eq!(d4.games, 1);

        // profondeur 1 : precalcule
        let after_e4 = explore(&conn, &["e4".to_string()], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(after_e4.len(), 2);
        let e5 = after_e4.iter().find(|s| s.san == "e5").unwrap();
        assert_eq!(e5.games, 2);
        assert_eq!(e5.white_wins, 1);
        assert_eq!(e5.black_wins, 1);
        let c5 = after_e4.iter().find(|s| s.san == "c5").unwrap();
        assert_eq!(c5.games, 1);
        assert_eq!(c5.draws, 1);
    }

    // Coeur de la refonte Zobrist, cote panneau "prochains coups" : deux
    // parties qui atteignent la MEME position par des ORDRES DE COUPS
    // differents (transposition) doivent apparaitre ensemble, meme si leurs
    // mainline_sans respectifs ne partagent aucun prefixe de texte commun --
    // "d4 Nf6 c4" et "c4 Nf6 d4" aboutissent au meme plateau (trait aux
    // Noirs) apres 3 demi-coups.
    #[test]
    fn explore_finds_next_move_stats_across_transposed_games() {
        let conn = conn_with_games(&[("Alice", "d4 Nf6 c4 e6", "1-0"), ("Bob", "c4 Nf6 d4 g6", "0-1")]);

        let stats =
            explore(&conn, &["d4".to_string(), "Nf6".to_string(), "c4".to_string()], &ExplorerFilters::default(), None).unwrap();

        assert_eq!(stats.iter().map(|s| s.san.as_str()).collect::<std::collections::HashSet<_>>(), ["e6", "g6"].into());
    }

    // Sous l'ancien systeme (texte de coups + start_fen distinct), une partie
    // a depart "personnalise" dont le tag FEN decrit pourtant le MEME plateau
    // que le depart standard (ici, seul le numero de coup plein differe :
    // "... 0 1" vs "... 0 5", un champ que le hash Zobrist ignore
    // volontairement, voir explorer::zobrist_of) etait traitee a tort comme
    // une position distincte. Les deux parties doivent maintenant apparaitre
    // ENSEMBLE, la position de depart etant reellement identique.
    #[test]
    fn explore_merges_games_whose_start_fen_describes_the_same_board() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        const SAME_BOARD_DIFFERENT_MOVE_NUMBER: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 5";

        let mut standard_tags = BTreeMap::new();
        standard_tags.insert("White".to_string(), "Alice".to_string());
        standard_tags.insert("Result".to_string(), "1-0".to_string());
        db::save_game(&conn, "1. e4 *", &standard_tags, "e4").unwrap();

        let mut custom_tags = BTreeMap::new();
        custom_tags.insert("White".to_string(), "Carol".to_string());
        custom_tags.insert("Result".to_string(), "0-1".to_string());
        custom_tags.insert("FEN".to_string(), SAME_BOARD_DIFFERENT_MOVE_NUMBER.to_string());
        db::save_game(&conn, "1. e4 *", &custom_tags, "e4").unwrap();

        let stats = explore(&conn, &[], &ExplorerFilters::default(), None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "e4");
        assert_eq!(stats[0].games, 2);
        assert_eq!(stats[0].white_wins, 1);
        assert_eq!(stats[0].black_wins, 1);
    }

    fn query_plan(conn: &Connection, sql: &str) -> String {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(3)).unwrap();
        rows.map(|r| r.unwrap()).collect::<Vec<_>>().join(" | ")
    }

    // Les requetes de l'explorateur doivent se faire par recherche sur la cle
    // primaire (zobrist, game_id) et ne jamais trier des millions de lignes :
    // c'est ce qui gelait l'appli sur la position de depart d'une grosse base.
    #[test]
    fn explorer_queries_use_the_primary_key_without_sorting() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let stats = query_plan(
            &conn,
            "SELECT d.san, t.result, t.cnt FROM (SELECT p.next_id AS next_id, p.result AS result, COUNT(*) AS cnt FROM positions p
             WHERE p.zobrist = 1 AND p.next_id IS NOT NULL GROUP BY p.next_id, p.result) t JOIN san_dict d ON d.id = t.next_id",
        );
        assert!(stats.contains("SEARCH p USING PRIMARY KEY (zobrist=?)"), "{stats}");

        let count = query_plan(&conn, "SELECT COUNT(*) FROM positions WHERE zobrist = 1");
        assert!(count.contains("PRIMARY KEY (zobrist=?)"), "{count}");

        let list = query_plan(
            &conn,
            "SELECT g.id FROM games g JOIN positions p ON p.game_id = g.id WHERE p.zobrist = 1 ORDER BY p.game_id DESC LIMIT 200",
        );
        assert!(list.contains("PRIMARY KEY (zobrist=?)"), "{list}");
        assert!(!list.contains("TEMP B-TREE"), "{list}");
    }

    // Avec un filtre joueur, la requete doit partir de `games` puis chercher
    // dans positions par cle primaire (zobrist, game_id), jamais l'inverse.
    #[test]
    fn player_filtered_stats_start_from_games() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let filters = ExplorerFilters { player: Some(PlayerFilter { name: "Alice".to_string(), white: true, exact: true }), ..Default::default() };
        let (sql, params) = stats_query(1, &filters);
        let params_refs: Vec<&dyn ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = stmt
            .query_map(params_refs.as_slice(), |row| row.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>()
            .join(" | ");
        let g = plan.find("SEARCH g").or_else(|| plan.find("SCAN g")).expect(&plan);
        let p = plan.find("SEARCH p").expect(&plan);
        assert!(g < p, "{plan}");
        assert!(plan.contains("zobrist=? AND game_id=?"), "{plan}");
    }

    #[test]
    fn with_time_limit_interrupts_a_runaway_query_and_lets_fast_ones_through() {
        let conn = Connection::open_in_memory().unwrap();
        let fast = with_time_limit(&conn, std::time::Duration::from_secs(5), || Ok(1 + 1)).unwrap();
        assert_eq!(fast, 2);

        let slow = with_time_limit(&conn, std::time::Duration::from_millis(200), || {
            conn.query_row(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 100000000000) SELECT COUNT(*) FROM n",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())
        });
        assert!(slow.unwrap_err().contains("trop longue"));
        // la connexion reste utilisable apres une interruption
        assert_eq!(conn.query_row("SELECT 7", [], |row| row.get::<_, i64>(0)).unwrap(), 7);
    }

    // Mesure de debit (cargo test --release -- --ignored --nocapture bench_rebuild) :
    // parties aleatoires legales de ~80 demi-coups, base sur fichier.
    #[test]
    #[ignore]
    fn bench_rebuild_position_index() {
        use shakmaty::Move;
        let path = std::env::temp_dir().join("chesscraft_bench_position_index.sqlite");
        let _ = std::fs::remove_file(&path);
        let mut conn = db::open(&path).unwrap();
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        const GAMES: usize = 20_000;
        conn.execute_batch("BEGIN").unwrap();
        for i in 0..GAMES {
            let mut pos = Chess::default();
            let mut sans = Vec::new();
            for _ in 0..80 {
                let moves: Vec<Move> = pos.legal_moves().into_iter().collect();
                if moves.is_empty() {
                    break;
                }
                let mv = moves[next() % moves.len()].clone();
                sans.push(San::from_move(&pos, mv.clone()).to_string());
                pos = pos.play(mv).unwrap();
            }
            conn.execute(
                "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified) VALUES (?1, 'B', '1-0', 'x', ?2, 0, 0)",
                params![format!("W{i}"), sans.join(" ")],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();

        let started = std::time::Instant::now();
        rebuild_position_index(&mut conn, |_| {}, || false).unwrap();
        let elapsed = started.elapsed();
        let before_vacuum = std::fs::metadata(&path).unwrap().len();
        db::compact_if_worthwhile(&conn, || {}).unwrap();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        let with_games: i64 = conn.query_row("SELECT SUM(LENGTH(mainline_sans)) FROM games", [], |r| r.get(0)).unwrap();
        println!("BENCH: fichier avant compactage {:.1} Mo ; mainline_sans total {:.1} Mo", before_vacuum as f64 / 1e6, with_games as f64 / 1e6);
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM positions", [], |r| r.get(0)).unwrap();
        drop(conn);
        let size = std::fs::metadata(&path).unwrap().len();
        println!(
            "BENCH: {GAMES} parties, {rows} lignes en {:.2}s = {:.0} parties/s ; fichier {:.1} Mo ({:.0} octets/ligne)",
            elapsed.as_secs_f64(),
            GAMES as f64 / elapsed.as_secs_f64(),
            size as f64 / 1e6,
            size as f64 / rows as f64
        );
        let _ = std::fs::remove_file(&path);
    }
}
