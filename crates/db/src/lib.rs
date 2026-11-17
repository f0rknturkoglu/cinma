//! SQLite storage: connection setup and versioned migrations.
//!
//! The schema version is kept in `PRAGMA user_version`. Each migration runs in
//! its own transaction together with the version bump, so a crash mid-migration
//! leaves the database at the previous version.

#![forbid(unsafe_code)]

mod repo;
mod repo_library;
mod repo_meta;

use std::path::Path;

use rusqlite::Connection;

/// Ordered migrations. Index `i` upgrades the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_core_schema.sql"),
    include_str!("../migrations/0003_metadata.sql"),
    include_str!("../migrations/0004_library_and_progress.sql"),
];

/// Schema version this build understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub use repo::{
    AddonRow, DownloadKind, DownloadRow, DownloadState, NewDownload, ProfileId, ProfileRow,
};
pub use repo_library::{LibraryItemRow, WatchProgressRow};
pub use repo_meta::{IdMapEntry, MediaKind};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema v{found} is newer than this build (v{supported})")]
    FutureSchema { found: u32, supported: u32 },
    #[error("invalid stored value: {0}")]
    Invalid(String),
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (or creates) the database file and brings the schema up to date.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    /// Opens without migrating (tests: build an old-version database).
    #[cfg(test)]
    fn raw(conn: Connection) -> Self {
        Self { conn }
    }

    fn init(mut conn: Connection) -> Result<Self, DbError> {
        // Wait for other connections (e.g. a second process) instead of
        // failing immediately with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32, DbError> {
        user_version(&self.conn)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn user_version(conn: &Connection) -> Result<u32, DbError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    migrate_to(conn, SCHEMA_VERSION)
}

fn migrate_to(conn: &mut Connection, target: u32) -> Result<(), DbError> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(DbError::FutureSchema {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().take(target as usize).skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_is_at_latest_version() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn default_profile_exists() {
        let db = Database::open_in_memory().unwrap();
        let name: String = db
            .connection()
            .query_row("SELECT name FROM profile WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Default");
    }

    #[test]
    fn reopening_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite3");
        drop(Database::open(&path).unwrap());
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let profiles: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM profile", [], |r| r.get(0))
            .unwrap();
        assert_eq!(profiles, 1);
    }

    #[test]
    fn uses_wal_journal() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("wal.sqlite3")).unwrap();
        let mode: String = db
            .connection()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn rejects_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        assert!(matches!(
            Database::open(&path),
            Err(DbError::FutureSchema { .. })
        ));
    }

    #[test]
    fn app_settings_upsert() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_app_setting("k").unwrap(), None);
        db.set_app_setting("k", "1").unwrap();
        db.set_app_setting("k", "2").unwrap();
        assert_eq!(db.get_app_setting("k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn upgrade_from_v1_moves_phase1_addons_into_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate_to(&mut conn, 1).unwrap();
        let phase1 = r#"[
            {"transport_url":"https://a.example/manifest.json","manifest":{"id":"org.a","name":"A","resources":["stream"],"types":["movie"]}},
            {"transport_url":"https://b.example/x/manifest.json","manifest":{"id":"org.b","name":"B","resources":["stream"],"types":["series"]}}
        ]"#;
        conn.execute(
            "INSERT INTO app_setting (key, value) VALUES ('addons.v1', ?1)",
            [phase1],
        )
        .unwrap();
        migrate_to(&mut conn, SCHEMA_VERSION).unwrap();
        let db = Database::raw(conn);
        let addons = db.addons_for_profile(1).unwrap();
        assert_eq!(addons.len(), 2);
        assert_eq!(addons[0].id, "org.a");
        assert_eq!(addons[0].position, 0);
        assert!(addons[0].enabled);
        assert_eq!(addons[1].transport_url, "https://b.example/x/manifest.json");
        let manifest: serde_json::Value = serde_json::from_str(&addons[1].manifest_json).unwrap();
        assert_eq!(manifest["name"], "B");
        assert_eq!(db.get_app_setting("addons.v1").unwrap(), None);
    }

    #[test]
    fn upgrade_from_v1_without_addons() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to(&mut conn, 1).unwrap();
        migrate_to(&mut conn, SCHEMA_VERSION).unwrap();
        assert!(Database::raw(conn).addons_for_profile(1).unwrap().is_empty());
    }

    #[test]
    fn tables_are_strict() {
        let db = Database::open_in_memory().unwrap();
        let result = db.connection().execute(
            "INSERT INTO profile (id, name) VALUES ('not-an-int', 'x')",
            [],
        );
        assert!(result.is_err());
    }
}
