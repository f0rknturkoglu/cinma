//! SQLite storage: connection setup and versioned migrations.
//!
//! The schema version is kept in `PRAGMA user_version`. Each migration runs in
//! its own transaction together with the version bump, so a crash mid-migration
//! leaves the database at the previous version.

#![forbid(unsafe_code)]

use std::path::Path;

use rusqlite::Connection;

/// Ordered migrations. Index `i` upgrades the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

/// Schema version this build understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema v{found} is newer than this build (v{supported})")]
    FutureSchema { found: u32, supported: u32 },
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

    fn init(mut conn: Connection) -> Result<Self, DbError> {
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
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(DbError::FutureSchema {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
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
    fn tables_are_strict() {
        let db = Database::open_in_memory().unwrap();
        let result = db.connection().execute(
            "INSERT INTO profile (id, name) VALUES ('not-an-int', 'x')",
            [],
        );
        assert!(result.is_err());
    }
}
