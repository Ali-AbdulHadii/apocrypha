//! Persistent state for Apocrypha: SQLite-backed games, installed mods,
//! profiles, and per-profile option selections.
//!
//! SQLite is embedded (bundled, no system dependency), runs in WAL mode with
//! foreign keys enforced, and is migrated forward by `user_version`.

pub mod paths;

use apoc_domain::modgroups::{self, Arrangement, LockBreach, Membership, ModGroup, OrderMove};
use apoc_domain::{ModBundle, Selection};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub use paths::Paths;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    /// An order that would disturb a locked group.
    ///
    /// Carries the finished sentence rather than ids. The person reading it
    /// named the group and installed the mod; matching an id against a row to
    /// find out what was refused is work the refusal should have done.
    #[error("{0}")]
    LockedGroup(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Where game definitions come from. Mirrors the "Game Database Source" setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameDbSource {
    LocalBuiltin,
    OnlineApi,
}

impl GameDbSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            GameDbSource::LocalBuiltin => "local-builtin",
            GameDbSource::OnlineApi => "online-api",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "online-api" => GameDbSource::OnlineApi,
            _ => GameDbSource::LocalBuiltin,
        }
    }
}

/// A game the user has configured (detected or manually pointed at).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRecord {
    pub id: String,
    pub name: String,
    pub install_dir: Option<String>,
    pub proton_prefix: Option<String>,
    pub active_profile_id: Option<i64>,
}

/// An imported mod (one archive), with its normalized bundle retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModRecord {
    pub id: String,
    pub game_id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub archive_path: String,
    pub archive_sha256: Option<String>,
    pub installer_model: String,
    /// Unix seconds when the mod was imported.
    #[serde(default)]
    pub imported_at: i64,
    /// Nexus provenance, absent for mods imported from a local file. Kept as two
    /// separate ids because an update check needs the file id to tell "a newer
    /// file exists" from "the same file under a different name".
    #[serde(default)]
    pub nexus_mod_id: Option<i64>,
    #[serde(default)]
    pub nexus_file_id: Option<i64>,
    /// Which directory under the game's staging root holds this mod's extracted
    /// files.
    ///
    /// Deliberately not the mod id. The id is library identity — stable across
    /// versions, and what conflict overrides and load order name — while staging
    /// holds the bytes of one particular archive. Keeping them separate is what
    /// lets a mod be replaced by a newer version without writing over the files
    /// a live deployment still needs to repair from.
    ///
    /// Existing rows are backfilled to their id, which is where their files
    /// already are, so nothing moves on disk.
    #[serde(default)]
    pub staging_key: String,
    /// Full normalized bundle, so the wizard can be reopened without the archive.
    pub bundle: ModBundle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub id: i64,
    pub game_id: String,
    pub name: String,
}

/// A mod's state within one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModState {
    pub mod_id: String,
    pub enabled: bool,
    /// Load-order priority; higher wins conflicts.
    pub priority: i64,
    pub selection: Selection,
    /// The group this mod belongs to in this profile, if any.
    #[serde(default)]
    pub group_id: Option<i64>,
}

/// A group as stored, with the one field the domain type has no use for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModGroupRecord {
    pub group: ModGroup,
    pub profile_id: i64,
    /// Where an *empty* group sits in the list.
    ///
    /// Read only while the group has no members, because a group that has
    /// members is already positioned by them. Two stored answers to one question
    /// is how a block ends up drawn in two places, so this one is written when
    /// the group is created and when its last member leaves, and consulted
    /// nowhere else.
    pub anchor: i64,
}

/// Where a downloaded archive came from.
///
/// Recorded when a download begins, keyed on the path the file will have, and
/// kept after the download queue has forgotten it — the queue is in memory and
/// rebuilds itself by scanning the downloads folder, so a row here is the only
/// thing that still knows the origin of a file tomorrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub archive_path: String,
    /// Nexus game domain, e.g. `monsterhunterwilds`.
    pub domain: Option<String>,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    /// The local mod this file was fetched to update, when it was fetched from
    /// the update screen. A hint: the row it names may since have been removed,
    /// so it is checked before it is believed.
    pub replaces_mod_id: Option<String>,
}

const SCHEMA_VERSION: i64 = 7;

