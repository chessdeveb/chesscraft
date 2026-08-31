use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveResult {
    Saved { id: i64 },
    Duplicate { existing: GameSummary },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub event: String,
    pub time_control: String,
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
    // le mode WAL permet a d'autres connexions de lire pendant qu'un import de
    // masse ecrit ; le busy_timeout evite un echec immediat ("database is
    // locked") en cas de bref chevauchement d'ecriture.
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS games (
            id INTEGER PRIMARY KEY,
            white TEXT,
            black TEXT,
            white_elo INTEGER,
            black_elo INTEGER,
            result TEXT,
            event TEXT,
            site TEXT,
            round TEXT,
            date_played TEXT,
            eco TEXT,
            opening_name TEXT,
            ply_count INTEGER,
            time_control TEXT,
            pgn_moves TEXT NOT NULL,
            extra_tags TEXT,
            mainline_sans TEXT,
            created_at INTEGER NOT NULL,
            last_modified INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_games_white ON games(white COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_games_black ON games(black COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_games_date ON games(date_played);
        CREATE INDEX IF NOT EXISTS idx_games_time_control ON games(time_control);
        CREATE INDEX IF NOT EXISTS idx_games_dedup ON games(white, black, date_played, result, mainline_sans);
        CREATE TABLE IF NOT EXISTS opening_prefix_stats (
            prefix TEXT NOT NULL,
            next_move TEXT NOT NULL,
            result TEXT NOT NULL,
            cnt INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_opening_prefix_stats_unique ON opening_prefix_stats(prefix, next_move, result);",
    )
    .map_err(|e| e.to_string())?;
    migrate_add_columns(&conn)?;
    migrate_mainline_index(&conn)?;
    migrate_name_indexes(&conn)?;
    migrate_prefix_stats_unique_index(&conn)?;
    migrate_prefix_stats(&mut conn)?;
    Ok(conn)
}

/// La table opening_prefix_stats a d'abord existe sans contrainte
/// d'unicite ; les mises a jour incrementales (voir
/// explorer::increment_prefix_stats) ont besoin d'un `ON CONFLICT` sur
/// (prefix, next_move, result), donc d'un index unique. Si l'ancien index
/// (non unique) est encore la, on vide la table : migrate_prefix_stats la
/// reconstruira juste apres avec le nouveau schema.
fn migrate_prefix_stats_unique_index(conn: &Connection) -> Result<(), String> {
    let has_unique_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_opening_prefix_stats_unique'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_unique_index > 0 {
        return Ok(());
    }
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_opening_prefix_stats_prefix;
         DELETE FROM opening_prefix_stats;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_opening_prefix_stats_unique ON opening_prefix_stats(prefix, next_move, result);",
    )
    .map_err(|e| e.to_string())
}

/// Precalcule (une seule fois) les stats de l'explorateur d'ouvertures pour
/// les bases qui existaient deja avant l'ajout de opening_prefix_stats :
/// sans ca, une base deja importee resterait indefiniment sur le calcul a la
/// volee, couteux sur les tout premiers coups (voir explorer::explore). Ne
/// fait rien si la table est deja remplie (verification bon marche, quelques
/// milliers de lignes tout au plus) ou si la base est vide.
fn migrate_prefix_stats(conn: &mut Connection) -> Result<(), String> {
    let already_built: i64 = conn
        .query_row("SELECT COUNT(*) FROM opening_prefix_stats", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if already_built > 0 {
        return Ok(());
    }
    let has_games: i64 = conn
        .query_row("SELECT COUNT(*) FROM games WHERE mainline_sans IS NOT NULL AND mainline_sans <> ''", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if has_games == 0 {
        return Ok(());
    }
    crate::explorer::rebuild_prefix_stats(conn)
}

/// La recherche de parties par nom (prefixe insensible a la casse) s'appuie
/// sur des intervalles explicites (col >= ? AND col < ?) plutot que sur LIKE
/// (voir migrate_mainline_index pour le meme constat sur l'explorateur) :
/// pour que cet intervalle utilise l'index au lieu d'un scan complet, la
/// colonne indexee doit deja etre en collation NOCASE. Recree seulement si
/// l'index existant est encore dans son ancienne forme (collation binaire).
fn migrate_name_indexes(conn: &Connection) -> Result<(), String> {
    for (name, column) in [("idx_games_white", "white"), ("idx_games_black", "black")] {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let up_to_date = matches!(&sql, Some(s) if s.contains("COLLATE NOCASE"));
        if !up_to_date {
            conn.execute_batch(&format!(
                "DROP INDEX IF EXISTS {name};
                 CREATE INDEX {name} ON games({column} COLLATE NOCASE);"
            ))
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// L'explorateur d'ouvertures ne lit que mainline_sans et result : un index
/// couvrant les deux evite a SQLite de retourner a la table pour chaque ligne
/// (et l'empeche de preferer a tort un scan complet de idx_games_dedup, plus
/// large, faute de pouvoir "couvrir" la requete avec un index a une seule
/// colonne). Recree seulement si absent ou encore dans son ancienne forme a
/// une colonne, pas a chaque ouverture.
fn migrate_mainline_index(conn: &Connection) -> Result<(), String> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_games_mainline'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let up_to_date = matches!(&sql, Some(s) if s.contains("mainline_sans, result"));
    if !up_to_date {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_games_mainline;
             CREATE INDEX idx_games_mainline ON games(mainline_sans, result);",
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Les bases creees avant l'ajout du support Elo / du cache des coups n'ont
/// pas ces colonnes ; "CREATE TABLE IF NOT EXISTS" ne les ajoute pas
/// retroactivement.
fn migrate_add_columns(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn.prepare("PRAGMA table_info(games)").map_err(|e| e.to_string())?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    if !existing.iter().any(|c| c == "white_elo") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN white_elo INTEGER;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "black_elo") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN black_elo INTEGER;")
            .map_err(|e| e.to_string())?;
    }
    if !existing.iter().any(|c| c == "mainline_sans") {
        conn.execute_batch("ALTER TABLE games ADD COLUMN mainline_sans TEXT;")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn or_default(v: Option<String>, default: &str) -> String {
    v.unwrap_or_else(|| default.to_string())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn save_game(
    conn: &Connection,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<i64, String> {
    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let timestamp = now();
    conn.execute(
        "INSERT INTO games (white, black, white_elo, black_elo, result, event, site, round, date_played, time_control, pgn_moves, mainline_sans, created_at, last_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![
            get("White"),
            get("Black"),
            get_elo("WhiteElo"),
            get_elo("BlackElo"),
            get("Result"),
            get("Event"),
            get("Site"),
            get("Round"),
            get("Date"),
            get("TimeControl"),
            pgn,
            mainline_sans,
            timestamp,
        ],
    )
    .map_err(|e| e.to_string())?;
    // last_insert_rowid() doit etre capture tout de suite : l'appel suivant
    // fait lui-meme des INSERT (dans opening_prefix_stats), qui l'ecraserait
    // sinon avec le rowid de cette AUTRE table.
    let id = conn.last_insert_rowid();
    // Tient opening_prefix_stats a jour en dehors d'un import en masse (qui
    // fait son propre rebuild complet apres coup) : echec non bloquant, ce
    // n'est qu'un cache de performance pour l'explorateur d'ouvertures.
    let _ = crate::explorer::increment_prefix_stats(conn, mainline_sans, tags.get("Result").map(|s| s.as_str()));
    Ok(id)
}

pub fn update_game(
    conn: &Connection,
    id: i64,
    pgn: &str,
    tags: &BTreeMap<String, String>,
    mainline_sans: &str,
) -> Result<(), String> {
    // Le contenu remplace peut avoir change (coups ajoutes, resultat
    // modifie...) : decremente l'ancienne contribution avant d'ecrire la
    // nouvelle, sinon opening_prefix_stats compterait deux fois cette
    // partie.
    let old = conn
        .query_row(
            "SELECT mainline_sans, result FROM games WHERE id = ?1",
            params![id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let get = |key: &str| tags.get(key).cloned();
    let get_elo = |key: &str| tags.get(key).and_then(|v| v.parse::<i64>().ok());
    let timestamp = now();
    conn.execute(
        "UPDATE games SET white = ?1, black = ?2, white_elo = ?3, black_elo = ?4, result = ?5, event = ?6,
         site = ?7, round = ?8, date_played = ?9, time_control = ?10, pgn_moves = ?11, mainline_sans = ?12, last_modified = ?13 WHERE id = ?14",
        params![
            get("White"),
            get("Black"),
            get_elo("WhiteElo"),
            get_elo("BlackElo"),
            get("Result"),
            get("Event"),
            get("Site"),
            get("Round"),
            get("Date"),
            get("TimeControl"),
            pgn,
            mainline_sans,
            timestamp,
            id,
        ],
    )
    .map_err(|e| e.to_string())?;

    if let Some((Some(old_mainline), old_result)) = old {
        let _ = crate::explorer::decrement_prefix_stats(conn, &old_mainline, old_result.as_deref());
    }
    let _ = crate::explorer::increment_prefix_stats(conn, mainline_sans, tags.get("Result").map(|s| s.as_str()));
    Ok(())
}

pub fn delete_game(conn: &Connection, id: i64) -> Result<(), String> {
    let old = conn
        .query_row(
            "SELECT mainline_sans, result FROM games WHERE id = ?1",
            params![id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM games WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;

    if let Some((Some(old_mainline), old_result)) = old {
        let _ = crate::explorer::decrement_prefix_stats(conn, &old_mainline, old_result.as_deref());
    }
    Ok(())
}

fn map_game_summary_row(row: &rusqlite::Row) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        id: row.get(0)?,
        white: or_default(row.get(1)?, "?"),
        black: or_default(row.get(2)?, "?"),
        date: or_default(row.get(3)?, "????.??.??"),
        result: or_default(row.get(4)?, "*"),
        event: or_default(row.get(5)?, "?"),
        time_control: or_default(row.get(6)?, "?"),
    })
}

/// Filtres de recherche des parties d'une base. `search` (recherche rapide,
/// Blancs OU Noirs) et les filtres avances (Blancs/Noirs/Resultat/periode,
/// combines en ET) sont mutuellement exclusifs cote interface : si `search`
/// est renseigne, les filtres avances sont ignores.
#[derive(Default)]
pub struct GamesFilter {
    pub search: String,
    pub white: String,
    pub black: String,
    pub result: String,
    pub date_from: String,
    pub date_to: String,
    pub time_control: String,
}

/// Bornes d'un intervalle "commence par" insensible a la casse.
fn prefix_bounds(value: &str) -> (String, String) {
    (value.to_string(), format!("{value}\u{ffff}"))
}

/// Construit la source ("FROM ...", eventuellement avec INDEXED BY ou une
/// sous-requete) et la clause WHERE (avec ses parametres, dans l'ordre des
/// `?N`) a partir d'un `GamesFilter`. Les valeurs viennent de l'utilisateur
/// et ne sont jamais interpolees telles quelles dans le SQL (injection).
///
/// Prefixe (col >= 'x' AND col < 'x\u{ffff}') plutot que "contient"
/// (LIKE '%x%') : sur une base de plusieurs millions de parties, seul un
/// prefixe permet d'utiliser un index au lieu de parcourir toute la table.
///
/// Deux precautions contre le planificateur de requetes de SQLite, constatees
/// a plusieurs reprises sur cette base (ici et dans l'explorateur) :
/// - un intervalle explicite est utilise plutot que LIKE 'x%', qui ne
///   declenche pas de facon fiable l'optimisation d'index de SQLite ;
/// - `INDEXED BY` force explicitement l'index attendu (idx_games_white /
///   idx_games_black), le planificateur ayant deja choisi a tort de scanner
///   entierement idx_games_dedup (le considerant "couvrant") plutot que de
///   chercher dans l'index dedie, beaucoup plus selectif.
fn build_from_and_where(filter: &GamesFilter) -> (String, String, Vec<Box<dyn rusqlite::ToSql>>) {
    if !filter.search.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.search);
        // Reunion de deux recherches d'index dediees plutot qu'un OR sur
        // white/black dans une seule requete : force chaque branche a
        // utiliser son propre index (INDEXED BY), impossible a exprimer
        // sur une seule reference a `games` avec un OR.
        let from = "(SELECT id FROM games INDEXED BY idx_games_white
                       WHERE white COLLATE NOCASE >= ?1 AND white COLLATE NOCASE < ?2
                     UNION
                     SELECT id FROM games INDEXED BY idx_games_black
                       WHERE black COLLATE NOCASE >= ?1 AND black COLLATE NOCASE < ?2) AS matched
                    JOIN games ON games.id = matched.id"
            .to_string();
        return (from, String::new(), vec![Box::new(lo), Box::new(hi)]);
    }

    let mut clauses = Vec::new();
    let mut ps: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if !filter.white.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.white);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!(
            "(white COLLATE NOCASE >= ?{} AND white COLLATE NOCASE < ?{})",
            ps.len() - 1,
            ps.len()
        ));
    }
    if !filter.black.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.black);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!(
            "(black COLLATE NOCASE >= ?{} AND black COLLATE NOCASE < ?{})",
            ps.len() - 1,
            ps.len()
        ));
    }
    if !filter.result.is_empty() {
        ps.push(Box::new(filter.result.clone()));
        clauses.push(format!("result = ?{}", ps.len()));
    }
    if !filter.date_from.is_empty() {
        ps.push(Box::new(filter.date_from.clone()));
        clauses.push(format!("date_played >= ?{}", ps.len()));
    }
    if !filter.date_to.is_empty() {
        ps.push(Box::new(filter.date_to.clone()));
        clauses.push(format!("date_played <= ?{}", ps.len()));
    }
    if !filter.time_control.is_empty() {
        let (lo, hi) = prefix_bounds(&filter.time_control);
        ps.push(Box::new(lo));
        ps.push(Box::new(hi));
        clauses.push(format!("(time_control >= ?{} AND time_control < ?{})", ps.len() - 1, ps.len()));
    }

    // Un seul filtre sur un nom (sans l'autre) : force son index dedie pour
    // le meme motif que ci-dessus. Combiner white ET black (AND), ou ne
    // filtrer que par resultat/date, reste sur le choix du planificateur.
    let from = if !filter.white.is_empty() && filter.black.is_empty() {
        "games INDEXED BY idx_games_white".to_string()
    } else if filter.white.is_empty() && !filter.black.is_empty() {
        "games INDEXED BY idx_games_black".to_string()
    } else if filter.white.is_empty() && filter.black.is_empty() && !filter.time_control.is_empty() {
        "games INDEXED BY idx_games_time_control".to_string()
    } else {
        "games".to_string()
    };

    let where_clause = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
    (from, where_clause, ps)
}

