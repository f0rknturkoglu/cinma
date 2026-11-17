//! Cinma core runtime: owns the database, the async runtime and the feature
//! modules, and implements the fixed flow (addon → streams → play).

#![forbid(unsafe_code)]

mod addons;
mod playback;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cinma_db::{Database, DbError};
use cinma_stremio::AddonClient;
use cinma_torrent::{RqbitEngine, TorrentConfig};

pub use addons::{InstalledAddon, StreamCandidate};
pub use cinma_player::{EndReason, PlayerEvent, PlayerEvents, UpdateCallback};
pub use playback::Playback;

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
    #[error("addon: {0}")]
    Addon(#[from] cinma_stremio::AddonError),
    #[error("torrent: {0}")]
    Torrent(#[from] cinma_torrent::TorrentError),
    #[error("player: {0}")]
    Player(#[from] cinma_player::PlayerError),
    #[error("{0}")]
    Invalid(String),
    #[error("task failed: {0}")]
    Task(String),
}

pub struct Core {
    data_dir: PathBuf,
    db: Mutex<Database>,
    addons: Mutex<Vec<InstalledAddon>>,
    runtime: OnceLock<tokio::runtime::Runtime>,
    http: OnceLock<AddonClient>,
    torrents: OnceLock<Arc<RqbitEngine>>,
}

impl Core {
    /// Opens the core with all persistent state under `data_dir`. Cheap: no
    /// runtime, network or torrent session is started here.
    pub fn open(data_dir: &Path) -> Result<Arc<Self>, CoreError> {
        std::fs::create_dir_all(data_dir)?;
        let db = Database::open(&data_dir.join(DB_FILE_NAME))?;
        let addons = addons::load(&db)?;
        Ok(Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            db: Mutex::new(db),
            addons: Mutex::new(addons),
            runtime: OnceLock::new(),
            http: OnceLock::new(),
            torrents: OnceLock::new(),
        }))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn schema_version(&self) -> Result<u32, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.schema_version()?)
    }

    /// Small bounded worker pool, created on first async use.
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, CoreError> {
        if let Some(rt) = self.runtime.get() {
            return Ok(rt);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(8)
            .thread_name("cinma-core")
            .enable_all()
            .build()?;
        Ok(self.runtime.get_or_init(|| rt))
    }

    /// Runs `future` on the core runtime; awaitable from any executor
    /// (UniFFI polls it from Swift).
    pub async fn spawn<T, F>(&self, future: F) -> Result<T, CoreError>
    where
        F: Future<Output = Result<T, CoreError>> + Send + 'static,
        T: Send + 'static,
    {
        self.runtime()?
            .spawn(future)
            .await
            .map_err(|e| CoreError::Task(e.to_string()))?
    }

    fn http(&self) -> Result<AddonClient, CoreError> {
        if let Some(client) = self.http.get() {
            return Ok(client.clone());
        }
        let client = AddonClient::new()?;
        Ok(self.http.get_or_init(|| client).clone())
    }

    fn torrents(&self) -> Result<Arc<RqbitEngine>, CoreError> {
        if let Some(engine) = self.torrents.get() {
            return Ok(Arc::clone(engine));
        }
        let extra_peers = std::env::var("CINMA_DEV_PEERS")
            .ok()
            .map(|v| v.split(',').filter_map(|p| p.trim().parse().ok()).collect())
            .unwrap_or_default();
        let engine = Arc::new(RqbitEngine::new(
            TorrentConfig {
                root: self.data_dir.join("torrents"),
                extra_peers,
                local_only: false,
            },
            self.runtime()?.handle().clone(),
        ));
        Ok(Arc::clone(self.torrents.get_or_init(|| engine)))
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
        assert!(core.runtime.get().is_none(), "runtime must be lazy");
        assert!(core.addons().unwrap().is_empty());
    }
}