/// The application's persistent store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// An in-memory store (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS games (
                id                TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                install_dir       TEXT,
                proton_prefix     TEXT,
                active_profile_id INTEGER
            );

            CREATE TABLE IF NOT EXISTS mods (
                id              TEXT PRIMARY KEY,
                game_id         TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                name            TEXT NOT NULL,
                version         TEXT,
                author          TEXT,
                archive_path    TEXT NOT NULL,
                archive_sha256  TEXT,
                installer_model TEXT NOT NULL,
                bundle_json     TEXT NOT NULL,
                imported_at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                staging_key     TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_mods_game ON mods(game_id);

            CREATE TABLE IF NOT EXISTS profiles (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                name    TEXT NOT NULL,
                UNIQUE(game_id, name)
            );

            CREATE TABLE IF NOT EXISTS mod_groups (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                name       TEXT NOT NULL,
                color      TEXT NOT NULL DEFAULT 'default',
                locked     INTEGER NOT NULL DEFAULT 0,
                collapsed  INTEGER NOT NULL DEFAULT 0,
                anchor     INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_mod_groups_profile ON mod_groups(profile_id);

            CREATE TABLE IF NOT EXISTS profile_mod_state (
                profile_id     INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                mod_id         TEXT NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
                enabled        INTEGER NOT NULL DEFAULT 0,
                priority       INTEGER NOT NULL DEFAULT 0,
                selection_json TEXT NOT NULL DEFAULT '{"chosen":[]}',
                group_id       INTEGER REFERENCES mod_groups(id) ON DELETE SET NULL,
                PRIMARY KEY (profile_id, mod_id)
            );

            CREATE TABLE IF NOT EXISTS deployments (
                id           TEXT PRIMARY KEY,
                game_id      TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                profile_id   INTEGER REFERENCES profiles(id) ON DELETE SET NULL,
                journal_path TEXT NOT NULL,
                state        TEXT NOT NULL,
                created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            "#,
        )?;
        // v2: record which mods a deployment covered, so the UI can show which
        // mods are actually live in the game folder right now.
        let has_col: bool = self
            .conn
            .prepare("SELECT deployed_mods FROM deployments LIMIT 1")
            .is_ok();
        if !has_col {
            let _ = self
                .conn
                .execute("ALTER TABLE deployments ADD COLUMN deployed_mods TEXT", []);
        }

        // v3: a per-profile record of which mod the user picked to win a
        // contested path, plus the Nexus ids an update check needs.
        //
        // mod_id cascades on delete because an override naming an uninstalled
        // mod is worse than no override at all: the deploy planner would hand
        // the path to a winner that has no files left to place there.
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS conflict_overrides (
                profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                game_rel_path TEXT NOT NULL,
                mod_id        TEXT NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
                PRIMARY KEY (profile_id, game_rel_path)
            );
            "#,
        )?;
        for column in ["nexus_mod_id", "nexus_file_id"] {
            let has_col: bool = self
                .conn
                .prepare(&format!("SELECT {column} FROM mods LIMIT 1"))
                .is_ok();
            if !has_col {
                let _ = self
                    .conn
                    .execute(&format!("ALTER TABLE mods ADD COLUMN {column} INTEGER"), []);
            }
        }

        // Staging stopped being addressed by mod id. Every row that predates the
        // split already has its files at `staging/<id>/`, so backfilling the key
        // to the id is what makes this migration move nothing on disk.
        let has_staging_key: bool = self
            .conn
            .prepare("SELECT staging_key FROM mods LIMIT 1")
            .is_ok();
        if !has_staging_key {
            let _ = self
                .conn
                .execute("ALTER TABLE mods ADD COLUMN staging_key TEXT", []);
        }
        self.conn.execute(
            "UPDATE mods SET staging_key = id WHERE staging_key IS NULL OR staging_key = ''",
            [],
        )?;

        // v5: where a downloaded archive came from, so importing it can tell an
        // update of something installed from a new mod that happens to share a
        // name.
        //
        // Deliberately no foreign key onto `mods`. The archive outlives the mod
        // it produced — a file stays in the downloads folder after its mod is
        // removed — and a cascade here would forget its origin at exactly the
        // moment someone might reinstall it. `replaces_mod_id` is a hint,
        // validated when it is used and never trusted on its own.
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS archive_provenance (
                archive_path    TEXT PRIMARY KEY,
                domain          TEXT,
                nexus_mod_id    INTEGER,
                nexus_file_id   INTEGER,
                replaces_mod_id TEXT,
                recorded_at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            "#,
        )?;

        // v6: the last game profiles the service published.
        //
        // A cache, and specifically a cache that survives a restart, which is
        // the whole reason it is on disk rather than in memory. Somebody who
        // fetched profiles yesterday and opens the app on a train should get
        // yesterday's profiles, not the ones compiled into their build months
        // ago. Losing this file costs one refresh and never a wrong answer.
        //
        // The document is stored as it arrived. Parsing it into columns here
        // would mean this table has to learn every field the profile schema
        // grows, and a cache that can fall behind the thing it caches is worse
        // than no cache.
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS game_profile_cache (
                game_id        TEXT PRIMARY KEY,
                document       TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                fetched_at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            "#,
        )?;

        // v7: named, lockable blocks of mods inside one profile's load order.
        //
        // Membership is a column rather than a position. The tempting model is
        // Mod Organizer's separators, where a group is a divider row and a mod
        // belongs to whichever divider sits above it, but that cannot be locked:
        // a lock is a promise about position, and membership derived from
        // position is defined by the very thing the lock freezes.
        //
        // `ON DELETE SET NULL` rather than a cascade, because deleting a group
        // is a statement about the grouping and never about the mods. SQLite
        // only accepts a REFERENCES clause on ADD COLUMN when the column
        // defaults to NULL, and NULL is exactly what "ungrouped" means, so the
        // two constraints happen to want the same thing.
        //
        // Nothing moves for an existing database: every row gets a NULL
        // `group_id`, no priority is rewritten, and an order with no groups in
        // it is unconstrained exactly as it was before.
        let has_group_id: bool = self
            .conn
            .prepare("SELECT group_id FROM profile_mod_state LIMIT 1")
            .is_ok();
        if !has_group_id {
            let _ = self.conn.execute(
                "ALTER TABLE profile_mod_state
                 ADD COLUMN group_id INTEGER REFERENCES mod_groups(id) ON DELETE SET NULL",
                [],
            );
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Replace the cached profiles with what the service just published.
    ///
    /// All of them at once: a profile that has been withdrawn should stop being
    /// used, and leaving it behind because this run happened not to mention it
    /// would keep a definition alive that nobody publishes any more.
    pub fn put_cached_profiles(&self, profiles: &[(String, String, i64)]) -> Result<()> {
        self.conn.execute("DELETE FROM game_profile_cache", [])?;
        let mut stmt = self.conn.prepare(
            "INSERT INTO game_profile_cache (game_id, document, schema_version) VALUES (?1, ?2, ?3)",
        )?;
        for (game_id, document, schema_version) in profiles {
            stmt.execute(params![game_id, document, schema_version])?;
        }
        Ok(())
    }

    /// The cached profile documents, as `(game_id, document)`.
    ///
    /// Only those written for a contract the caller understands: a document
    /// cached by a newer build is left where it is rather than handed to an
    /// older one that would read its fields to mean something else.
    pub fn cached_profiles(&self, schema_version: i64) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT game_id, document FROM game_profile_cache
             WHERE schema_version = ?1 ORDER BY game_id",
        )?;
        let rows = stmt.query_map(params![schema_version], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// When the cache was last written, in Unix seconds.
    pub fn profiles_fetched_at(&self) -> Result<Option<i64>> {
        let at: Option<i64> =
            self.conn
                .query_row("SELECT MAX(fetched_at) FROM game_profile_cache", [], |r| {
                    r.get(0)
                })?;
        Ok(at)
    }

    // ---- settings -------------------------------------------------------

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn game_db_source(&self) -> Result<GameDbSource> {
        Ok(self
            .get_setting("game_db_source")?
            .map(|s| GameDbSource::parse(&s))
            .unwrap_or(GameDbSource::LocalBuiltin))
    }

    pub fn set_game_db_source(&self, source: GameDbSource) -> Result<()> {
        self.set_setting("game_db_source", source.as_str())
    }

    // ---- games ----------------------------------------------------------

    pub fn upsert_game(&self, game: &GameRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO games(id,name,install_dir,proton_prefix,active_profile_id)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name,
                install_dir=excluded.install_dir,
                proton_prefix=excluded.proton_prefix",
            params![
                game.id,
                game.name,
                game.install_dir,
                game.proton_prefix,
                game.active_profile_id
            ],
        )?;
        Ok(())
    }

    pub fn get_game(&self, id: &str) -> Result<Option<GameRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,name,install_dir,proton_prefix,active_profile_id FROM games WHERE id=?1",
                params![id],
                |r| {
                    Ok(GameRecord {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        install_dir: r.get(2)?,
                        proton_prefix: r.get(3)?,
                        active_profile_id: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Point a game at one of **its own** profiles.
    ///
    /// The profile must belong to this game. Without that check a mismatched
    /// id would be accepted and every later lookup would silently read another
    /// game's mod states, which looks like the profile simply doing nothing.
    pub fn set_active_profile(&self, game_id: &str, profile_id: i64) -> Result<()> {
        let owned: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM profiles WHERE id=?1 AND game_id=?2",
                params![profile_id, game_id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if !owned {
            return Err(StorageError::NotFound(format!(
                "profile {profile_id} does not belong to game '{game_id}'"
            )));
        }
        self.conn.execute(
            "UPDATE games SET active_profile_id=?2 WHERE id=?1",
            params![game_id, profile_id],
        )?;
        Ok(())
    }

    // ---- mods -----------------------------------------------------------

    pub fn insert_mod(&self, rec: &ModRecord) -> Result<()> {
        let bundle_json = serde_json::to_string(&rec.bundle)?;
        self.conn.execute(
            "INSERT INTO mods(id,game_id,name,version,author,archive_path,archive_sha256,installer_model,bundle_json,nexus_mod_id,nexus_file_id,staging_key)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, version=excluded.version, author=excluded.author,
                archive_path=excluded.archive_path, archive_sha256=excluded.archive_sha256,
                installer_model=excluded.installer_model, bundle_json=excluded.bundle_json,
                nexus_mod_id=excluded.nexus_mod_id, nexus_file_id=excluded.nexus_file_id,
                staging_key=excluded.staging_key",
            params![
                rec.id, rec.game_id, rec.name, rec.version, rec.author,
                rec.archive_path, rec.archive_sha256, rec.installer_model, bundle_json,
                rec.nexus_mod_id, rec.nexus_file_id, rec.staging_key
            ],
        )?;
        Ok(())
    }

    fn row_to_mod(row: &rusqlite::Row) -> rusqlite::Result<ModRecord> {
        let bundle_json: String = row.get(8)?;
        let imported_at: i64 = row.get(9).unwrap_or(0);
        let bundle: ModBundle = serde_json::from_str(&bundle_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(ModRecord {
            id: row.get(0)?,
            game_id: row.get(1)?,
            name: row.get(2)?,
            version: row.get(3)?,
            author: row.get(4)?,
            archive_path: row.get(5)?,
            archive_sha256: row.get(6)?,
            installer_model: row.get(7)?,
            imported_at,
            nexus_mod_id: row.get(10)?,
            nexus_file_id: row.get(11)?,
            // A row written before the split has no key of its own; its files
            // are at the id, which is what the migration backfills. The fallback
            // is belt and braces for a row inserted between the two.
            staging_key: row
                .get::<_, Option<String>>(12)?
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| row.get::<_, String>(0).unwrap_or_default()),
            bundle,
        })
    }

    /// The column list every `ModRecord` read shares, so the positional indices
    /// in `row_to_mod` cannot drift between the two queries that use it.
    const MOD_COLUMNS: &'static str = "id,game_id,name,version,author,archive_path,\
         archive_sha256,installer_model,bundle_json,imported_at,nexus_mod_id,nexus_file_id,\
         staging_key";

    pub fn get_mod(&self, id: &str) -> Result<Option<ModRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {} FROM mods WHERE id=?1", Self::MOD_COLUMNS),
                params![id],
                Self::row_to_mod,
            )
            .optional()?)
    }

    pub fn list_mods(&self, game_id: &str) -> Result<Vec<ModRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM mods WHERE game_id=?1 ORDER BY imported_at, name",
            Self::MOD_COLUMNS
        ))?;
        let rows = stmt.query_map(params![game_id], Self::row_to_mod)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Mods that came from Nexus, as (mod_id, nexus_mod_id, nexus_file_id).
    /// An update check needs the file id to tell "newer file exists" from
    /// "same file, different name".
    pub fn nexus_linked_mods(&self, game_id: &str) -> Result<Vec<(String, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,nexus_mod_id,nexus_file_id FROM mods
             WHERE game_id=?1 AND nexus_mod_id IS NOT NULL AND nexus_file_id IS NOT NULL
             ORDER BY id",
        )?;
        let rows = stmt.query_map(params![game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every archive an installed mod was imported from, as `(path, mod name)`.
    ///
    /// The downloads list uses this to show which files are already in the
    /// library. Matched on path because that is exactly what import records and
    /// it costs nothing to compare, where matching on content would mean
    /// re-hashing every archive in the folder on every listing. A file moved or
    /// renamed since import therefore reads as "not installed", which is a
    /// conservative answer rather than a wrong one.
    pub fn installed_archives(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT archive_path, name FROM mods")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- archive provenance ---------------------------------------------

    /// Record where an archive came from, replacing any earlier note for the
    /// same path.
    ///
    /// Upserts because a path can be re-downloaded: the same file fetched again
    /// as a plain download after being fetched as an update should stop claiming
    /// to replace anything.
    pub fn record_archive_provenance(&self, p: &Provenance) -> Result<()> {
        self.conn.execute(
            "INSERT INTO archive_provenance(archive_path,domain,nexus_mod_id,nexus_file_id,replaces_mod_id)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(archive_path) DO UPDATE SET
                domain=excluded.domain, nexus_mod_id=excluded.nexus_mod_id,
                nexus_file_id=excluded.nexus_file_id,
                replaces_mod_id=excluded.replaces_mod_id",
            params![
                p.archive_path,
                p.domain,
                p.nexus_mod_id,
                p.nexus_file_id,
                p.replaces_mod_id
            ],
        )?;
        Ok(())
    }

    const PROVENANCE_COLUMNS: &'static str =
        "archive_path,domain,nexus_mod_id,nexus_file_id,replaces_mod_id";

    fn row_to_provenance(row: &rusqlite::Row) -> rusqlite::Result<Provenance> {
        Ok(Provenance {
            archive_path: row.get(0)?,
            domain: row.get(1)?,
            nexus_mod_id: row.get(2)?,
            nexus_file_id: row.get(3)?,
            replaces_mod_id: row.get(4)?,
        })
    }

    pub fn archive_provenance(&self, archive_path: &str) -> Result<Option<Provenance>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {} FROM archive_provenance WHERE archive_path=?1",
                    Self::PROVENANCE_COLUMNS
                ),
                params![archive_path],
                Self::row_to_provenance,
            )
            .optional()?)
    }

    /// Every archive's provenance, keyed by path.
    ///
    /// Whole-map like [`Store::conflict_overrides`], because the downloads list
    /// asks about every row it displays at once and one query beats one per row.
    pub fn all_archive_provenance(&self) -> Result<HashMap<String, Provenance>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM archive_provenance",
            Self::PROVENANCE_COLUMNS
        ))?;
        let rows = stmt.query_map([], Self::row_to_provenance)?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|p| (p.archive_path.clone(), p))
            .collect())
    }

    /// Every staging directory the library still needs, for one game.
    ///
    /// The keep-list for pruning: anything under the game's staging root that is
    /// not in here belongs to no mod and can go. Returned rather than a "find
    /// the stale ones" query so that deletion is always driven by what is known
    /// to be live, never by a pattern match on what looks dead.
    pub fn staging_keys(&self, game_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT staging_key FROM mods WHERE game_id=?1 AND staging_key IS NOT NULL")?;
        let rows = stmt.query_map(params![game_id], |r| r.get::<_, String>(0))?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|k| !k.is_empty())
            .collect())
    }

    pub fn delete_mod(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM mods WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- profiles -------------------------------------------------------

    pub fn create_profile(&self, game_id: &str, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO profiles(game_id,name) VALUES(?1,?2)",
            params![game_id, name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Get a profile by name, creating it if absent.
    pub fn ensure_profile(&self, game_id: &str, name: &str) -> Result<i64> {
        if let Some(id) = self
            .conn
            .query_row(
                "SELECT id FROM profiles WHERE game_id=?1 AND name=?2",
                params![game_id, name],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        self.create_profile(game_id, name)
    }

    pub fn list_profiles(&self, game_id: &str) -> Result<Vec<ProfileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,game_id,name FROM profiles WHERE game_id=?1 ORDER BY id")?;
        let rows = stmt.query_map(params![game_id], |r| {
            Ok(ProfileRecord {
                id: r.get(0)?,
                game_id: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_profile(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM profiles WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Copy every mod state from one profile into a new profile.
    pub fn clone_profile(&self, source_id: i64, new_name: &str) -> Result<i64> {
        let game_id: String = self.conn.query_row(
            "SELECT game_id FROM profiles WHERE id=?1",
            params![source_id],
            |r| r.get(0),
        )?;
        let new_id = self.create_profile(&game_id, new_name)?;

        // Groups first, and the copies get their own rows. Copying the state
        // rows alone would leave the new profile's mods pointing at the *source*
        // profile's groups, which no invariant here would ever catch: the
        // foreign key is satisfied, the rows exist, and the two profiles would
        // quietly share a lock.
        let tx = self.conn.unchecked_transaction()?;
        let mut remap: HashMap<i64, i64> = HashMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id,name,color,locked,collapsed,anchor FROM mod_groups WHERE profile_id=?1",
            )?;
            let rows = stmt.query_map(params![source_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (old_id, name, color, locked, collapsed, anchor) = row?;
                tx.execute(
                    "INSERT INTO mod_groups(profile_id,name,color,locked,collapsed,anchor)
                     VALUES(?1,?2,?3,?4,?5,?6)",
                    params![new_id, name, color, locked, collapsed, anchor],
                )?;
                remap.insert(old_id, tx.last_insert_rowid());
            }
        }

        tx.execute(
            "INSERT INTO profile_mod_state(profile_id,mod_id,enabled,priority,selection_json,group_id)
             SELECT ?1, mod_id, enabled, priority, selection_json, group_id
             FROM profile_mod_state WHERE profile_id=?2",
            params![new_id, source_id],
        )?;
        for (old_id, copy_id) in &remap {
            tx.execute(
                "UPDATE profile_mod_state SET group_id=?3 WHERE profile_id=?1 AND group_id=?2",
                params![new_id, old_id, copy_id],
            )?;
        }
        tx.commit()?;
        Ok(new_id)
    }

    // ---- per-profile mod state -----------------------------------------

    pub fn set_mod_state(&self, profile_id: i64, state: &ModState) -> Result<()> {
        let selection_json = serde_json::to_string(&state.selection)?;
        self.conn.execute(
            "INSERT INTO profile_mod_state(profile_id,mod_id,enabled,priority,selection_json,group_id)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(profile_id,mod_id) DO UPDATE SET
                enabled=excluded.enabled, priority=excluded.priority,
                selection_json=excluded.selection_json, group_id=excluded.group_id",
            params![
                profile_id,
                state.mod_id,
                state.enabled as i64,
                state.priority,
                selection_json,
                state.group_id
            ],
        )?;
        Ok(())
    }

    pub fn get_mod_state(&self, profile_id: i64, mod_id: &str) -> Result<Option<ModState>> {
        let row = self
            .conn
            .query_row(
                "SELECT mod_id,enabled,priority,selection_json,group_id
                 FROM profile_mod_state WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, mod_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((mod_id, enabled, priority, selection_json, group_id)) = row else {
            return Ok(None);
        };
        Ok(Some(ModState {
            mod_id,
            enabled: enabled != 0,
            priority,
            selection: serde_json::from_str(&selection_json)?,
            group_id,
        }))
    }

    /// All mod states in a profile, ordered by load-order priority.
    pub fn list_mod_states(&self, profile_id: i64) -> Result<Vec<ModState>> {
        let mut stmt = self.conn.prepare(
            "SELECT mod_id,enabled,priority,selection_json,group_id
             FROM profile_mod_state WHERE profile_id=?1 ORDER BY priority, mod_id",
        )?;
        let rows = stmt.query_map(params![profile_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mod_id, enabled, priority, selection_json, group_id) = row?;
            out.push(ModState {
                mod_id,
                enabled: enabled != 0,
                priority,
                selection: serde_json::from_str(&selection_json)?,
                group_id,
            });
        }
        Ok(out)
    }

    pub fn set_enabled(&self, profile_id: i64, mod_id: &str, enabled: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE profile_mod_state SET enabled=?3 WHERE profile_id=?1 AND mod_id=?2",
            params![profile_id, mod_id, enabled as i64],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound(format!(
                "mod {mod_id} is not in profile {profile_id}"
            )));
        }
        Ok(())
    }

    /// Enable or disable many mods in one profile, all or nothing.
    ///
    /// The bulk form exists because the single one is the wrong shape for it:
    /// turning off forty mods through [`Store::set_enabled`] is forty statements
    /// that can stop after nineteen, and the profile then describes a state
    /// nobody chose and nobody asked for.
    ///
    /// So this runs in a transaction, and an id that is not in the profile fails
    /// the whole batch — the same refusal [`Store::set_enabled`] already makes
    /// for one mod, applied to the set. Half a bulk action is worse than none,
    /// because the user's next move is to look at the list and believe it.
    ///
    /// [`Store::set_mod_order`] is the other multi-row writer and had the same
    /// weakness for longer. It is written the same way now, so this file has one
    /// answer to "what happens halfway through" rather than two.
    pub fn set_enabled_bulk(
        &self,
        profile_id: i64,
        mod_ids: &[String],
        enabled: bool,
    ) -> Result<usize> {
        // `unchecked_transaction` rather than `transaction`, because `Store`
        // owns its `Connection` behind `&self` and the checked form needs
        // `&mut self`. The check it skips is a compile-time one about nesting,
        // and nothing here nests.
        let tx = self.conn.unchecked_transaction()?;
        for id in mod_ids {
            let changed = tx.execute(
                "UPDATE profile_mod_state SET enabled=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, id, enabled as i64],
            )?;
            if changed == 0 {
                // Dropping `tx` would roll back on its own; rolling back
                // explicitly says so where somebody is reading for the failure
                // path rather than inferring it from a `?`.
                tx.rollback()?;
                return Err(StorageError::NotFound(format!(
                    "mod {id} is not in profile {profile_id}"
                )));
            }
        }
        tx.commit()?;
        Ok(mod_ids.len())
    }

    // ---- conflict overrides ---------------------------------------------

    /// Pin `game_rel_path` to `mod_id` for this profile, overriding load order.
    ///
    /// Keyed by path rather than by pair of mods so the choice survives the
    /// other mod being reordered, disabled, or replaced by a third one.
    pub fn set_conflict_override(
        &self,
        profile_id: i64,
        game_rel_path: &str,
        mod_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO conflict_overrides(profile_id,game_rel_path,mod_id)
             VALUES(?1,?2,?3)
             ON CONFLICT(profile_id,game_rel_path) DO UPDATE SET mod_id=excluded.mod_id",
            params![profile_id, game_rel_path, mod_id],
        )?;
        Ok(())
    }

    /// Drop the override for one path, returning that path to load-order rules.
    pub fn clear_conflict_override(&self, profile_id: i64, game_rel_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM conflict_overrides WHERE profile_id=?1 AND game_rel_path=?2",
            params![profile_id, game_rel_path],
        )?;
        Ok(())
    }

    /// Every override in a profile, as `game_rel_path -> mod_id`. Returned whole
    /// because the deploy planner consults it once per path it is about to
    /// place, and a per-path query there would be one round trip per file.
    pub fn conflict_overrides(&self, profile_id: i64) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT game_rel_path,mod_id FROM conflict_overrides WHERE profile_id=?1")?;
        let rows = stmt.query_map(params![profile_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    // ---- deployments ----------------------------------------------------

    pub fn record_deployment(
        &self,
        id: &str,
        game_id: &str,
        profile_id: Option<i64>,
        journal_path: &str,
        state: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO deployments(id,game_id,profile_id,journal_path,state)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state",
            params![id, game_id, profile_id, journal_path, state],
        )?;
        Ok(())
    }

    /// Record which mods a deployment covered.
    pub fn set_deployed_mods(&self, deployment_id: &str, mod_ids: &[String]) -> Result<()> {
        let json = serde_json::to_string(mod_ids)?;
        self.conn.execute(
            "UPDATE deployments SET deployed_mods=?2 WHERE id=?1",
            params![deployment_id, json],
        )?;
        Ok(())
    }

    /// Mod ids whose files are currently in the game folder, across every
    /// deployment still marked applied.
    pub fn applied_mod_ids(&self, game_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT deployed_mods FROM deployments
             WHERE game_id=?1 AND state='applied' AND deployed_mods IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![game_id], |r| r.get::<_, String>(0))?;
        let mut out: Vec<String> = Vec::new();
        for row in rows {
            let ids: Vec<String> = serde_json::from_str(&row?).unwrap_or_default();
            for id in ids {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        Ok(out)
    }

    /// Rewrite load-order priority for a profile from an ordered id list, all
    /// or nothing.
    ///
    /// A load order is a single arrangement rather than a list of independent
    /// facts, so half of one is not a partial success: it is an order the user
    /// did not choose, and the file that wins each conflict follows from it.
    /// This ran as bare statements outside a transaction until now, which meant
    /// a failure partway left exactly that.
    ///
    /// An id that is not in the profile fails the whole batch, the same refusal
    /// [`Store::set_enabled`] and [`Store::set_enabled_bulk`] already make.
    /// Silently writing nothing for it was the older behaviour and the worse
    /// one: the caller was told the order it asked for had been stored when the
    /// stored order was a different arrangement.
    ///
    /// An order that would disturb a locked group is refused the same way, and
    /// for the same reason. This is the chokepoint every writer passes through:
    /// the screen, the command line, and whatever sorts a list automatically
    /// later. A guard in the screen would be an affordance, not a rule.
    pub fn set_mod_order(&self, profile_id: i64, ordered_ids: &[String]) -> Result<()> {
        self.check_locks(profile_id, ordered_ids)?;
        // `unchecked_transaction` for the reason given on `set_enabled_bulk`:
        // `Store` owns its `Connection` behind `&self`, and the checked form
        // needs `&mut self` to rule out a nesting that does not happen here.
        let tx = self.conn.unchecked_transaction()?;
        for (index, id) in ordered_ids.iter().enumerate() {
            let changed = tx.execute(
                "UPDATE profile_mod_state SET priority=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, id, index as i64],
            )?;
            if changed == 0 {
                tx.rollback()?;
                return Err(StorageError::NotFound(format!(
                    "mod {id} is not in profile {profile_id}"
                )));
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- groups ---------------------------------------------------------

    /// The load order as ids alone, which is what the group rules are stated in.
    fn order_of(&self, profile_id: i64) -> Result<Vec<String>> {
        Ok(self
            .list_mod_states(profile_id)?
            .into_iter()
            .map(|s| s.mod_id)
            .collect())
    }

    /// Which group each mod in this profile belongs to.
    pub fn membership(&self, profile_id: i64) -> Result<Membership> {
        let mut out = Membership::new();
        for state in self.list_mod_states(profile_id)? {
            if let Some(group_id) = state.group_id {
                out.insert(state.mod_id, group_id);
            }
        }
        Ok(out)
    }

    /// Every group in a profile, in the order their blocks appear.
    pub fn list_groups(&self, profile_id: i64) -> Result<Vec<ModGroupRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,name,color,locked,collapsed,anchor
             FROM mod_groups WHERE profile_id=?1",
        )?;
        let rows = stmt.query_map(params![profile_id], |r| {
            Ok(ModGroupRecord {
                group: ModGroup {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    color: r.get(2)?,
                    locked: r.get::<_, i64>(3)? != 0,
                    collapsed: r.get::<_, i64>(4)? != 0,
                },
                profile_id,
                anchor: r.get(5)?,
            })
        })?;
        let mut groups: Vec<ModGroupRecord> = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        // Sorted by where each block actually sits, so the caller never has to
        // work that out and never disagrees with the screen about it.
        let order = self.order_of(profile_id)?;
        let membership = self.membership(profile_id)?;
        groups.sort_by_key(|g| {
            order
                .iter()
                .position(|id| membership.get(id) == Some(&g.group.id))
                .map(|p| p as i64)
                .unwrap_or(g.anchor)
        });
        Ok(groups)
    }

    /// The domain view of a profile's groups, for the ordering rules.
    fn groups_only(&self, profile_id: i64) -> Result<Vec<ModGroup>> {
        Ok(self
            .list_groups(profile_id)?
            .into_iter()
            .map(|g| g.group)
            .collect())
    }

    fn group_name(&self, group_id: i64) -> Result<String> {
        Ok(self
            .conn
            .query_row(
                "SELECT name FROM mod_groups WHERE id=?1",
                params![group_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| format!("group {group_id}")))
    }

    fn mod_name(&self, mod_id: &str) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT name FROM mods WHERE id=?1", params![mod_id], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_else(|| mod_id.to_string()))
    }

    /// Turn a breach into the sentence the person who set the lock will read.
    fn refuse(&self, breach: LockBreach) -> StorageError {
        let group = self
            .group_name(breach.group_id())
            .unwrap_or_else(|_| "this group".into());
        let name = self
            .mod_name(breach.mod_id())
            .unwrap_or_else(|_| breach.mod_id().to_string());
        StorageError::LockedGroup(match breach {
            LockBreach::MemberMoved { .. } => format!(
                "\"{group}\" is locked, so its mods stay together in the order you set. \
                 Unlock it to move \"{name}\"."
            ),
            LockBreach::Split { .. } => format!(
                "\"{group}\" is locked, so nothing can sit between its mods. \
                 Unlock it to put \"{name}\" inside it."
            ),
        })
    }

    fn check_locks(&self, profile_id: i64, requested: &[String]) -> Result<()> {
        let groups = self.groups_only(profile_id)?;
        if groups.iter().all(|g| !g.locked) {
            return Ok(());
        }
        let current = self.order_of(profile_id)?;
        let membership = self.membership(profile_id)?;
        modgroups::check_order(&current, requested, &groups, &membership)
            .map_err(|breach| self.refuse(breach))
    }

    /// Replay one drag and store what it produced, returning the new order.
    ///
    /// The screen sends the drag rather than the arrangement it thinks resulted,
    /// because the screen can be searched, filtered and collapsed: the row above
    /// a drop point is very often not the entry above it in the true order. An
    /// anchor id means the same thing in every one of those views, and applies
    /// against the order as the store currently holds it rather than the one the
    /// client last saw.
    pub fn move_in_order(&self, profile_id: i64, mv: &OrderMove) -> Result<Vec<String>> {
        let current = self.order_of(profile_id)?;
        let groups = self.groups_only(profile_id)?;
        let membership = self.membership(profile_id)?;

        let Arrangement { order, regrouped } =
            modgroups::apply_move(&current, &groups, &membership, mv)
                .map_err(|breach| self.refuse(breach))?;

        let tx = self.conn.unchecked_transaction()?;
        for (mod_id, group_id) in &regrouped {
            tx.execute(
                "UPDATE profile_mod_state SET group_id=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, mod_id, group_id],
            )?;
        }
        for (index, id) in order.iter().enumerate() {
            let changed = tx.execute(
                "UPDATE profile_mod_state SET priority=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, id, index as i64],
            )?;
            if changed == 0 {
                tx.rollback()?;
                return Err(StorageError::NotFound(format!(
                    "mod {id} is not in profile {profile_id}"
                )));
            }
        }
        // A group that just lost its last member has nothing left positioning
        // it, so it records where it was before it can be forgotten.
        self.park_empty_groups(&tx, profile_id, &order)?;
        tx.commit()?;
        Ok(order)
    }

    /// Give every memberless group an anchor, so it keeps its place in the list.
    fn park_empty_groups(
        &self,
        tx: &rusqlite::Transaction<'_>,
        profile_id: i64,
        order: &[String],
    ) -> Result<()> {
        let membership = self.membership(profile_id)?;
        let mut stmt = tx.prepare("SELECT id FROM mod_groups WHERE profile_id=?1")?;
        let ids: Vec<i64> = stmt
            .query_map(params![profile_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for group_id in ids {
            let first = order
                .iter()
                .position(|id| membership.get(id) == Some(&group_id));
            if first.is_none() {
                continue;
            }
            tx.execute(
                "UPDATE mod_groups SET anchor=?2 WHERE id=?1",
                params![group_id, first.unwrap() as i64],
            )?;
        }
        Ok(())
    }

    /// Create a group, positioned where the caller says and holding nothing yet.
    pub fn create_group(&self, profile_id: i64, name: &str, color: &str) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let anchor: i64 = tx.query_row(
            "SELECT COALESCE(MAX(priority), -1) + 1 FROM profile_mod_state WHERE profile_id=?1",
            params![profile_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO mod_groups(profile_id,name,color,anchor) VALUES(?1,?2,?3,?4)",
            params![profile_id, name, color, anchor],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// Rename, recolour, or collapse a group.
    ///
    /// Allowed while locked, all three of them. A lock holds the order; refusing
    /// to fix a typo in the name would teach people to unlock out of habit,
    /// which costs the lock the only thing it is for.
    pub fn update_group(
        &self,
        group_id: i64,
        name: Option<&str>,
        color: Option<&str>,
        collapsed: Option<bool>,
    ) -> Result<()> {
        if let Some(name) = name {
            self.conn.execute(
                "UPDATE mod_groups SET name=?2 WHERE id=?1",
                params![group_id, name],
            )?;
        }
        if let Some(color) = color {
            self.conn.execute(
                "UPDATE mod_groups SET color=?2 WHERE id=?1",
                params![group_id, color],
            )?;
        }
        if let Some(collapsed) = collapsed {
            self.conn.execute(
                "UPDATE mod_groups SET collapsed=?2 WHERE id=?1",
                params![group_id, collapsed as i64],
            )?;
        }
        Ok(())
    }

    /// Lock or unlock a group.
    ///
    /// Locking gathers the members first. A lock is somebody asking for a
    /// guarantee, and latching one onto a group whose mods are scattered would
    /// promise an arrangement that does not exist and then refuse every attempt
    /// to reach one.
    pub fn set_group_locked(&self, profile_id: i64, group_id: i64, locked: bool) -> Result<()> {
        if locked {
            let order = self.order_of(profile_id)?;
            let membership = self.membership(profile_id)?;
            let gathered = modgroups::gather(&order, &membership);
            let tx = self.conn.unchecked_transaction()?;
            for (index, id) in gathered.iter().enumerate() {
                tx.execute(
                    "UPDATE profile_mod_state SET priority=?3 WHERE profile_id=?1 AND mod_id=?2",
                    params![profile_id, id, index as i64],
                )?;
            }
            tx.execute(
                "UPDATE mod_groups SET locked=1 WHERE id=?1",
                params![group_id],
            )?;
            tx.commit()?;
            return Ok(());
        }
        self.conn.execute(
            "UPDATE mod_groups SET locked=0 WHERE id=?1",
            params![group_id],
        )?;
        Ok(())
    }

    /// Put mods in a group, gathering them beside the members already there.
    pub fn assign_to_group(
        &self,
        profile_id: i64,
        group_id: Option<i64>,
        mod_ids: &[String],
    ) -> Result<()> {
        let groups = self.groups_only(profile_id)?;
        let membership = self.membership(profile_id)?;
        let locked = |id: i64| groups.iter().any(|g| g.id == id && g.locked);

        for mod_id in mod_ids {
            if let Some(from) = membership.get(mod_id) {
                if locked(*from) && Some(*from) != group_id {
                    return Err(self.refuse(LockBreach::MemberMoved {
                        group_id: *from,
                        mod_id: mod_id.clone(),
                    }));
                }
            }
            if let Some(to) = group_id {
                if locked(to) && membership.get(mod_id) != Some(&to) {
                    return Err(self.refuse(LockBreach::Split {
                        group_id: to,
                        mod_id: mod_id.clone(),
                    }));
                }
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        for mod_id in mod_ids {
            let changed = tx.execute(
                "UPDATE profile_mod_state SET group_id=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, mod_id, group_id],
            )?;
            if changed == 0 {
                tx.rollback()?;
                return Err(StorageError::NotFound(format!(
                    "mod {mod_id} is not in profile {profile_id}"
                )));
            }
        }
        tx.commit()?;

        // Gathering is a second write on purpose: membership decides where the
        // run is, so it has to be true before the run can be worked out.
        let order = self.order_of(profile_id)?;
        let membership = self.membership(profile_id)?;
        let gathered = modgroups::gather(&order, &membership);
        let tx = self.conn.unchecked_transaction()?;
        for (index, id) in gathered.iter().enumerate() {
            tx.execute(
                "UPDATE profile_mod_state SET priority=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, id, index as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete a group. Its mods keep every priority they had.
    ///
    /// Deleting a group is a statement about the grouping and never about the
    /// mods, so nothing is gathered, nothing is compacted, and the deployment it
    /// plans is byte for byte the one it planned before.
    pub fn delete_group(&self, profile_id: i64, group_id: i64) -> Result<()> {
        let groups = self.groups_only(profile_id)?;
        if groups.iter().any(|g| g.id == group_id && g.locked) {
            let name = self.group_name(group_id)?;
            return Err(StorageError::LockedGroup(format!(
                "\"{name}\" is locked. Unlock it before deleting it."
            )));
        }
        self.conn
            .execute("DELETE FROM mod_groups WHERE id=?1", params![group_id])?;
        Ok(())
    }

    /// The priority a newly imported mod should take: after everything else.
    ///
    /// Every never-ordered mod used to be written at zero and the list breaks
    /// ties by id, so an import landed wherever the alphabet put it, which with
    /// groups means it could land inside one.
    pub fn next_priority(&self, profile_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(priority), -1) + 1 FROM profile_mod_state WHERE profile_id=?1",
            params![profile_id],
            |r| r.get(0),
        )?)
    }

    /// Every deployment still marked applied, newest first. Reconciling the game
    /// directory means reverting all of them, not just the most recent: earlier
    /// partial deployments would otherwise leave files behind forever.
    pub fn applied_deployments(&self, game_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,journal_path FROM deployments
             WHERE game_id=?1 AND state='applied'
             ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map(params![game_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn latest_deployment(&self, game_id: &str) -> Result<Option<(String, String, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,journal_path,state FROM deployments
                 WHERE game_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                params![game_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::modgroups::{Belonging, MoveSubject, Placement};
    use apoc_domain::InstallerModel;

    fn empty_bundle(name: &str) -> ModBundle {
        ModBundle {
            name: name.into(),
            version: Some("v1".into()),
            author: None,
            category: None,
            installer_model: InstallerModel::FluffyAio,
            archive_sha256: None,
            fomod: None,
            unclaimed_root_files: Vec::new(),
            groups: vec![],
        }
    }

    #[test]
    fn a_profile_cannot_be_created_for_a_game_that_is_not_a_row() {
        // profiles.game_id is a foreign key onto games. This is the constraint
        // that surfaced as "database error: FOREIGN KEY constraint failed" on a
        // fresh database, because nothing wrote the game row until detection
        // ran. The failure is correct — the caller must create the game first —
        // so this pins the behaviour rather than the bug.
        let s = Store::open_in_memory().unwrap();
        assert!(
            s.ensure_profile("never-seen", "Default").is_err(),
            "a profile must not attach to a game that does not exist"
        );
    }

    #[test]
    fn a_profile_can_be_created_once_the_game_exists() {
        let s = Store::open_in_memory().unwrap();
        s.upsert_game(&GameRecord {
            id: "cyberpunk-2077".into(),
            name: "Cyberpunk 2077".into(),
            // Undetected: identity without a path is exactly the state the app
            // is in before anyone presses Find game.
            install_dir: None,
            proton_prefix: None,
            active_profile_id: None,
        })
        .unwrap();
        assert!(s.ensure_profile("cyberpunk-2077", "Default").is_ok());
    }

    fn seeded() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.upsert_game(&GameRecord {
            id: "monster-hunter-wilds".into(),
            name: "Monster Hunter Wilds".into(),
            install_dir: Some("/games/mhw".into()),
            proton_prefix: None,
            active_profile_id: None,
        })
        .unwrap();
        s
    }

    fn add_mod(s: &Store, id: &str, nexus: Option<(i64, i64)>) {
        s.insert_mod(&ModRecord {
            id: id.into(),
            game_id: "monster-hunter-wilds".into(),
            name: id.into(),
            version: None,
            author: None,
            archive_path: format!("/downloads/{id}.zip"),
            archive_sha256: None,
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: nexus.map(|(m, _)| m),
            nexus_file_id: nexus.map(|(_, f)| f),
            staging_key: id.into(),
            bundle: empty_bundle(id),
        })
        .unwrap();
    }

    #[test]
    fn defaults_to_the_local_builtin_game_database() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.game_db_source().unwrap(), GameDbSource::LocalBuiltin);
        s.set_game_db_source(GameDbSource::OnlineApi).unwrap();
        assert_eq!(s.game_db_source().unwrap(), GameDbSource::OnlineApi);
    }

    #[test]
    fn mods_round_trip_with_their_bundle() {
        let s = seeded();
        s.insert_mod(&ModRecord {
            id: "mod-1".into(),
            game_id: "monster-hunter-wilds".into(),
            name: "Ver.R Hirabami F-M Armor".into(),
            version: Some("v1.03".into()),
            author: Some("Ranaragua".into()),
            archive_path: "/tmp/a.zip".into(),
            archive_sha256: Some("abc".into()),
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: None,
            nexus_file_id: None,
            staging_key: String::new(),
            bundle: empty_bundle("Ver.R Hirabami F-M Armor"),
        })
        .unwrap();

        let got = s.get_mod("mod-1").unwrap().unwrap();
        assert_eq!(got.name, "Ver.R Hirabami F-M Armor");
        assert_eq!(got.bundle.version.as_deref(), Some("v1"));
        assert_eq!(s.list_mods("monster-hunter-wilds").unwrap().len(), 1);
    }

    #[test]
    fn installed_archives_report_where_each_mod_came_from() {
        let s = seeded();
        for (id, name, archive) in [
            ("mod-1", "Body Sliders", "/downloads/sliders.rar"),
            ("mod-2", "Ebony Armor", "/downloads/ebony.zip"),
        ] {
            s.insert_mod(&ModRecord {
                id: id.into(),
                game_id: "monster-hunter-wilds".into(),
                name: name.into(),
                version: None,
                author: None,
                archive_path: archive.into(),
                archive_sha256: None,
                installer_model: "fluffy-aio".into(),
                imported_at: 0,
                nexus_mod_id: None,
                nexus_file_id: None,
                staging_key: String::new(),
                bundle: empty_bundle(name),
            })
            .unwrap();
        }

        let map: std::collections::HashMap<String, String> =
            s.installed_archives().unwrap().into_iter().collect();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("/downloads/sliders.rar").map(String::as_str),
            Some("Body Sliders"),
        );
        // A file that was never imported must not appear, or the downloads list
        // would claim something is installed when it is not.
        assert!(!map.contains_key("/downloads/never-installed.zip"));
    }

    #[test]
    fn profile_selections_are_independent() {
        let s = seeded();
        s.insert_mod(&ModRecord {
            id: "mod-1".into(),
            game_id: "monster-hunter-wilds".into(),
            name: "M".into(),
            version: None,
            author: None,
            archive_path: "/tmp/a.zip".into(),
            archive_sha256: None,
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: None,
            nexus_file_id: None,
            staging_key: String::new(),
            bundle: empty_bundle("M"),
        })
        .unwrap();

        let vanilla = s.ensure_profile("monster-hunter-wilds", "Vanilla").unwrap();
        let testing = s.ensure_profile("monster-hunter-wilds", "Testing").unwrap();

        let mut sel_a = Selection::new();
        sel_a.insert("Helm-01");
        s.set_mod_state(
            vanilla,
            &ModState {
                mod_id: "mod-1".into(),
                enabled: true,
                priority: 0,
                selection: sel_a,
                group_id: None,
            },
        )
        .unwrap();

        let mut sel_b = Selection::new();
        sel_b.insert("Helm-03");
        s.set_mod_state(
            testing,
            &ModState {
                mod_id: "mod-1".into(),
                enabled: false,
                priority: 5,
                selection: sel_b,
                group_id: None,
            },
        )
        .unwrap();

        let a = s.get_mod_state(vanilla, "mod-1").unwrap().unwrap();
        let b = s.get_mod_state(testing, "mod-1").unwrap().unwrap();
        assert!(a.enabled && a.selection.contains("Helm-01"));
        assert!(!b.enabled && b.selection.contains("Helm-03"));
    }

    #[test]
    fn cloning_a_profile_copies_its_state() {
        let s = seeded();
        s.insert_mod(&ModRecord {
            id: "mod-1".into(),
            game_id: "monster-hunter-wilds".into(),
            name: "M".into(),
            version: None,
            author: None,
            archive_path: "/a.zip".into(),
            archive_sha256: None,
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: None,
            nexus_file_id: None,
            staging_key: String::new(),
            bundle: empty_bundle("M"),
        })
        .unwrap();
        let base = s.ensure_profile("monster-hunter-wilds", "Base").unwrap();
        let mut sel = Selection::new();
        sel.insert("Body-02");
        s.set_mod_state(
            base,
            &ModState {
                mod_id: "mod-1".into(),
                enabled: true,
                priority: 3,
                selection: sel,
                group_id: None,
            },
        )
        .unwrap();

        let copy = s.clone_profile(base, "Copy").unwrap();
        let st = s.get_mod_state(copy, "mod-1").unwrap().unwrap();
        assert!(st.enabled);
        assert_eq!(st.priority, 3);
        assert!(st.selection.contains("Body-02"));
    }

    #[test]
    fn applied_mod_ids_reflect_only_live_deployments() {
        let s = seeded();
        s.record_deployment("d1", "monster-hunter-wilds", None, "/j1", "applied")
            .unwrap();
        s.set_deployed_mods("d1", &["mod-a".into(), "mod-b".into()])
            .unwrap();
        s.record_deployment("d2", "monster-hunter-wilds", None, "/j2", "applied")
            .unwrap();
        s.set_deployed_mods("d2", &["mod-c".into()]).unwrap();

        let mut live = s.applied_mod_ids("monster-hunter-wilds").unwrap();
        live.sort();
        assert_eq!(live, vec!["mod-a", "mod-b", "mod-c"]);

        // Reverting one deployment drops its mods from the live set.
        s.record_deployment("d1", "monster-hunter-wilds", None, "/j1", "reverted")
            .unwrap();
        assert_eq!(
            s.applied_mod_ids("monster-hunter-wilds").unwrap(),
            vec!["mod-c"]
        );
    }

    #[test]
    fn reordering_rewrites_priority_from_the_given_sequence() {
        let s = seeded();
        for id in ["mod-a", "mod-b", "mod-c"] {
            s.insert_mod(&ModRecord {
                id: id.into(),
                game_id: "monster-hunter-wilds".into(),
                name: id.into(),
                version: None,
                author: None,
                archive_path: "/a.zip".into(),
                archive_sha256: None,
                installer_model: "fluffy-aio".into(),
                imported_at: 0,
                nexus_mod_id: None,
                nexus_file_id: None,
                staging_key: String::new(),
                bundle: empty_bundle(id),
            })
            .unwrap();
        }
        let p = s.ensure_profile("monster-hunter-wilds", "P").unwrap();
        for id in ["mod-a", "mod-b", "mod-c"] {
            s.set_mod_state(
                p,
                &ModState {
                    mod_id: id.into(),
                    enabled: true,
                    priority: 0,
                    selection: Selection::new(),
                    group_id: None,
                },
            )
            .unwrap();
        }

        s.set_mod_order(p, &["mod-c".into(), "mod-a".into(), "mod-b".into()])
            .unwrap();
        let order: Vec<String> = s
            .list_mod_states(p)
            .unwrap()
            .into_iter()
            .map(|m| m.mod_id)
            .collect();
        assert_eq!(order, vec!["mod-c", "mod-a", "mod-b"]);
    }

    #[test]
    fn enabling_a_mod_not_in_the_profile_is_an_error() {
        let s = seeded();
        let p = s.ensure_profile("monster-hunter-wilds", "P").unwrap();
        assert!(matches!(
            s.set_enabled(p, "ghost", true),
            Err(StorageError::NotFound(_))
        ));
    }

    /// A profile of `count` mods, all enabled, for the bulk tests.
    fn profile_of_enabled_mods(s: &Store, count: usize) -> i64 {
        let p = s.ensure_profile("monster-hunter-wilds", "P").unwrap();
        for i in 0..count {
            let id = format!("mod-{i}");
            add_mod(s, &id, None);
            s.set_mod_state(
                p,
                &ModState {
                    mod_id: id,
                    enabled: true,
                    priority: i as i64,
                    selection: Selection::new(),
                    group_id: None,
                },
            )
            .unwrap();
        }
        p
    }

    fn enabled_ids(s: &Store, p: i64) -> Vec<String> {
        s.list_mod_states(p)
            .unwrap()
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| m.mod_id)
            .collect()
    }

    #[test]
    fn a_bulk_toggle_changes_exactly_the_mods_it_names() {
        let s = seeded();
        let p = profile_of_enabled_mods(&s, 5);

        let n = s
            .set_enabled_bulk(p, &["mod-1".into(), "mod-3".into()], false)
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(enabled_ids(&s, p), vec!["mod-0", "mod-2", "mod-4"]);

        // And back, because a bulk enable is the same operation with the other
        // answer and is the one a user reaches for after changing their mind.
        s.set_enabled_bulk(p, &["mod-1".into(), "mod-3".into()], true)
            .unwrap();
        assert_eq!(
            enabled_ids(&s, p),
            vec!["mod-0", "mod-1", "mod-2", "mod-3", "mod-4"]
        );
    }

    #[test]
    fn one_unknown_id_rolls_the_whole_batch_back() {
        // The reason this is a transaction. The unknown id sits in the middle,
        // so two mods have already been written by the time it is reached: a
        // loop of bare statements would leave those two off and report failure,
        // and the user would be looking at a list nobody chose.
        let s = seeded();
        let p = profile_of_enabled_mods(&s, 4);

        let err = s.set_enabled_bulk(
            p,
            &[
                "mod-0".into(),
                "mod-1".into(),
                "ghost".into(),
                "mod-2".into(),
            ],
            false,
        );
        assert!(matches!(err, Err(StorageError::NotFound(_))));
        assert_eq!(
            enabled_ids(&s, p),
            vec!["mod-0", "mod-1", "mod-2", "mod-3"],
            "a failed batch must leave every mod as it was"
        );
    }

    #[test]
    fn an_empty_batch_is_a_no_op_rather_than_an_error() {
        // "Disable selected" with nothing selected is a UI state that should
        // cost nothing, not one the store refuses.
        let s = seeded();
        let p = profile_of_enabled_mods(&s, 2);
        assert_eq!(s.set_enabled_bulk(p, &[], false).unwrap(), 0);
        assert_eq!(enabled_ids(&s, p), vec!["mod-0", "mod-1"]);
    }

    /// The load order, by mod id.
    fn order_of(s: &Store, p: i64) -> Vec<String> {
        s.list_mod_states(p)
            .unwrap()
            .into_iter()
            .map(|m| m.mod_id)
            .collect()
    }

    #[test]
    fn a_reorder_naming_a_mod_that_is_not_there_changes_nothing() {
        // The absent id sits in the middle, so two priorities have already been
        // rewritten by the time it is reached. Before this was a transaction
        // those two writes stood, the call reported success, and the stored
        // order was an arrangement nobody asked for -- which then decided who
        // won every contested file.
        let s = seeded();
        let p = profile_of_enabled_mods(&s, 4);
        let before = order_of(&s, p);

        let err = s.set_mod_order(
            p,
            &[
                "mod-3".into(),
                "mod-2".into(),
                "ghost".into(),
                "mod-1".into(),
                "mod-0".into(),
            ],
        );

        assert!(matches!(err, Err(StorageError::NotFound(_))));
        assert_eq!(
            order_of(&s, p),
            before,
            "a refused reorder must leave the order it found"
        );
    }

    #[test]
    fn deleting_a_game_cascades_to_its_mods_and_profiles() {
        let s = seeded();
        s.insert_mod(&ModRecord {
            id: "mod-1".into(),
            game_id: "monster-hunter-wilds".into(),
            name: "M".into(),
            version: None,
            author: None,
            archive_path: "/a.zip".into(),
            archive_sha256: None,
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: None,
            nexus_file_id: None,
            staging_key: String::new(),
            bundle: empty_bundle("M"),
        })
        .unwrap();
        s.ensure_profile("monster-hunter-wilds", "P").unwrap();

        s.conn
            .execute("DELETE FROM games WHERE id='monster-hunter-wilds'", [])
            .unwrap();
        assert!(s.get_mod("mod-1").unwrap().is_none());
        assert!(s.list_profiles("monster-hunter-wilds").unwrap().is_empty());
    }

    #[test]
    fn a_game_cannot_be_pointed_at_another_games_profile() {
        // Accepting a foreign id would make every later lookup read the wrong
        // profile's mod states, which presents as the profile doing nothing.
        let s = seeded();
        s.upsert_game(&GameRecord {
            id: "cyberpunk-2077".into(),
            name: "Cyberpunk 2077".into(),
            install_dir: None,
            proton_prefix: None,
            active_profile_id: None,
        })
        .unwrap();
        let theirs = s.ensure_profile("cyberpunk-2077", "Default").unwrap();

        assert!(matches!(
            s.set_active_profile("monster-hunter-wilds", theirs),
            Err(StorageError::NotFound(_))
        ));

        let ours = s.ensure_profile("monster-hunter-wilds", "Default").unwrap();
        s.set_active_profile("monster-hunter-wilds", ours).unwrap();
        assert_eq!(
            s.get_game("monster-hunter-wilds")
                .unwrap()
                .and_then(|g| g.active_profile_id),
            Some(ours)
        );
    }

    #[test]
    fn conflict_overrides_round_trip_and_are_independent_between_profiles() {
        let s = seeded();
        add_mod(&s, "mod-a", None);
        add_mod(&s, "mod-b", None);
        let vanilla = s.ensure_profile("monster-hunter-wilds", "Vanilla").unwrap();
        let testing = s.ensure_profile("monster-hunter-wilds", "Testing").unwrap();

        s.set_conflict_override(vanilla, "natives/stm/a.pak", "mod-a")
            .unwrap();
        s.set_conflict_override(testing, "natives/stm/a.pak", "mod-b")
            .unwrap();

        assert_eq!(
            s.conflict_overrides(vanilla)
                .unwrap()
                .get("natives/stm/a.pak"),
            Some(&"mod-a".to_string())
        );
        assert_eq!(
            s.conflict_overrides(testing)
                .unwrap()
                .get("natives/stm/a.pak"),
            Some(&"mod-b".to_string())
        );

        // Re-pinning the same path replaces the winner instead of erroring.
        s.set_conflict_override(vanilla, "natives/stm/a.pak", "mod-b")
            .unwrap();
        assert_eq!(
            s.conflict_overrides(vanilla)
                .unwrap()
                .get("natives/stm/a.pak"),
            Some(&"mod-b".to_string())
        );

        s.clear_conflict_override(vanilla, "natives/stm/a.pak")
            .unwrap();
        assert!(s.conflict_overrides(vanilla).unwrap().is_empty());
        assert_eq!(s.conflict_overrides(testing).unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_mod_removes_the_overrides_that_pointed_at_it() {
        let s = seeded();
        add_mod(&s, "mod-a", None);
        add_mod(&s, "mod-b", None);
        let p = s.ensure_profile("monster-hunter-wilds", "P").unwrap();
        s.set_conflict_override(p, "natives/stm/a.pak", "mod-a")
            .unwrap();
        s.set_conflict_override(p, "natives/stm/b.pak", "mod-b")
            .unwrap();

        s.delete_mod("mod-a").unwrap();

        let overrides = s.conflict_overrides(p).unwrap();
        assert!(!overrides.contains_key("natives/stm/a.pak"));
        assert_eq!(
            overrides.get("natives/stm/b.pak"),
            Some(&"mod-b".to_string())
        );
    }

    #[test]
    fn nexus_linked_mods_returns_only_the_mods_carrying_both_ids() {
        let s = seeded();
        add_mod(&s, "mod-local", None);
        add_mod(&s, "mod-nexus", Some((1234, 5678)));
        add_mod(&s, "mod-half", None);
        // A mod known on Nexus but never downloaded through Apocrypha has no file
        // id, so an update check cannot compare it and must skip it.
        s.conn
            .execute("UPDATE mods SET nexus_mod_id=99 WHERE id='mod-half'", [])
            .unwrap();

        assert_eq!(
            s.nexus_linked_mods("monster-hunter-wilds").unwrap(),
            vec![("mod-nexus".to_string(), 1234, 5678)]
        );
        let got = s.get_mod("mod-nexus").unwrap().unwrap();
        assert_eq!(got.nexus_mod_id, Some(1234));
        assert_eq!(got.nexus_file_id, Some(5678));
        assert_eq!(s.get_mod("mod-local").unwrap().unwrap().nexus_mod_id, None);
    }

    #[test]
    fn upgrading_a_v2_database_keeps_its_rows_and_adds_the_v3_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("apocrypha.sqlite");

        {
            let s = Store::open(&db).unwrap();
            s.upsert_game(&GameRecord {
                id: "monster-hunter-wilds".into(),
                name: "Monster Hunter Wilds".into(),
                install_dir: Some("/games/mhw".into()),
                proton_prefix: None,
                active_profile_id: None,
            })
            .unwrap();
            add_mod(&s, "mod-a", None);
            let p = s.ensure_profile("monster-hunter-wilds", "Vanilla").unwrap();
            s.set_mod_state(
                p,
                &ModState {
                    mod_id: "mod-a".into(),
                    enabled: true,
                    priority: 7,
                    selection: Selection::new(),
                    group_id: None,
                },
            )
            .unwrap();

            // Reshape the file into exactly what the previous release wrote.
            s.conn
                .execute_batch(
                    "DROP TABLE conflict_overrides;
                     ALTER TABLE mods DROP COLUMN nexus_mod_id;
                     ALTER TABLE mods DROP COLUMN nexus_file_id;
                     PRAGMA user_version = 2;",
                )
                .unwrap();
        }

        let s = Store::open(&db).unwrap();
        let version: i64 = s
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let kept = s.get_mod("mod-a").unwrap().unwrap();
        assert_eq!(kept.archive_path, "/downloads/mod-a.zip");
        assert_eq!(kept.nexus_mod_id, None);
        let profiles = s.list_profiles("monster-hunter-wilds").unwrap();
        assert_eq!(profiles.len(), 1);
        let state = s.get_mod_state(profiles[0].id, "mod-a").unwrap().unwrap();
        assert_eq!(state.priority, 7);

        // The v3 additions are usable on the upgraded file, not just on a fresh one.
        s.set_conflict_override(profiles[0].id, "natives/stm/a.pak", "mod-a")
            .unwrap();
        assert_eq!(s.conflict_overrides(profiles[0].id).unwrap().len(), 1);
        add_mod(&s, "mod-b", Some((10, 20)));
        assert_eq!(
            s.nexus_linked_mods("monster-hunter-wilds").unwrap(),
            vec![("mod-b".to_string(), 10, 20)]
        );
    }

    #[test]
    fn upgrading_an_older_database_gains_the_profile_cache_without_losing_anything() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("apocrypha.db");

        {
            let s = Store::open(&db).unwrap();
            s.upsert_game(&GameRecord {
                id: "monster-hunter-wilds".into(),
                name: "Monster Hunter Wilds".into(),
                install_dir: Some("/games/mhw".into()),
                proton_prefix: None,
                active_profile_id: None,
            })
            .unwrap();
            add_mod(&s, "mod-a", None);

            // Reshape the file into what the previous release wrote.
            s.conn
                .execute_batch(
                    "DROP TABLE game_profile_cache;
                     PRAGMA user_version = 5;",
                )
                .unwrap();
        }

        let s = Store::open(&db).unwrap();
        assert!(
            s.get_mod("mod-a").unwrap().is_some(),
            "the library survives"
        );
        assert!(
            s.cached_profiles(1).unwrap().is_empty(),
            "an upgraded file starts with nothing cached rather than failing to read"
        );
        assert_eq!(s.profiles_fetched_at().unwrap(), None);
    }

    #[test]
    fn cached_profiles_are_replaced_wholesale_and_read_back_by_contract() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("apocrypha.db")).unwrap();

        s.put_cached_profiles(&[
            ("game-a".into(), "{\"id\":\"game-a\"}".into(), 1),
            ("game-b".into(), "{\"id\":\"game-b\"}".into(), 1),
        ])
        .unwrap();
        assert_eq!(s.cached_profiles(1).unwrap().len(), 2);
        assert!(s.profiles_fetched_at().unwrap().is_some());

        // A profile the service has stopped publishing must stop being used,
        // so a write replaces the set rather than merging into it.
        s.put_cached_profiles(&[("game-a".into(), "{\"id\":\"game-a\"}".into(), 1)])
            .unwrap();
        let kept = s.cached_profiles(1).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, "game-a");

        // A document written for a contract this build does not read is left
        // alone rather than handed over to be misread.
        s.put_cached_profiles(&[("game-c".into(), "{}".into(), 99)])
            .unwrap();
        assert!(s.cached_profiles(1).unwrap().is_empty());
        assert_eq!(s.cached_profiles(99).unwrap().len(), 1);
    }

    #[test]
    fn upgrading_from_v3_points_every_mod_at_the_directory_it_already_uses() {
        // The whole safety argument for splitting staging from identity rests on
        // this: an existing library's files are at `staging/<id>/`, so the
        // backfill has to name exactly that and move nothing.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("apocrypha.db");

        {
            let s = Store::open(&db).unwrap();
            s.upsert_game(&GameRecord {
                id: "monster-hunter-wilds".into(),
                name: "Monster Hunter Wilds".into(),
                install_dir: None,
                proton_prefix: None,
                active_profile_id: None,
            })
            .unwrap();
            add_mod(&s, "mod-a", None);
            add_mod(&s, "mod-b", Some((10, 20)));

            // Reshape into what the previous release wrote: no staging_key.
            s.conn
                .execute_batch(
                    "ALTER TABLE mods DROP COLUMN staging_key;
                     PRAGMA user_version = 3;",
                )
                .unwrap();
        }

        let s = Store::open(&db).unwrap();
        let version: i64 = s
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        for id in ["mod-a", "mod-b"] {
            let m = s.get_mod(id).unwrap().unwrap();
            assert_eq!(
                m.staging_key, id,
                "{id} must keep pointing at its own files"
            );
        }

        let mut keys = s.staging_keys("monster-hunter-wilds").unwrap();
        keys.sort();
        assert_eq!(keys, vec!["mod-a".to_string(), "mod-b".to_string()]);
    }

    #[test]
    fn replacing_a_mod_in_place_keeps_its_profile_state_and_overrides() {
        // What makes an update a replacement rather than a second row: the id is
        // the identity, so an upsert can swap the bundle and the staging
        // generation underneath it without disturbing anything keyed to it.
        let s = seeded();
        add_mod(&s, "mod-a", None);
        let p = s.ensure_profile("monster-hunter-wilds", "Vanilla").unwrap();
        s.set_mod_state(
            p,
            &ModState {
                mod_id: "mod-a".into(),
                enabled: false,
                priority: 7,
                selection: Selection::new(),
                group_id: None,
            },
        )
        .unwrap();
        s.set_conflict_override(p, "natives/stm/a.pak", "mod-a")
            .unwrap();

        let mut updated = s.get_mod("mod-a").unwrap().unwrap();
        updated.version = Some("2.0".into());
        updated.staging_key = "mod-a__v2".into();
        s.insert_mod(&updated).unwrap();

        let after = s.get_mod("mod-a").unwrap().unwrap();
        assert_eq!(after.version.as_deref(), Some("2.0"));
        assert_eq!(after.staging_key, "mod-a__v2", "a new generation of files");

        let state = s.get_mod_state(p, "mod-a").unwrap().unwrap();
        assert!(
            !state.enabled,
            "a disabled mod stays disabled across a swap"
        );
        assert_eq!(state.priority, 7, "load order survives");
        assert_eq!(
            s.conflict_overrides(p).unwrap().get("natives/stm/a.pak"),
            Some(&"mod-a".to_string()),
            "an override naming this mod still names it"
        );

        assert_eq!(
            s.staging_keys("monster-hunter-wilds").unwrap(),
            vec!["mod-a__v2".to_string()],
            "the old generation is no longer claimed, so pruning may reclaim it"
        );
    }

    #[test]
    fn provenance_round_trips_and_outlives_the_mod_it_produced() {
        // Keyed on the archive, not the mod, and deliberately without a foreign
        // key: a file stays in the downloads folder after its mod is removed,
        // and forgetting where it came from at that moment is exactly wrong.
        let s = seeded();
        add_mod(&s, "mod-a", None);
        s.record_archive_provenance(&Provenance {
            archive_path: "/downloads/armour-v2.zip".into(),
            domain: Some("monsterhunterwilds".into()),
            nexus_mod_id: Some(1234),
            nexus_file_id: Some(99),
            replaces_mod_id: Some("mod-a".into()),
        })
        .unwrap();

        let got = s
            .archive_provenance("/downloads/armour-v2.zip")
            .unwrap()
            .unwrap();
        assert_eq!(got.nexus_mod_id, Some(1234));
        assert_eq!(got.replaces_mod_id.as_deref(), Some("mod-a"));

        s.delete_mod("mod-a").unwrap();
        let after = s
            .archive_provenance("/downloads/armour-v2.zip")
            .unwrap()
            .unwrap();
        assert_eq!(
            after.replaces_mod_id.as_deref(),
            Some("mod-a"),
            "a dangling hint is checked at use, not cascaded away"
        );
    }

    #[test]
    fn re_downloading_an_archive_replaces_what_was_known_about_it() {
        // The same file fetched again as a plain download must stop claiming to
        // update anything, or it would silently replace a mod nobody named.
        let s = seeded();
        s.record_archive_provenance(&Provenance {
            archive_path: "/downloads/armour.zip".into(),
            domain: Some("monsterhunterwilds".into()),
            nexus_mod_id: Some(1234),
            nexus_file_id: Some(99),
            replaces_mod_id: Some("mod-a".into()),
        })
        .unwrap();
        s.record_archive_provenance(&Provenance {
            archive_path: "/downloads/armour.zip".into(),
            domain: Some("monsterhunterwilds".into()),
            nexus_mod_id: Some(1234),
            nexus_file_id: Some(100),
            replaces_mod_id: None,
        })
        .unwrap();

        let got = s
            .archive_provenance("/downloads/armour.zip")
            .unwrap()
            .unwrap();
        assert_eq!(got.nexus_file_id, Some(100));
        assert_eq!(got.replaces_mod_id, None);
        assert_eq!(
            s.all_archive_provenance().unwrap().len(),
            1,
            "one row per path"
        );
    }

    #[test]
    fn an_archive_nothing_was_recorded_for_simply_has_no_provenance() {
        let s = seeded();
        assert!(s
            .archive_provenance("/downloads/saved-by-hand.zip")
            .unwrap()
            .is_none());
        assert!(s.all_archive_provenance().unwrap().is_empty());
    }

    // ---- groups ---------------------------------------------------------

    /// Five mods in a profile, ordered `a b c x y`, with `a b c` in one group.
    fn grouped() -> (Store, i64, i64) {
        let s = seeded();
        let profile_id = s.ensure_profile("monster-hunter-wilds", "Default").unwrap();
        for id in ["a", "b", "c", "x", "y"] {
            add_mod(&s, id, None);
            s.set_mod_state(
                profile_id,
                &ModState {
                    mod_id: id.into(),
                    enabled: true,
                    priority: 0,
                    selection: Selection::default(),
                    group_id: None,
                },
            )
            .unwrap();
        }
        s.set_mod_order(profile_id, &ids(&["a", "b", "c", "x", "y"]))
            .unwrap();
        let group_id = s.create_group(profile_id, "Frameworks", "default").unwrap();
        s.assign_to_group(profile_id, Some(group_id), &ids(&["a", "b", "c"]))
            .unwrap();
        (s, profile_id, group_id)
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn order(s: &Store, profile_id: i64) -> Vec<String> {
        s.list_mod_states(profile_id)
            .unwrap()
            .into_iter()
            .map(|m| m.mod_id)
            .collect()
    }

    #[test]
    fn a_reorder_that_lifts_one_mod_out_of_a_locked_group_is_refused_and_writes_nothing() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();

        let err = s
            .set_mod_order(profile_id, &ids(&["a", "c", "x", "b", "y"]))
            .unwrap_err();
        assert!(matches!(err, StorageError::LockedGroup(_)), "{err}");
        assert_eq!(
            order(&s, profile_id),
            ids(&["a", "b", "c", "x", "y"]),
            "a refused order must leave every priority where it was"
        );
    }

    #[test]
    fn a_reorder_that_drops_a_stranger_between_two_members_of_a_locked_group_is_refused() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        assert!(s
            .set_mod_order(profile_id, &ids(&["a", "b", "x", "c", "y"]))
            .is_err());
    }

    #[test]
    fn a_locked_block_can_still_be_carried_whole() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        s.set_mod_order(profile_id, &ids(&["x", "y", "a", "b", "c"]))
            .unwrap();
        assert_eq!(order(&s, profile_id), ids(&["x", "y", "a", "b", "c"]));
    }

    #[test]
    fn unlocking_a_group_lets_the_very_reorder_that_was_just_refused_through() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        assert!(s
            .set_mod_order(profile_id, &ids(&["a", "c", "x", "b", "y"]))
            .is_err());

        s.set_group_locked(profile_id, group_id, false).unwrap();
        s.set_mod_order(profile_id, &ids(&["a", "c", "x", "b", "y"]))
            .unwrap();
        assert_eq!(order(&s, profile_id), ids(&["a", "c", "x", "b", "y"]));
    }

    #[test]
    fn an_order_naming_a_mod_that_is_not_in_the_profile_is_still_the_refusal_it_always_was() {
        let (s, profile_id, _) = grouped();
        let err = s
            .set_mod_order(profile_id, &ids(&["a", "b", "c", "x", "y", "ghost"]))
            .unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)), "{err}");
    }

    #[test]
    fn assigning_a_mod_to_a_group_gathers_it_beside_the_members_instead_of_leaving_it_behind() {
        let (s, profile_id, group_id) = grouped();
        s.assign_to_group(profile_id, Some(group_id), &ids(&["y"]))
            .unwrap();
        assert_eq!(order(&s, profile_id), ids(&["a", "b", "c", "y", "x"]));
    }

    #[test]
    fn assigning_a_mod_to_a_locked_group_is_refused() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        assert!(s
            .assign_to_group(profile_id, Some(group_id), &ids(&["y"]))
            .is_err());
    }

    #[test]
    fn deleting_a_group_keeps_its_mods_and_every_priority_exactly_where_they_were() {
        let (s, profile_id, group_id) = grouped();
        let before = order(&s, profile_id);
        s.delete_group(profile_id, group_id).unwrap();
        assert_eq!(order(&s, profile_id), before);
        assert!(s.membership(profile_id).unwrap().is_empty());
        assert_eq!(s.list_mod_states(profile_id).unwrap().len(), 5);
    }

    #[test]
    fn a_locked_group_cannot_be_deleted_until_it_is_unlocked() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        assert!(s.delete_group(profile_id, group_id).is_err());
        s.set_group_locked(profile_id, group_id, false).unwrap();
        assert!(s.delete_group(profile_id, group_id).is_ok());
    }

    #[test]
    fn cloning_a_profile_points_the_copy_at_its_own_groups_rather_than_the_originals() {
        let (s, profile_id, group_id) = grouped();
        let copy_id = s.clone_profile(profile_id, "Experiment").unwrap();

        let copies = s.list_groups(copy_id).unwrap();
        assert_eq!(copies.len(), 1);
        let copied_group = copies[0].group.id;
        assert_ne!(
            copied_group, group_id,
            "the copy must own its group, not share one"
        );

        let membership = s.membership(copy_id).unwrap();
        assert_eq!(membership.get("a"), Some(&copied_group));
        assert_eq!(order(&s, copy_id), ids(&["a", "b", "c", "x", "y"]));

        // Regrouping the copy must not reach back into what it came from.
        s.assign_to_group(copy_id, None, &ids(&["a"])).unwrap();
        assert_eq!(s.membership(profile_id).unwrap().get("a"), Some(&group_id));
    }

    #[test]
    fn a_cloned_profile_keeps_its_locks_so_the_copy_is_as_trustworthy_as_its_source() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        let copy_id = s.clone_profile(profile_id, "Experiment").unwrap();
        assert!(s.list_groups(copy_id).unwrap()[0].group.locked);
        assert!(s
            .set_mod_order(copy_id, &ids(&["a", "c", "x", "b", "y"]))
            .is_err());
    }

    #[test]
    fn removing_a_mod_from_the_library_shrinks_the_group_it_was_in_and_leaves_the_rest_locked() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        s.delete_mod("b").unwrap();

        assert_eq!(order(&s, profile_id), ids(&["a", "c", "x", "y"]));
        assert!(
            s.set_mod_order(profile_id, &ids(&["a", "x", "c", "y"]))
                .is_err(),
            "the survivors are still a locked group"
        );
    }

    #[test]
    fn a_group_whose_last_member_left_the_library_survives_as_an_empty_group() {
        let (s, profile_id, group_id) = grouped();
        for id in ["a", "b", "c"] {
            s.delete_mod(id).unwrap();
        }
        let groups = s.list_groups(profile_id).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group.id, group_id);
        assert!(s.membership(profile_id).unwrap().is_empty());
    }

    #[test]
    fn a_locked_group_still_takes_a_bulk_enable_because_a_lock_holds_the_order_not_the_switches() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        s.set_enabled_bulk(profile_id, &ids(&["a", "b"]), false)
            .unwrap();
        let states = s.list_mod_states(profile_id).unwrap();
        assert!(!states.iter().find(|m| m.mod_id == "a").unwrap().enabled);
        assert_eq!(order(&s, profile_id), ids(&["a", "b", "c", "x", "y"]));
    }

    #[test]
    fn renaming_a_locked_group_is_allowed_because_a_lock_holds_the_order_and_not_the_label() {
        let (s, profile_id, group_id) = grouped();
        s.set_group_locked(profile_id, group_id, true).unwrap();
        s.update_group(group_id, Some("Core"), None, Some(true))
            .unwrap();
        let g = &s.list_groups(profile_id).unwrap()[0].group;
        assert_eq!(g.name, "Core");
        assert!(g.collapsed);
    }

    #[test]
    fn a_drop_below_a_row_lands_immediately_after_it_whatever_the_screen_was_showing() {
        let (s, profile_id, _) = grouped();
        let out = s
            .move_in_order(
                profile_id,
                &OrderMove {
                    subject: MoveSubject::Mod("y".into()),
                    // "x" is what the person saw above the drop point. Whatever
                    // was filtered out between them is not their problem.
                    placement: Placement::After("x".into()),
                    belonging: Belonging::Keep,
                },
            )
            .unwrap();
        assert_eq!(out, ids(&["a", "b", "c", "x", "y"]));
    }

    #[test]
    fn a_new_mod_lands_after_everything_rather_than_wherever_the_alphabet_puts_it() {
        let (s, profile_id, _) = grouped();
        assert_eq!(s.next_priority(profile_id).unwrap(), 5);
    }

    #[test]
    fn upgrading_a_database_written_before_groups_keeps_every_priority_and_groups_nothing() {
        let s = seeded();
        let profile_id = s.ensure_profile("monster-hunter-wilds", "Default").unwrap();
        for id in ["a", "b"] {
            add_mod(&s, id, None);
            s.set_mod_state(
                profile_id,
                &ModState {
                    mod_id: id.into(),
                    enabled: true,
                    priority: 0,
                    selection: Selection::default(),
                    group_id: None,
                },
            )
            .unwrap();
        }
        s.set_mod_order(profile_id, &ids(&["b", "a"])).unwrap();

        // What a v6 database looks like on the next launch: the migration runs
        // again from a lower version against rows that already exist.
        s.conn.pragma_update(None, "user_version", 6i64).unwrap();
        let s = Store::init(s.conn).unwrap();

        assert_eq!(order(&s, profile_id), ids(&["b", "a"]));
        assert!(s.membership(profile_id).unwrap().is_empty());
        assert!(s.list_groups(profile_id).unwrap().is_empty());
    }
}