/// Nombre de parties correspondant aux filtres, ou de la base entiere si
/// aucun filtre n'est actif.
pub fn count_games(conn: &Connection, filter: &GamesFilter) -> Result<u32, String> {
    let (from, where_clause, params) = build_from_and_where(filter);
    let sql = format!("SELECT COUNT(*) FROM {from} {where_clause}");
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let count: i64 = conn
        .query_row(&sql, params_refs.as_slice(), |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

/// Traduit une paire (colonne, sens) fournie par l'interface en fragment SQL
/// fixe : `sort_by`/`sort_dir` viennent de l'utilisateur et ne doivent jamais
/// etre interpoles tels quels dans la requete (injection SQL). Retombe sur le
/// tri par defaut (le plus recent d'abord) pour toute valeur inconnue. Les
/// colonnes sont qualifiees par `games.` : le FROM peut etre une jointure
/// avec une sous-requete (voir build_from_and_where), ou `id`/`white`/`black`
/// seraient ambigus.
fn order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("white", false) => "ORDER BY games.white COLLATE NOCASE ASC, games.id DESC",
        ("white", true) => "ORDER BY games.white COLLATE NOCASE DESC, games.id DESC",
        ("black", false) => "ORDER BY games.black COLLATE NOCASE ASC, games.id DESC",
        ("black", true) => "ORDER BY games.black COLLATE NOCASE DESC, games.id DESC",
        ("date", false) => "ORDER BY games.date_played ASC, games.id DESC",
        ("date", true) => "ORDER BY games.date_played DESC, games.id DESC",
        _ => "ORDER BY games.id DESC",
    }
}

