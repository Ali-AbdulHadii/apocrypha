//! Persistent state for Apocrypha: SQLite-backed games, installed mods,
//! profiles, and per-profile option selections.
//!
//! SQLite is embedded (bundled, no system dependency), runs in WAL mode with
//! foreign keys enforced, and is migrated forward by `user_version`.

pub mod paths;

use apoc_domain::{ModBundle, Selection};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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
}

const SCHEMA_VERSION: i64 = 2;

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
                imported_at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE INDEX IF NOT EXISTS idx_mods_game ON mods(game_id);

            CREATE TABLE IF NOT EXISTS profiles (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                name    TEXT NOT NULL,
                UNIQUE(game_id, name)
            );

            CREATE TABLE IF NOT EXISTS profile_mod_state (
                profile_id     INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                mod_id         TEXT NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
                enabled        INTEGER NOT NULL DEFAULT 0,
                priority       INTEGER NOT NULL DEFAULT 0,
                selection_json TEXT NOT NULL DEFAULT '{"chosen":[]}',
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

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
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
            .query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| {
                r.get(0)
            })
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

    pub fn set_active_profile(&self, game_id: &str, profile_id: i64) -> Result<()> {
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
            "INSERT INTO mods(id,game_id,name,version,author,archive_path,archive_sha256,installer_model,bundle_json)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, version=excluded.version, author=excluded.author,
                archive_path=excluded.archive_path, archive_sha256=excluded.archive_sha256,
                installer_model=excluded.installer_model, bundle_json=excluded.bundle_json",
            params![
                rec.id, rec.game_id, rec.name, rec.version, rec.author,
                rec.archive_path, rec.archive_sha256, rec.installer_model, bundle_json
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
            bundle,
        })
    }

    pub fn get_mod(&self, id: &str) -> Result<Option<ModRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,game_id,name,version,author,archive_path,archive_sha256,installer_model,bundle_json,imported_at
                 FROM mods WHERE id=?1",
                params![id],
                Self::row_to_mod,
            )
            .optional()?)
    }

    pub fn list_mods(&self, game_id: &str) -> Result<Vec<ModRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,game_id,name,version,author,archive_path,archive_sha256,installer_model,bundle_json,imported_at
             FROM mods WHERE game_id=?1 ORDER BY imported_at, name",
        )?;
        let rows = stmt.query_map(params![game_id], Self::row_to_mod)?;
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

    pub fn delete_mod(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM mods WHERE id=?1", params![id])?;
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
        self.conn.execute(
            "INSERT INTO profile_mod_state(profile_id,mod_id,enabled,priority,selection_json)
             SELECT ?1, mod_id, enabled, priority, selection_json
             FROM profile_mod_state WHERE profile_id=?2",
            params![new_id, source_id],
        )?;
        Ok(new_id)
    }

    // ---- per-profile mod state -----------------------------------------

    pub fn set_mod_state(&self, profile_id: i64, state: &ModState) -> Result<()> {
        let selection_json = serde_json::to_string(&state.selection)?;
        self.conn.execute(
            "INSERT INTO profile_mod_state(profile_id,mod_id,enabled,priority,selection_json)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(profile_id,mod_id) DO UPDATE SET
                enabled=excluded.enabled, priority=excluded.priority,
                selection_json=excluded.selection_json",
            params![
                profile_id,
                state.mod_id,
                state.enabled as i64,
                state.priority,
                selection_json
            ],
        )?;
        Ok(())
    }

    pub fn get_mod_state(&self, profile_id: i64, mod_id: &str) -> Result<Option<ModState>> {
        let row = self
            .conn
            .query_row(
                "SELECT mod_id,enabled,priority,selection_json
                 FROM profile_mod_state WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, mod_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((mod_id, enabled, priority, selection_json)) = row else {
            return Ok(None);
        };
        Ok(Some(ModState {
            mod_id,
            enabled: enabled != 0,
            priority,
            selection: serde_json::from_str(&selection_json)?,
        }))
    }

    /// All mod states in a profile, ordered by load-order priority.
    pub fn list_mod_states(&self, profile_id: i64) -> Result<Vec<ModState>> {
        let mut stmt = self.conn.prepare(
            "SELECT mod_id,enabled,priority,selection_json
             FROM profile_mod_state WHERE profile_id=?1 ORDER BY priority, mod_id",
        )?;
        let rows = stmt.query_map(params![profile_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mod_id, enabled, priority, selection_json) = row?;
            out.push(ModState {
                mod_id,
                enabled: enabled != 0,
                priority,
                selection: serde_json::from_str(&selection_json)?,
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

    /// Rewrite load-order priority for a profile from an ordered id list.
    pub fn set_mod_order(&self, profile_id: i64, ordered_ids: &[String]) -> Result<()> {
        for (index, id) in ordered_ids.iter().enumerate() {
            self.conn.execute(
                "UPDATE profile_mod_state SET priority=?3 WHERE profile_id=?1 AND mod_id=?2",
                params![profile_id, id, index as i64],
            )?;
        }
        Ok(())
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
    use apoc_domain::InstallerModel;

    fn empty_bundle(name: &str) -> ModBundle {
        ModBundle {
            name: name.into(),
            version: Some("v1".into()),
            author: None,
            category: None,
            installer_model: InstallerModel::FluffyAio,
            archive_sha256: None,
            groups: vec![],
        }
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
        assert_eq!(s.applied_mod_ids("monster-hunter-wilds").unwrap(), vec!["mod-c"]);
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
}
