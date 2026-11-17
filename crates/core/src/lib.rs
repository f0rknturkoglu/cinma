//! Cinma core runtime: owns the database and (later) feature modules.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cinma_db::{Database, DbError};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const DB_FILE_NAME: &str = "cinma.sqlite3";

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database: {0}")]
    Database(#[from] DbError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal state poisoned")]
    Poisoned,
}

pub struct Core {
    data_dir: PathBuf,
    db: Mutex<Database>,
}

impl Core {
    /// Opens the core with all persistent state under `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self, CoreError> {
        std::fs::create_dir_all(data_dir)?;
        let db = Database::open(&data_dir.join(DB_FILE_NAME))?;
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            db: Mutex::new(db),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn schema_version(&self) -> Result<u32, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.schema_version()?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_data_dir_and_database() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("nested").join("Cinma");
        let core = Core::open(&data_dir).unwrap();
        assert!(data_dir.join(DB_FILE_NAME).exists());
        assert_eq!(core.schema_version().unwrap(), cinma_db::SCHEMA_VERSION);
    }
}