/// Une page de parties, filtrees selon `filter` et triees selon
/// `sort_by`/`sort_dir` (voir `order_by_clause`). Indispensable sur les
/// grosses bases (des centaines de milliers de parties) ou charger la table
/// entiere en un coup n'est pas envisageable.
pub fn list_games_page(
    conn: &Connection,
    filter: &GamesFilter,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<GameSummary>, String> {
    let order_by = order_by_clause(sort_by, sort_dir);
    let (from, where_clause, mut params) = build_from_and_where(filter);
    let limit_ph = params.len() + 1;
    let offset_ph = params.len() + 2;
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    let sql = format!(
        "SELECT games.id, games.white, games.black, games.date_played, games.result, games.event, games.time_control FROM {from}
         {where_clause} {order_by} LIMIT ?{limit_ph} OFFSET ?{offset_ph}"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), map_game_summary_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

/// Cherche une partie deja presente dans la base avec les memes Blancs/Noirs/Date/Resultat
/// et la meme suite de coups (SAN) que celle donnee. Utilise pour alerter l'utilisateur
/// sans jamais bloquer la sauvegarde (voir save_game_to cote lib.rs).
/// Cherche une partie deja presente avec les memes Blancs/Noirs/Date/Resultat
/// et exactement la meme suite de coups. Compare directement la colonne
/// `mainline_sans` mise en cache a la sauvegarde plutot que de reparser (et
/// revalider coup par coup) chaque partie candidate : indispensable pour
/// rester rapide sur une base de plusieurs centaines de milliers de parties.
/// Les parties enregistrees avant l'ajout de cette colonne (mainline_sans
/// NULL) ne peuvent pas etre detectees comme doublons ; c'est un compromis
/// acceptable plutot que de re-parser toute une grosse base a chaque appel.
pub fn find_duplicate(
    conn: &Connection,
    tags: &BTreeMap<String, String>,
    moves: &[String],
) -> Result<Option<GameSummary>, String> {
    let white = tags.get("White").cloned().unwrap_or_default();
    let black = tags.get("Black").cloned().unwrap_or_default();
    let date = tags.get("Date").cloned().unwrap_or_default();
    let result = tags.get("Result").cloned().unwrap_or_default();
    let moves_str = moves.join(" ");

    conn.query_row(
        "SELECT id, white, black, date_played, result, event FROM games
         WHERE white = ?1 AND black = ?2 AND date_played = ?3 AND result = ?4 AND mainline_sans = ?5
         LIMIT 1",
        params![white, black, date, result, moves_str],
        |row| {
            Ok(GameSummary {
                id: row.get(0)?,
                white: or_default(row.get(1)?, "?"),
                black: or_default(row.get(2)?, "?"),
                date: or_default(row.get(3)?, "????.??.??"),
                result: or_default(row.get(4)?, "*"),
                event: or_default(row.get(5)?, "?"),
                time_control: String::new(),
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn load_game_pgn(conn: &Connection, id: i64) -> Result<Option<String>, String> {
    conn.query_row("SELECT pgn_moves FROM games WHERE id = ?1", params![id], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(|e| e.to_string())
}

/// Recharge une partie sous la forme attendue par save_game (tags, pgn,
/// mainline_sans) : les colonnes structurees (white, black, ...) sont deja
/// la version fiable des tags PGN de cette partie, inutile de reparser le
/// texte pour les retrouver. Utilise pour deplacer une partie vers une autre
/// base (voir move_game cote lib.rs).
pub fn load_game_for_move(conn: &Connection, id: i64) -> Result<Option<(BTreeMap<String, String>, String, String)>, String> {
    conn.query_row(
        "SELECT white, black, white_elo, black_elo, result, event, site, round, date_played, time_control, pgn_moves, mainline_sans
         FROM games WHERE id = ?1",
        params![id],
        |row| {
            let mut tags = BTreeMap::new();
            let mut set = |key: &str, value: Option<String>| {
                if let Some(v) = value {
                    tags.insert(key.to_string(), v);
                }
            };
            set("White", row.get::<_, Option<String>>(0)?);
            set("Black", row.get::<_, Option<String>>(1)?);
            set("WhiteElo", row.get::<_, Option<i64>>(2)?.map(|v| v.to_string()));
            set("BlackElo", row.get::<_, Option<i64>>(3)?.map(|v| v.to_string()));
            set("Result", row.get::<_, Option<String>>(4)?);
            set("Event", row.get::<_, Option<String>>(5)?);
            set("Site", row.get::<_, Option<String>>(6)?);
            set("Round", row.get::<_, Option<String>>(7)?);
            set("Date", row.get::<_, Option<String>>(8)?);
            set("TimeControl", row.get::<_, Option<String>>(9)?);
            let pgn: String = row.get(10)?;
            let mainline_sans: Option<String> = row.get(11)?;
            Ok((tags, pgn, mainline_sans.unwrap_or_default()))
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_old_schema_without_elo_columns() {
        let conn = Connection::open_in_memory().unwrap();
        // schema tel qu'il existait avant l'ajout des colonnes Elo
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, result TEXT, event TEXT, site TEXT,
                round TEXT, date_played TEXT, eco TEXT, opening_name TEXT,
                ply_count INTEGER, time_control TEXT, pgn_moves TEXT NOT NULL,
                extra_tags TEXT, created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );",
        )
        .unwrap();

        migrate_add_columns(&conn).unwrap();

        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), "Alice".to_string());
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        let id = save_game(&conn, "1. e4 *", &tags, "e4").unwrap();
        assert!(id > 0);
    }

    fn fresh_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    fn base_tags(white: &str, black: &str, date: &str, result: &str) -> BTreeMap<String, String> {
        let mut tags = BTreeMap::new();
        tags.insert("White".to_string(), white.to_string());
        tags.insert("Black".to_string(), black.to_string());
        tags.insert("Date".to_string(), date.to_string());
        tags.insert("Result".to_string(), result.to_string());
        tags
    }

    #[test]
    fn find_duplicate_matches_same_moves_only() {
        let conn = fresh_conn();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );",
        )
        .unwrap();

        let tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        save_game(&conn, "1. e4 e5 1-0", &tags, "e4 e5").unwrap();

        // memes joueurs/date/resultat mais coups differents : pas un doublon
        let different_moves = vec!["d4".to_string(), "d5".to_string()];
        assert!(find_duplicate(&conn, &tags, &different_moves).unwrap().is_none());

        // coups identiques : doublon detecte
        let same_moves = vec!["e4".to_string(), "e5".to_string()];
        assert!(find_duplicate(&conn, &tags, &same_moves).unwrap().is_some());
    }

    fn opening_count(conn: &Connection, prefix: &str, next_move: &str) -> i64 {
        conn.query_row(
            "SELECT COALESCE(SUM(cnt), 0) FROM opening_prefix_stats WHERE prefix = ?1 AND next_move = ?2",
            params![prefix, next_move],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn opening_prefix_stats_follow_save_update_delete() {
        let conn = open(Path::new(":memory:")).unwrap();

        let tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        let id = save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        assert_eq!(opening_count(&conn, "", "e4"), 1);
        assert_eq!(opening_count(&conn, "e4", "e5"), 1);

        // la partie change de premier coup : l'ancienne contribution doit
        // disparaitre, la nouvelle apparaitre
        update_game(&conn, id, "1. d4 d5 *", &tags, "d4 d5").unwrap();
        assert_eq!(opening_count(&conn, "", "e4"), 0);
        assert_eq!(opening_count(&conn, "", "d4"), 1);
        assert_eq!(opening_count(&conn, "e4", "e5"), 0);
        assert_eq!(opening_count(&conn, "d4", "d5"), 1);

        delete_game(&conn, id).unwrap();
        assert_eq!(opening_count(&conn, "", "d4"), 0);
        assert_eq!(opening_count(&conn, "d4", "d5"), 0);
    }
}
