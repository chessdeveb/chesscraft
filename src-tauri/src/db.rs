use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
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

/// Comme GameSummary, avec les classements Elo -- propre a la liste "parties
/// dans cette position" de l'explorateur d'ouvertures (voir games_at_position),
/// pas au panneau Parties principal, qui n'en a pas besoin. Struct separee
/// plutot que d'ajouter ces deux champs a GameSummary : celui-ci est partage
/// par plusieurs requetes (list_games_page...) qu'il aurait fallu toutes
/// modifier pour peupler des colonnes dont elles n'ont pas besoin.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerGameSummary {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub event: String,
    pub time_control: String,
    pub white_elo: Option<i32>,
    pub black_elo: Option<i32>,
}

fn map_explorer_game_summary_row(row: &rusqlite::Row) -> rusqlite::Result<ExplorerGameSummary> {
    Ok(ExplorerGameSummary {
        id: row.get(0)?,
        white: or_default(row.get(1)?, "?"),
        black: or_default(row.get(2)?, "?"),
        date: or_default(row.get(3)?, "????.??.??"),
        result: or_default(row.get(4)?, "*"),
        event: or_default(row.get(5)?, "?"),
        time_control: or_default(row.get(6)?, "?"),
        white_elo: row.get(7)?,
        black_elo: row.get(8)?,
    })
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
        CREATE UNIQUE INDEX IF NOT EXISTS idx_opening_prefix_stats_unique ON opening_prefix_stats(prefix, next_move, result);
        CREATE TABLE IF NOT EXISTS player_stats (
            name TEXT NOT NULL COLLATE NOCASE,
            games INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_player_stats_name ON player_stats(name);
        CREATE TABLE IF NOT EXISTS db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    migrate_add_columns(&conn)?;
    migrate_mainline_index(&conn)?;
    migrate_white_elo_index(&conn)?;
    migrate_name_indexes(&conn)?;
    migrate_prefix_stats_unique_index(&conn)?;
    // migrate_strip_check_suffixes n'est PAS appelee ici : sur une base de
    // reference de plusieurs millions de parties, c'est une reecriture
    // couteuse (potentiellement la plupart des lignes + deux index a jour),
    // et open() est appele de facon synchrone/bloquante par de nombreuses
    // commandes (ensure_reference_conn, etc.) -- l'executer ici geleait
    // l'explorateur sans le moindre retour visuel, rapporte en pratique par
    // l'utilisateur ("l'explorateur a l'air planté"). A ne relancer que
    // depuis une tache de fond avec progression (meme mecanique que
    // bulk_import_pgn/refresh_fide_players), pas depuis ce chemin synchrone.
    migrate_prefix_stats(&mut conn)?;
    migrate_player_stats(&mut conn)?;
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

/// Meme principe que migrate_prefix_stats, pour la liste des joueurs : sans
/// precalcul, compter les joueurs distincts d'une base de plusieurs millions
/// de parties (UNION Blancs+Noirs, GROUP BY) prend potentiellement plusieurs
/// minutes -- observe en pratique sur la base de reference de l'utilisateur.
fn migrate_player_stats(conn: &mut Connection) -> Result<(), String> {
    let already_built: i64 = conn.query_row("SELECT COUNT(*) FROM player_stats", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if already_built > 0 {
        return Ok(());
    }
    let has_games: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if has_games == 0 {
        return Ok(());
    }
    rebuild_player_stats(conn)
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

/// idx_games_white_elo (filtre par classement de l'explorateur, voir
/// explorer::ExplorerFilters/position_from_and_where) porte sur white_elo,
/// une colonne ajoutee apres coup par migrate_add_columns : ne peut donc pas
/// etre cree dans le CREATE TABLE IF NOT EXISTS initial (echouerait sur une
/// base plus ancienne, ou la colonne n'existe pas encore a ce moment-la) --
/// doit s'executer apres migrate_add_columns, comme migrate_mainline_index
/// pour mainline_sans.
fn migrate_white_elo_index(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_games_white_elo ON games(white_elo);").map_err(|e| e.to_string())
}

/// Avant ce correctif, mainline_sans etait rempli lors de l'import en masse
/// (voir bulk_import::BulkGameVisitor::san) avec le SAN "complet" du fichier
/// PGN source, "+"/"#" inclus pour un coup qui donne echec/mat -- alors que
/// partout ailleurs dans l'appli (arbre de partie, legal_moves, moteur, voir
/// pgn::san/game_tree::legal_moves/engine::resolve_line) le SAN stocke/compare
/// n'a jamais ce suffixe (shakmaty::San, pas SanPlus). Consequence concrete
/// rapportee par l'utilisateur : un coup avec "+" dans l'explorateur ne
/// pouvait pas se jouer au clic (aucune correspondance dans legal_moves), et
/// rejouer ce coup manuellement faisait ensuite echouer la recherche de
/// parties a cette position (le prefixe de mainline_sans envoye, sans "+",
/// ne correspondait plus a la borne d'intervalle attendue par SQLite, voir
/// position_from_and_where). Corrige a la source cote bulk_import pour les
/// imports futurs ; cette migration nettoie les bases deja importees. "+"/"#"
/// ne servent qu'a cette annotation en SAN, jamais a autre chose : un simple
/// retrait de ces caracteres est donc strictement equivalent au SAN sans
/// suffixe utilise partout ailleurs, sans ambiguite possible.
///
/// PAS appelee depuis open() : sur une base de reference de plusieurs
/// millions de parties c'est une reecriture couteuse, et open() est sur le
/// chemin synchrone de nombreuses commandes -- geleait l'explorateur sans le
/// moindre retour visuel (premiere version de ce correctif). Concue au
/// contraire pour tourner sur une connexion separee, depuis une tache de
/// fond (voir lib.rs::maybe_start_check_suffix_cleanup) : paginee par id
/// (empreinte memoire bornee a une page, jamais toute la table en RAM,
/// contrairement a un unique UPDATE ... WHERE mainline_sans LIKE '%+%' qui
/// materialiserait/verrouillerait tout d'un coup) et interrompue au prochain
/// controle si `should_cancel` repond vrai, un commit par page deja fait
/// restant acquis. `on_progress` recoit le nombre de parties nettoyees
/// jusqu'ici (pas de total connu a l'avance -- le calculer couterait un
/// balayage complet supplementaire).
pub fn already_check_suffixes_cleaned(conn: &Connection) -> Result<bool, String> {
    let done: Option<String> = conn
        .query_row("SELECT value FROM db_meta WHERE key = 'check_suffixes_stripped'", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(done.is_some())
}

const STRIP_CHECK_SUFFIXES_PAGE_SIZE: i64 = 5000;

pub fn strip_check_suffixes(
    conn: &mut Connection,
    mut on_progress: impl FnMut(u32),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<u32, String> {
    if already_check_suffixes_cleaned(conn)? {
        return Ok(0);
    }

    let mut cleaned = 0u32;
    let mut last_id = 0i64;
    loop {
        if should_cancel() {
            return Ok(cleaned);
        }
        let page: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, mainline_sans FROM games WHERE id > ?1 ORDER BY id LIMIT ?2")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![last_id, STRIP_CHECK_SUFFIXES_PAGE_SIZE], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default()))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last_id = page.last().expect("page non vide").0;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut update = tx.prepare("UPDATE games SET mainline_sans = ?1 WHERE id = ?2").map_err(|e| e.to_string())?;
            for (id, mainline) in &page {
                if mainline.contains(['+', '#']) {
                    let cleaned_mainline: String = mainline.chars().filter(|c| *c != '+' && *c != '#').collect();
                    update.execute(params![cleaned_mainline, id]).map_err(|e| e.to_string())?;
                    cleaned += 1;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        on_progress(cleaned);
    }

    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('check_suffixes_stripped', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )
    .map_err(|e| e.to_string())?;
    if cleaned > 0 {
        // opening_prefix_stats derive de l'ancien mainline_sans (donc encore
        // avec "+"/"#" dans next_move) : a reconstruire pour rester coherent,
        // sans attendre migrate_prefix_stats (qui ne s'execute que si la
        // table est encore vide).
        crate::explorer::rebuild_prefix_stats(conn)?;
    }
    Ok(cleaned)
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
    // Tient opening_prefix_stats et player_stats a jour en dehors d'un import
    // en masse (qui fait son propre rebuild complet apres coup) : echec non
    // bloquant, ce ne sont que des caches de performance.
    let _ = crate::explorer::increment_prefix_stats(conn, mainline_sans, tags.get("Result").map(|s| s.as_str()));
    let _ = increment_player_stats(conn, &get("White").unwrap_or_default(), &get("Black").unwrap_or_default());
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
            "SELECT mainline_sans, result, white, black FROM games WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
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

    if let Some((old_mainline, old_result, old_white, old_black)) = &old {
        if let Some(old_mainline) = old_mainline {
            let _ = crate::explorer::decrement_prefix_stats(conn, old_mainline, old_result.as_deref());
        }
        let _ = decrement_player_stats(conn, old_white.as_deref().unwrap_or_default(), old_black.as_deref().unwrap_or_default());
    }
    let _ = crate::explorer::increment_prefix_stats(conn, mainline_sans, tags.get("Result").map(|s| s.as_str()));
    let _ = increment_player_stats(conn, &get("White").unwrap_or_default(), &get("Black").unwrap_or_default());
    Ok(())
}

pub fn delete_game(conn: &Connection, id: i64) -> Result<(), String> {
    let old = conn
        .query_row(
            "SELECT mainline_sans, result, white, black FROM games WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM games WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;

    if let Some((old_mainline, old_result, old_white, old_black)) = old {
        if let Some(old_mainline) = old_mainline {
            let _ = crate::explorer::decrement_prefix_stats(conn, &old_mainline, old_result.as_deref());
        }
        let _ = decrement_player_stats(conn, old_white.as_deref().unwrap_or_default(), old_black.as_deref().unwrap_or_default());
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

/// Bornes d'un intervalle "commence exactement par ces coups" : contrairement
/// a explorer::explore (qui ne veut que les CONTINUATIONS, donc part de
/// `prefix + " "`), ici on veut aussi les parties qui s'arretent pile a cette
/// position -- la borne basse est donc `prefix` lui-meme. Comme les coups
/// sont separes par des espaces, aucune autre partie ne peut se glisser entre
/// `prefix` et `prefix + " "` dans l'ordre lexicographique : cette seule
/// plage capture a la fois le cas exact et toutes les continuations.
fn position_bounds(moves: &[String]) -> (String, String) {
    let prefix = moves.join(" ");
    let hi = format!("{prefix}\u{ffff}");
    (prefix, hi)
}

/// FROM+WHERE partages par count_games_at_position et games_at_position.
/// Seul le filtre joueur force un index different de idx_games_mainline (le
/// prefixe de coups) : un joueur reste une fraction infime de la base meme
/// sur plusieurs millions de parties. Classement et annee, eux, ne sont PAS
/// forcement selectifs -- un intervalle de 2-3 ans sur une base couvrant
/// plusieurs decennies peut representer des centaines de milliers de
/// parties -- forcer idx_games_date dans ce cas (essaye dans une version
/// anterieure) s'est mesure catastrophiquement lent en pratique (SQLite doit
/// relire mainline_sans ligne par ligne pour chacune). Ils restent donc de
/// simples conditions residuelles, jamais des criteres de choix d'index
/// (meme principe, garde synchronise avec explorer::explore_raw).
fn position_from_and_where(
    moves: &[String],
    filters: &crate::explorer::ExplorerFilters,
) -> (String, String, Vec<Box<dyn rusqlite::ToSql>>) {
    // A la position de depart sans filtre joueur, les bornes mainline_sans
    // (>= "" AND < "￿") ne restreignent rien : elles matchent toutes
    // les parties. Les garder forcait quand meme un INDEXED BY idx_games_mainline
    // suivi d'un tri complet des 10M lignes par id (ORDER BY id DESC dans
    // games_at_position) puisque cet index est ordonne par coups, pas par id
    // -- exactement le ralentissement a l'ouverture de l'explorateur sur une
    // grosse base. Sans condition, SQLite retombe sur l'ordre naturel de la
    // clef primaire (id), gratuit pour ORDER BY id DESC LIMIT. Classement/
    // annee n'y changent rien : ce sont de simples conditions residuelles
    // (voir plus haut), evaluees pendant ce balayage par id sans en changer
    // le cout.
    let no_index_needed = moves.is_empty() && filters.player.is_none();

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();
    if !no_index_needed {
        let (lo, hi) = position_bounds(moves);
        params.push(Box::new(lo));
        params.push(Box::new(hi));
        clauses.push(format!("mainline_sans >= ?{} AND mainline_sans < ?{}", params.len() - 1, params.len()));
    }

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

    let index = match &filters.player {
        Some(p) => Some(if p.white { "idx_games_white" } else { "idx_games_black" }),
        None if !no_index_needed => Some("idx_games_mainline"),
        None => None,
    };
    let from = match index {
        Some(index) => format!("games INDEXED BY {index}"),
        None => "games".to_string(),
    };
    let where_clause = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };

    (from, where_clause, params)
}

/// Nombre de parties de la base de reference passant exactement par cette
/// position (prefixe de mainline_sans), optionnellement restreint par
/// joueur/classement/annee (voir explorer::ExplorerFilters). Voir
/// games_at_position pour le detail des bornes utilisees.
pub fn count_games_at_position(conn: &Connection, moves: &[String], filters: &crate::explorer::ExplorerFilters) -> Result<u32, String> {
    let (from, where_clause, params) = position_from_and_where(moves, filters);
    let sql = format!("SELECT COUNT(*) FROM {from} {where_clause}");
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0)).map_err(|e| e.to_string())?;
    Ok(count as u32)
}

/// Traduit (colonne, sens) fournis par l'interface en clause ORDER BY fixe
/// (voir order_by_clause, meme principe pour le panneau Parties principal --
/// id en depart-egalite pour un ordre stable entre deux parties de meme
/// valeur). Retombe sur "les plus recentes d'abord" (id DESC) pour toute
/// valeur vide/inconnue.
fn explorer_games_order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("white", false) => "ORDER BY white COLLATE NOCASE ASC, id DESC",
        ("white", true) => "ORDER BY white COLLATE NOCASE DESC, id DESC",
        ("black", false) => "ORDER BY black COLLATE NOCASE ASC, id DESC",
        ("black", true) => "ORDER BY black COLLATE NOCASE DESC, id DESC",
        ("date", false) => "ORDER BY date_played ASC, id DESC",
        ("date", true) => "ORDER BY date_played DESC, id DESC",
        ("whiteElo", false) => "ORDER BY white_elo ASC, id DESC",
        ("whiteElo", true) => "ORDER BY white_elo DESC, id DESC",
        ("blackElo", false) => "ORDER BY black_elo ASC, id DESC",
        ("blackElo", true) => "ORDER BY black_elo DESC, id DESC",
        _ => "ORDER BY id DESC",
    }
}

/// Parties de la base de reference passant exactement par cette position,
/// triees selon `sort_by`/`sort_dir` (les plus recentes d'abord si vide),
/// optionnellement restreintes par joueur/classement/annee. Utilise pour le
/// panneau "parties dans cette position" de l'explorateur d'ouvertures.
pub fn games_at_position(
    conn: &Connection,
    moves: &[String],
    filters: &crate::explorer::ExplorerFilters,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<ExplorerGameSummary>, String> {
    // Le balayage id-decroissant-avec-arret-anticipe (voir
    // games_at_position_shallow_scan) ne sait produire QUE l'ordre naturel de
    // l'id -- un tri explicite sur une autre colonne demande de toute facon
    // de visiter/trier l'ensemble des correspondances, ce raccourci ne peut
    // donc s'appliquer qu'a l'ordre par defaut (aucun tri demande). Le
    // plafond (voir games_at_position_shallow_scan) retombe sur la branche
    // indexee ci-dessous si le prefixe ne correspond a aucune partie
    // reelle (ex. une partie demarree depuis l'editeur de position).
    if sort_by.is_empty() && filters.player.is_none() && !moves.is_empty() && moves.len() <= crate::explorer::PRECOMPUTED_MAX_DEPTH {
        if let Some(rows) = games_at_position_shallow_scan(conn, &moves.join(" "), filters, limit, offset, SHALLOW_SCAN_ROW_CAP)? {
            return Ok(rows);
        }
    }

    let order_by = explorer_games_order_by_clause(sort_by, sort_dir);
    let (from, where_clause, mut params) = position_from_and_where(moves, filters);
    let limit_ph = params.len() + 1;
    let offset_ph = params.len() + 2;
    params.push(Box::new(limit));
    params.push(Box::new(offset));
    let sql = format!(
        "SELECT id, white, black, date_played, result, event, time_control, white_elo, black_elo
         FROM {from} {where_clause}
         {order_by} LIMIT ?{limit_ph} OFFSET ?{offset_ph}"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), map_explorer_game_summary_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

// Au-dela de ce nombre de lignes EXAMINEES (pas seulement de correspondances
// trouvees), on abandonne le balayage plutot que d'aller jusqu'au bout de la
// table -- voir games_at_position_shallow_scan pour le cas reel qui a motive
// ce plafond. Assez large pour qu'un vrai coup d'ouverture usuel (qui trouve
// ses `limit` correspondances en quelques dizaines/centaines de lignes tout
// au plus) ne le touche jamais ; assez etroit pour qu'un prefixe qui ne
// correspond a rien coute une fraction de seconde plutot que de parcourir
// des millions de lignes.
const SHALLOW_SCAN_ROW_CAP: u32 = 300_000;

/// Balaie `games` par id decroissant -- l'ordre naturel de la cle primaire,
/// donc gratuit pour "les plus recentes d'abord", sans le moindre tri -- et
/// filtre le prefixe cote Rust, en s'arretant des que `limit` correspondances
/// (apres avoir saute les `offset` premieres) ont ete trouvees. Retourne None
/// (voir `row_cap`) si le plafond de lignes examinees est atteint sans avoir
/// rempli la demande : l'appelant retombe alors sur la branche indexee.
///
/// Reserve aux prefixes courts (1 ou 2 demi-coups, voir l'appelant) : a cette
/// profondeur, quelques coups d'ouverture usuels couvrent une tres grande
/// part de la base, donc peu de lignes sont a parcourir avant d'obtenir
/// `limit` correspondances. Au contraire, forcer `INDEXED BY idx_games_mainline`
/// (utilise pour un prefixe plus profond/selectif, voir games_at_position)
/// oblige alors SQLite a materialiser puis trier par id l'ENSEMBLE des lignes
/// correspondantes avant d'appliquer LIMIT ("USE TEMP B-TREE FOR ORDER BY"
/// dans EXPLAIN QUERY PLAN) -- sur la base de reference (plusieurs millions
/// de parties), ce tri portait sur des millions de lignes des le premier
/// coup clique, mesure ~650x plus lent que ce balayage sur un jeu de test de
/// 300k parties synthetiques (voir l'ancien probe_shallow_prefix_perf).
fn games_at_position_shallow_scan(
    conn: &Connection,
    prefix: &str,
    filters: &crate::explorer::ExplorerFilters,
    limit: u32,
    offset: u32,
    row_cap: u32,
) -> Result<Option<Vec<ExplorerGameSummary>>, String> {
    let with_space = format!("{prefix} ");
    // mainline_sans en dernier (pas entre time_control et white_elo) : garde
    // les 9 premieres colonnes dans le meme ordre que map_explorer_game_summary_row,
    // partagee avec games_at_position.
    let mut stmt = conn
        .prepare("SELECT id, white, black, date_played, result, event, time_control, white_elo, black_elo, mainline_sans FROM games ORDER BY id DESC")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut skipped = 0u32;
    let mut examined = 0u32;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        // Cas reel qui a revele le besoin de ce plafond : une partie demarree
        // depuis l'editeur de position ("Nouvelle position") envoie un coup
        // qui n'apparait jamais dans mainline_sans (ex. "dxe3" comme premier
        // coup n'a aucun sens pour une vraie partie) -- sans plafond, ce
        // balayage parcourait alors la table de reference EN ENTIER (10M+
        // lignes) sans jamais trouver de correspondance ni s'arreter, tout en
        // retenant le verrou partage avec la base de reference : toute autre
        // action de l'explorateur, dans n'importe quelle fenetre, restait
        // bloquee en attente pendant tout ce temps (rapporte par l'utilisateur
        // comme un plantage). Au-dela de ce plafond sans avoir rempli la
        // demande, on abandonne (None) : l'appelant retombe alors sur la
        // branche indexee (position_from_and_where), qui determine "aucune
        // correspondance" quasi instantanement via l'index plutot que de
        // scanner ligne par ligne.
        examined += 1;
        if examined > row_cap {
            return Ok(None);
        }
        let mainline: String = row.get(9).map_err(|e| e.to_string())?;
        if mainline != prefix && !mainline.starts_with(&with_space) {
            continue;
        }
        if let Some((min, max)) = filters.rating_range {
            let white_elo: Option<i32> = row.get(7).map_err(|e| e.to_string())?;
            let black_elo: Option<i32> = row.get(8).map_err(|e| e.to_string())?;
            let in_range = |elo: Option<i32>| elo.is_some_and(|e| e >= min && e <= max);
            if !in_range(white_elo) || !in_range(black_elo) {
                continue;
            }
        }
        if let Some((year_min, year_max)) = filters.year_range {
            let date: String = row.get(3).map_err(|e| e.to_string())?;
            let lo = format!("{year_min:04}");
            let hi = format!("{year_max:04}\u{ffff}");
            if !(date.as_str() >= lo.as_str() && date.as_str() < hi.as_str()) {
                continue;
            }
        }
        if skipped < offset {
            skipped += 1;
            continue;
        }
        out.push(map_explorer_game_summary_row(row).map_err(|e| e.to_string())?);
        if out.len() as u32 >= limit {
            break;
        }
    }
    Ok(Some(out))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSummary {
    pub name: String,
    pub games: u32,
}

/// Reconstruit entierement player_stats a partir des parties actuelles :
/// necessaire apres un import en masse, ou pour la premiere ouverture d'une
/// base importee avant l'ajout de cette table (voir migrate_player_stats).
/// Un seul passage sur white/black, agrege en Rust (HashMap) plutot que par
/// un GROUP BY SQL sur l'union des deux colonnes -- sur la base de reference
/// de l'utilisateur (~10M parties), la version SQL restait bloquee plusieurs
/// minutes (pas d'index utilisable pour un GROUP BY sur toutes les valeurs
/// distinctes), le seul cout ici est celui d'un aller-retour sur la table
/// (meme ordre de grandeur qu'un COUNT(*)). La cle du HashMap est la version
/// en minuscules du nom (les deux colonnes de l'index sont en COLLATE
/// NOCASE, donc "Kasparov,G" et "kasparov,g" doivent fusionner) ; la valeur
/// garde l'orthographe rencontree en premier pour l'affichage.
pub fn rebuild_player_stats(conn: &mut Connection) -> Result<(), String> {
    let tally = {
        let mut stmt = conn.prepare("SELECT white, black FROM games").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)))
            .map_err(|e| e.to_string())?;

        let mut tally: HashMap<String, (String, u64)> = HashMap::new();
        for row in rows {
            let (white, black) = row.map_err(|e| e.to_string())?;
            for name in [white, black].into_iter().flatten() {
                if name.is_empty() || name == "?" {
                    continue;
                }
                let entry = tally.entry(name.to_lowercase()).or_insert_with(|| (name.clone(), 0));
                entry.1 += 1;
            }
        }
        tally
    };

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM player_stats", []).map_err(|e| e.to_string())?;
    {
        let mut insert = tx.prepare("INSERT INTO player_stats (name, games) VALUES (?1, ?2)").map_err(|e| e.to_string())?;
        for (display_name, cnt) in tally.values() {
            insert.execute(params![display_name, *cnt as i64]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// A appeler juste apres l'ajout d'une partie : ajuste player_stats sans tout
/// recalculer (symetrique de explorer::increment_prefix_stats).
fn increment_player_stats(conn: &Connection, white: &str, black: &str) -> Result<(), String> {
    for name in [white, black] {
        if name.is_empty() || name == "?" {
            continue;
        }
        conn.execute(
            "INSERT INTO player_stats (name, games) VALUES (?1, 1)
             ON CONFLICT(name) DO UPDATE SET games = games + 1",
            params![name],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Symetrique de increment_player_stats, a appeler juste avant une
/// suppression ou un remplacement de partie.
fn decrement_player_stats(conn: &Connection, white: &str, black: &str) -> Result<(), String> {
    for name in [white, black] {
        if name.is_empty() || name == "?" {
            continue;
        }
        conn.execute("UPDATE player_stats SET games = games - 1 WHERE name = ?1 AND games > 0", params![name])
            .map_err(|e| e.to_string())?;
    }
    conn.execute("DELETE FROM player_stats WHERE games <= 0", []).map_err(|e| e.to_string())?;
    Ok(())
}

/// Nombre de joueurs distincts (Blancs ou Noirs) correspondant au prefixe de
/// recherche, ou de la base entiere si `search` est vide. Lit player_stats
/// (precalcule) plutot que d'agreger games a la volee.
pub fn count_players(conn: &Connection, search: &str) -> Result<u32, String> {
    let count: i64 = if search.is_empty() {
        conn.query_row("SELECT COUNT(*) FROM player_stats", [], |row| row.get(0))
    } else {
        let (lo, hi) = prefix_bounds(search);
        conn.query_row(
            "SELECT COUNT(*) FROM player_stats WHERE name >= ?1 AND name < ?2",
            params![lo, hi],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count as u32)
}

fn map_player_row(row: &rusqlite::Row) -> rusqlite::Result<PlayerSummary> {
    Ok(PlayerSummary { name: row.get(0)?, games: row.get::<_, i64>(1)? as u32 })
}

/// player_stats reste petit (une ligne par joueur distinct, pas par partie)
/// meme sur une base de plusieurs millions de parties : contrairement au
/// decompte par nom (voir count_players), trier par nombre de parties sans
/// index dedie reste largement assez rapide ici.
fn players_order_by_clause(sort_by: &str, sort_dir: &str) -> &'static str {
    let desc = sort_dir != "asc";
    match (sort_by, desc) {
        ("name", false) => "ORDER BY name ASC",
        ("name", true) => "ORDER BY name DESC",
        ("games", false) => "ORDER BY games ASC, name ASC",
        _ => "ORDER BY games DESC, name ASC",
    }
}

/// Page de joueurs (nom + nombre de parties, Blancs+Noirs confondus), triee
/// selon `sort_by`/`sort_dir` (par defaut : les plus actifs d'abord).
pub fn list_players_page(
    conn: &Connection,
    search: &str,
    sort_by: &str,
    sort_dir: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<PlayerSummary>, String> {
    let order_by = players_order_by_clause(sort_by, sort_dir);
    if search.is_empty() {
        let sql = format!("SELECT name, games FROM player_stats {order_by} LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit, offset], map_player_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    } else {
        let (lo, hi) = prefix_bounds(search);
        let sql = format!("SELECT name, games FROM player_stats WHERE name >= ?1 AND name < ?2 {order_by} LIMIT ?3 OFFSET ?4");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![lo, hi, limit, offset], map_player_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
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

    fn conn_for_strip_check_suffixes_tests() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE games (
                id INTEGER PRIMARY KEY,
                white TEXT, black TEXT, white_elo INTEGER, black_elo INTEGER,
                result TEXT, event TEXT, site TEXT, round TEXT, date_played TEXT,
                eco TEXT, opening_name TEXT, ply_count INTEGER, time_control TEXT,
                pgn_moves TEXT NOT NULL, extra_tags TEXT, mainline_sans TEXT,
                created_at INTEGER NOT NULL, last_modified INTEGER NOT NULL
            );
            CREATE TABLE opening_prefix_stats (
                prefix TEXT NOT NULL, next_move TEXT NOT NULL, result TEXT, cnt INTEGER NOT NULL
            );
            CREATE TABLE db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn strip_check_suffixes_cleans_mainline_sans_and_rebuilds_prefix_stats() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        // simule une partie importee avant le correctif : mainline_sans garde
        // le "+"/"#" du PGN source (voir l'ancien bulk_import::BulkGameVisitor::san).
        conn.execute(
            "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
             VALUES ('Alice', 'Bob', '1-0', '1. e4 e5 2. Qh5 Nc6 3. Qxf7# 1-0', 'e4 e5 Qh5 Nc6 Qxf7#', 0, 0)",
            [],
        )
        .unwrap();

        let mut progress_calls = Vec::new();
        let cleaned = strip_check_suffixes(&mut conn, |n| progress_calls.push(n), || false).unwrap();
        assert_eq!(cleaned, 1);
        assert_eq!(progress_calls, vec![1]);

        let mainline: String = conn.query_row("SELECT mainline_sans FROM games WHERE id = 1", [], |row| row.get(0)).unwrap();
        assert_eq!(mainline, "e4 e5 Qh5 Nc6 Qxf7");

        // opening_prefix_stats reconstruit a partir du mainline_sans nettoye :
        // le prefixe "e4 e5 Qh5 Nc6" (sans suffixe) doit maintenant matcher.
        // Le "+" est recalcule par explore() (voir annotate_check_suffixes),
        // pas copie du "#" (fictif) du PGN de depart -- Qxf7 met le roi noir
        // en echec ici, sans etre mat.
        let stats = crate::explorer::explore(
            &conn,
            &["e4".to_string(), "e5".to_string(), "Qh5".to_string(), "Nc6".to_string()],
            &crate::explorer::ExplorerFilters::default(),
        )
        .unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].san, "Qxf7+");

        // idempotent : un second appel ne doit rien casser (deja marque fait),
        // et ne redeclenche aucun travail (progress jamais appele).
        assert!(already_check_suffixes_cleaned(&conn).unwrap());
        let mut second_progress_calls = Vec::new();
        let cleaned_again = strip_check_suffixes(&mut conn, |n| second_progress_calls.push(n), || false).unwrap();
        assert_eq!(cleaned_again, 0);
        assert!(second_progress_calls.is_empty());
    }

    #[test]
    fn strip_check_suffixes_pages_across_multiple_batches() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        // plus de lignes que STRIP_CHECK_SUFFIXES_PAGE_SIZE ne le permettrait
        // en une seule page, pour verifier que la pagination par id couvre
        // bien toute la table (pas seulement la premiere page).
        for i in 0..(STRIP_CHECK_SUFFIXES_PAGE_SIZE * 2 + 7) {
            conn.execute(
                "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
                 VALUES ('Alice', 'Bob', '1-0', '1. e4+ 1-0', 'e4+', 0, 0)",
                [],
            )
            .unwrap();
            let _ = i;
        }

        let cleaned = strip_check_suffixes(&mut conn, |_| {}, || false).unwrap();
        assert_eq!(cleaned as i64, STRIP_CHECK_SUFFIXES_PAGE_SIZE * 2 + 7);

        let remaining_dirty: i64 = conn
            .query_row("SELECT COUNT(*) FROM games WHERE mainline_sans LIKE '%+%' OR mainline_sans LIKE '%#%'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_dirty, 0);
    }

    #[test]
    fn strip_check_suffixes_stops_at_the_next_check_when_cancelled() {
        let mut conn = conn_for_strip_check_suffixes_tests();
        for _ in 0..(STRIP_CHECK_SUFFIXES_PAGE_SIZE + 5) {
            conn.execute(
                "INSERT INTO games (white, black, result, pgn_moves, mainline_sans, created_at, last_modified)
                 VALUES ('Alice', 'Bob', '1-0', '1. e4+ 1-0', 'e4+', 0, 0)",
                [],
            )
            .unwrap();
        }

        // annule des le premier controle : rien ne doit etre nettoye, et le
        // travail (deja) commit d'une page precedente resterait acquis s'il y
        // en avait eu une -- ici, aucune.
        let cleaned = strip_check_suffixes(&mut conn, |_| {}, || true).unwrap();
        assert_eq!(cleaned, 0);
        assert!(!already_check_suffixes_cleaned(&conn).unwrap());
    }

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

    fn no_filters() -> crate::explorer::ExplorerFilters {
        crate::explorer::ExplorerFilters::default()
    }

    #[test]
    fn games_at_position_shallow_prefix_paginates_correctly() {
        // Prefixe d'un seul demi-coup ("e4") : passe par
        // games_at_position_shallow_scan plutot que la branche indexee+triee
        // -- verifie que le filtrage par prefixe et la pagination (offset/limit,
        // les plus recentes d'abord) y restent corrects, pas seulement rapides.
        let conn = open(Path::new(":memory:")).unwrap();
        for i in 0..5 {
            save_game(&conn, "1. e4 *", &base_tags(&format!("W{i}"), "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        }
        // ne doit jamais apparaitre : autre premier coup
        save_game(&conn, "1. d4 *", &base_tags("Erin", "Frank", "2024.01.01", "*"), "d4 d5").unwrap();

        let moves = vec!["e4".to_string()];
        assert_eq!(count_games_at_position(&conn, &moves, &no_filters()).unwrap(), 5);

        // les plus recentes d'abord (id decroissant) : W4 puis W3
        let page0 = games_at_position(&conn, &moves, &no_filters(), "", "", 2, 0).unwrap();
        assert_eq!(page0.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W4", "W3"]);

        // page suivante (offset 2) : W2 puis W1, "d4" jamais inclus
        let page1 = games_at_position(&conn, &moves, &no_filters(), "", "", 2, 2).unwrap();
        assert_eq!(page1.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W2", "W1"]);

        // derniere page partielle (offset 4, limit 2) : un seul resultat
        let page2 = games_at_position(&conn, &moves, &no_filters(), "", "", 2, 4).unwrap();
        assert_eq!(page2.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["W0"]);
    }

    // Cas reel rapporte par l'utilisateur : une partie demarree depuis
    // l'editeur de position (prise en passant depuis une position
    // personnalisee) produit un premier coup ("dxe3") qui n'apparait dans
    // AUCUNE partie reelle -- sans plafond, games_at_position_shallow_scan
    // parcourait alors la base de reference entiere (10M+ lignes en
    // pratique) sans jamais s'arreter, verrou partage retenu tout du long
    // (toute autre action de l'explorateur, dans n'importe quelle fenetre,
    // restait bloquee en attente -- signale comme un "plantage"). Verifie ici
    // qu'un plafond bas fait bien abandonner (None) plutot que de tout
    // parcourir, et que games_at_position retombe alors sur la branche
    // indexee pour donner malgre tout la bonne reponse (ici : aucune partie).
    #[test]
    fn games_at_position_shallow_scan_gives_up_past_the_row_cap_and_falls_back() {
        let conn = open(Path::new(":memory:")).unwrap();
        for i in 0..10 {
            save_game(&conn, "1. e4 *", &base_tags(&format!("W{i}"), "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        }

        let moves = vec!["dxe3".to_string()];
        // plafond bas (5) : abandonne avant d'avoir examine les 10 lignes
        let gave_up = games_at_position_shallow_scan(&conn, "dxe3", &no_filters(), 20, 0, 5).unwrap();
        assert!(gave_up.is_none());

        // plafond suffisant (100) : parcourt tout, ne trouve rien, mais le
        // dit explicitement (Some(vec![])), pas un abandon.
        let exhausted = games_at_position_shallow_scan(&conn, "dxe3", &no_filters(), 20, 0, 100).unwrap();
        assert_eq!(exhausted, Some(Vec::new()));

        // games_at_position (le point d'entree public) retombe bien sur la
        // branche indexee et donne la bonne reponse malgre le plafond bas --
        // le SHALLOW_SCAN_ROW_CAP reel (300 000) n'est jamais atteint ici
        // avec seulement 10 lignes, mais confirme que le mecanisme
        // d'abandon/repli au bout du compte ne casse jamais la correction.
        assert_eq!(count_games_at_position(&conn, &moves, &no_filters()).unwrap(), 0);
        assert_eq!(games_at_position(&conn, &moves, &no_filters(), "", "", 20, 0).unwrap(), Vec::new());
    }

    #[test]
    fn games_at_position_matches_exact_and_continuations_only() {
        let conn = open(Path::new(":memory:")).unwrap();

        // s'arrete pile a "e4 e5" : doit apparaitre pour le prefixe "e4 e5"
        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        // continue au-dela de "e4 e5" : doit aussi apparaitre
        save_game(&conn, "1. e4 e5 2. Nf3 *", &base_tags("Carol", "Dave", "2024.01.02", "*"), "e4 e5 Nf3").unwrap();
        // autre position : ne doit pas apparaitre
        save_game(&conn, "1. d4 d5 *", &base_tags("Erin", "Frank", "2024.01.03", "*"), "d4 d5").unwrap();

        let moves = vec!["e4".to_string(), "e5".to_string()];
        assert_eq!(count_games_at_position(&conn, &moves, &no_filters()).unwrap(), 2);
        let games = games_at_position(&conn, &moves, &no_filters(), "", "", 10, 0).unwrap();
        let whites: Vec<&str> = games.iter().map(|g| g.white.as_str()).collect();
        assert_eq!(whites.len(), 2);
        assert!(whites.contains(&"Alice"));
        assert!(whites.contains(&"Carol"));

        // position de depart (prefixe vide) : toutes les parties
        assert_eq!(count_games_at_position(&conn, &[], &no_filters()).unwrap(), 3);
    }

    // Le prefixe "e4 e5" est court (profondeur <= PRECOMPUTED_MAX_DEPTH) :
    // sans tri demande, ce test passerait par games_at_position_shallow_scan
    // (ordre id DESC uniquement) -- avec un tri explicite, il doit basculer
    // sur la branche generale (voir games_at_position) pour honorer l'ordre
    // demande, pas rester bloque sur l'ordre naturel de l'id.
    #[test]
    fn games_at_position_sorts_by_the_requested_column() {
        let conn = open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        tags.insert("WhiteElo".to_string(), "2200".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        let mut tags = base_tags("Carol", "Dave", "2024.01.02", "0-1");
        tags.insert("WhiteElo".to_string(), "2600".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        let mut tags = base_tags("Erin", "Frank", "2024.01.03", "1/2-1/2");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();

        let moves = vec!["e4".to_string(), "e5".to_string()];
        let desc = games_at_position(&conn, &moves, &no_filters(), "whiteElo", "desc", 10, 0).unwrap();
        assert_eq!(desc.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["Carol", "Erin", "Alice"]);

        let asc = games_at_position(&conn, &moves, &no_filters(), "whiteElo", "asc", 10, 0).unwrap();
        assert_eq!(asc.iter().map(|g| g.white.as_str()).collect::<Vec<_>>(), vec!["Alice", "Erin", "Carol"]);
    }

    #[test]
    fn games_at_position_respects_player_filter() {
        let conn = open(Path::new(":memory:")).unwrap();

        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4 e5").unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Carol", "Alice", "2024.01.02", "*"), "e4 e5").unwrap();

        let moves = vec!["e4".to_string(), "e5".to_string()];

        // Alice en Blancs : seulement la premiere partie
        let white_filters =
            crate::explorer::ExplorerFilters { player: Some(crate::explorer::PlayerFilter { name: "Alice".to_string(), white: true, exact: true }), ..Default::default() };
        assert_eq!(count_games_at_position(&conn, &moves, &white_filters).unwrap(), 1);
        let games = games_at_position(&conn, &moves, &white_filters, "", "", 10, 0).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].white, "Alice");

        // Alice en Noirs : seulement la seconde
        let black_filters =
            crate::explorer::ExplorerFilters { player: Some(crate::explorer::PlayerFilter { name: "Alice".to_string(), white: false, exact: true }), ..Default::default() };
        assert_eq!(count_games_at_position(&conn, &moves, &black_filters).unwrap(), 1);
        let games = games_at_position(&conn, &moves, &black_filters, "", "", 10, 0).unwrap();
        assert_eq!(games[0].black, "Alice");
    }

    #[test]
    fn games_at_position_respects_rating_range_filter() {
        let conn = open(Path::new(":memory:")).unwrap();
        let mut tags = base_tags("Alice", "Bob", "2024.01.01", "1-0");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "2450".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();
        // un seul camp dans l'intervalle : exclue
        let mut tags = base_tags("Carol", "Dave", "2024.01.02", "0-1");
        tags.insert("WhiteElo".to_string(), "2400".to_string());
        tags.insert("BlackElo".to_string(), "1500".to_string());
        save_game(&conn, "1. e4 e5 *", &tags, "e4 e5").unwrap();

        let moves = vec!["e4".to_string(), "e5".to_string()];
        let filters = crate::explorer::ExplorerFilters { rating_range: Some((2300, 2500)), ..Default::default() };
        assert_eq!(count_games_at_position(&conn, &moves, &filters).unwrap(), 1);
        let games = games_at_position(&conn, &moves, &filters, "", "", 10, 0).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].white, "Alice");
    }

    #[test]
    fn games_at_position_respects_year_range_filter() {
        let conn = open(Path::new(":memory:")).unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Alice", "Bob", "2015.06.01", "1-0"), "e4 e5").unwrap();
        save_game(&conn, "1. e4 e5 *", &base_tags("Carol", "Dave", "1990.01.01", "0-1"), "e4 e5").unwrap();

        let moves = vec!["e4".to_string(), "e5".to_string()];
        let filters = crate::explorer::ExplorerFilters { year_range: Some((2010, 2020)), ..Default::default() };
        assert_eq!(count_games_at_position(&conn, &moves, &filters).unwrap(), 1);
        let games = games_at_position(&conn, &moves, &filters, "", "", 10, 0).unwrap();
        assert_eq!(games[0].white, "Alice");
    }

    #[test]
    fn list_players_page_aggregates_white_and_black_across_games() {
        let conn = open(Path::new(":memory:")).unwrap();

        save_game(&conn, "1. e4 *", &base_tags("Alice", "Bob", "2024.01.01", "*"), "e4").unwrap();
        save_game(&conn, "1. e4 *", &base_tags("Bob", "Alice", "2024.01.02", "*"), "e4").unwrap();
        save_game(&conn, "1. e4 *", &base_tags("Carol", "Alice", "2024.01.03", "*"), "e4").unwrap();

        assert_eq!(count_players(&conn, "").unwrap(), 3);
        let players = list_players_page(&conn, "", "", "", 10, 0).unwrap();
        // Alice apparait dans les 3 parties (2x Blancs+Noirs melanges) : la plus active, en tete
        assert_eq!(players[0].name, "Alice");
        assert_eq!(players[0].games, 3);
        let bob = players.iter().find(|p| p.name == "Bob").unwrap();
        assert_eq!(bob.games, 2);

        // recherche par prefixe : ne garde que Carol
        assert_eq!(count_players(&conn, "Car").unwrap(), 1);
        let filtered = list_players_page(&conn, "Car", "", "", 10, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Carol");

        // tri alphabetique explicite
        let alpha = list_players_page(&conn, "", "name", "asc", 10, 0).unwrap();
        assert_eq!(alpha.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob", "Carol"]);
        let alpha_desc = list_players_page(&conn, "", "name", "desc", 10, 0).unwrap();
        assert_eq!(alpha_desc.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Carol", "Bob", "Alice"]);
    }

    #[test]
    fn rebuild_player_stats_merges_names_case_insensitively() {
        // insertion SQL directe (pas save_game) : simule une base existante
        // ouverte pour la premiere fois apres l'ajout de player_stats, dont
        // le seul peuplement possible est le rebuild complet fait par
        // migrate_player_stats.
        let mut conn = open(Path::new(":memory:")).unwrap();
        conn.execute("DELETE FROM player_stats", []).unwrap();
        conn.execute_batch(
            "INSERT INTO games (white, black, pgn_moves, mainline_sans, created_at, last_modified)
             VALUES ('Kasparov,G', 'Karpov,A', '1. e4 *', 'e4', 0, 0),
                    ('kasparov,g', 'Short,N', '1. e4 *', 'e4', 0, 0);",
        )
        .unwrap();

        rebuild_player_stats(&mut conn).unwrap();

        // Kasparov (fusionne), Karpov, Short : 3 joueurs distincts
        assert_eq!(count_players(&conn, "").unwrap(), 3);
        let players = list_players_page(&conn, "", "", "", 10, 0).unwrap();
        // "Kasparov,G" et "kasparov,g" fusionnent (COLLATE NOCASE) en un seul joueur, 2 parties
        let kasparov = players.iter().find(|p| p.name.eq_ignore_ascii_case("kasparov,g")).unwrap();
        assert_eq!(kasparov.games, 2);
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
