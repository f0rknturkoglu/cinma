//! Cinma core runtime: owns the database, the async runtime and the feature
//! modules, and implements the fixed flow (addon → streams → play).

#![forbid(unsafe_code)]

mod addons;
mod events;
mod playback;
mod settings;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cinma_db::{Database, DbError, ProfileId};
use cinma_stremio::AddonClient;
use cinma_torrent::{RqbitEngine, TorrentConfig};

pub use addons::{InstalledAddon, StreamCandidate};
pub use events::{CoreEvent, CoreEventListener};
pub use settings::Feature;
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
    #[error("feature `{0}` is turned off")]
    FeatureDisabled(&'static str),
    #[error("task failed: {0}")]
    Task(String),
}

const ACTIVE_PROFILE_KEY: &str = "profile.active";

pub struct Core {
    data_dir: PathBuf,
    db: Mutex<Database>,
    runtime: OnceLock<tokio::runtime::Runtime>,
    http: Mutex<Option<AddonClient>>,
    torrents: Mutex<Option<Arc<RqbitEngine>>>,
    listeners: Mutex<Vec<Arc<dyn CoreEventListener>>>,
}

impl Core {
    /// Opens the core with all persistent state under `data_dir`. Cheap: no
    /// runtime, network or torrent session is started here.
    pub fn open(data_dir: &Path) -> Result<Arc<Self>, CoreError> {
        std::fs::create_dir_all(data_dir)?;
        let db = Database::open(&data_dir.join(DB_FILE_NAME))?;
        Ok(Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            db: Mutex::new(db),
            runtime: OnceLock::new(),
            http: Mutex::new(None),
            torrents: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
        }))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn schema_version(&self) -> Result<u32, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.schema_version()?)
    }

    /// The profile whose data the UI shows (profile picker: Phase 3).
    pub fn active_profile(&self) -> Result<ProfileId, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let stored = db.get_app_setting(ACTIVE_PROFILE_KEY)?.and_then(|v| v.parse().ok());
        match stored {
            Some(id) if db.profile_exists(id)? => Ok(id),
            _ => Ok(1),
        }
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
        self.require(Feature::Addons)?;
        let mut slot = self.http.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(client) = slot.as_ref() {
            return Ok(client.clone());
        }
        let client = AddonClient::new()?;
        *slot = Some(client.clone());
        Ok(client)
    }

    fn torrents(&self) -> Result<Arc<RqbitEngine>, CoreError> {
        self.require(Feature::Torrent)?;
        let mut slot = self.torrents.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(engine) = slot.as_ref() {
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
        *slot = Some(Arc::clone(&engine));
        Ok(engine)
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
        assert_eq!(core.active_profile().unwrap(), 1);
    }

    #[test]
    fn disabled_features_are_never_constructed() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        for feature in Feature::ALL {
            assert!(core.feature_enabled(feature).unwrap(), "{feature:?} defaults on");
        }
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(core.set_feature_enabled(Feature::Torrent, false)).unwrap();
        rt.block_on(core.set_feature_enabled(Feature::Addons, false)).unwrap();
        assert!(matches!(core.torrents(), Err(CoreError::FeatureDisabled("torrent"))));
        assert!(matches!(core.http(), Err(CoreError::FeatureDisabled("addons"))));
        let streams = rt.block_on(core.streams("movie".into(), "tt1".into()));
        assert!(matches!(streams, Err(CoreError::FeatureDisabled("addons"))));
        assert!(core.runtime.get().is_none(), "no runtime for disabled features");
        assert!(core.torrents.lock().unwrap().is_none());
        // Persisted across reopen.
        drop(core);
        let core = Core::open(dir.path()).unwrap();
        assert!(!core.feature_enabled(Feature::Torrent).unwrap());
    }

    #[test]
    fn turning_torrent_off_drops_a_running_engine() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        let engine = core.torrents().unwrap();
        assert!(core.torrents.lock().unwrap().is_some());
        drop(engine);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(core.set_feature_enabled(Feature::Torrent, false)).unwrap();
        assert!(core.torrents.lock().unwrap().is_none());
    }

    #[test]
    fn events_reach_subscribers() {
        struct Recorder(Mutex<Vec<CoreEvent>>);
        impl CoreEventListener for Recorder {
            fn on_event(&self, event: CoreEvent) {
                self.0.lock().unwrap().push(event);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        core.subscribe(recorder.clone()).unwrap();
        core.remove_addon("missing").unwrap();
        assert_eq!(*recorder.0.lock().unwrap(), vec![CoreEvent::AddonsChanged]);
    }
}
