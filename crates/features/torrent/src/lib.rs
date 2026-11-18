//! Torrent streaming behind the [`TorrentEngine`] trait (librqbit today).
//!
//! The session is created lazily on the first torrent and torn down by
//! [`TorrentEngine::release_all`] so DHT/peer traffic stops when nothing plays.

#![forbid(unsafe_code)]

mod download;

use std::collections::HashSet;
use std::future::Future;
use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cinma_blobstore::{
    ByteSource, CancellationToken, OPENSUBTITLES_CHUNK_SIZE, RangeSet, SourceReader, SourceStats,
    cancelled_error,
};
pub use download::{
    FolderResolver, PARTIAL_DIR, SeedingPolicy, TorrentDownloadEngine, TorrentSourceSpec,
    remove_staging, staging_dir,
};
use librqbit::dht::DhtPersistenceConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, DhtSessionConfig,
    ManagedTorrent, PeerConnectionOptions, Session, SessionOptions, SessionPersistenceConfig,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

const METADATA_TIMEOUT: Duration = Duration::from_secs(90);
const WARMUP_TAIL_WINDOW: u64 = 64 * 1024;
const WARMUP_TIMEOUT: Duration = Duration::from_secs(8);
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv",
];
const FALLBACK_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "http://tracker.opentrackr.org:1337/announce",
];

#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("invalid info hash: {0}")]
    InvalidInfoHash(String),
    #[error("timed out waiting for torrent metadata (no peers?)")]
    MetadataTimeout,
    #[error("file index {index} out of range ({count} files)")]
    FileIndex { index: usize, count: usize },
    #[error("torrent has no files")]
    NoFiles,
    #[error("torrent engine: {0}")]
    Engine(String),
}

// librqbit returns anyhow errors; keep anyhow out of our public API.
impl From<anyhow::Error> for TorrentError {
    fn from(e: anyhow::Error) -> Self {
        Self::Engine(format!("{e:#}"))
    }
}

/// librqbit's `FileStream` isn't nameable from outside the crate.
trait AsyncReadSeek: AsyncRead + AsyncSeek + Send + Unpin {}
impl<T: AsyncRead + AsyncSeek + Send + Unpin> AsyncReadSeek for T {}

#[derive(Debug, Clone)]
pub struct TorrentFileRequest {
    /// 40-char hex BitTorrent v1 info hash.
    pub info_hash: String,
    /// File index inside the torrent; `None` picks the largest video file.
    pub file_idx: Option<usize>,
    pub trackers: Vec<String>,
}

pub trait TorrentEngine: Send + Sync {
    /// Resolves the torrent and returns a readable source for one file.
    /// Must be called from within the engine's tokio runtime.
    fn open_file(
        &self,
        request: TorrentFileRequest,
    ) -> BoxFuture<'_, Result<Arc<dyn ByteSource>, TorrentError>>;

    /// Stops all torrents and the session (DHT, listeners).
    fn release_all(&self) -> BoxFuture<'_, ()>;
}

#[derive(Debug, Clone)]
pub struct TorrentConfig {
    /// Downloads and DHT state live under this directory.
    pub root: PathBuf,
    /// Dev/test only: peers to connect to directly (e.g. a local seeder).
    pub extra_peers: Vec<SocketAddr>,
    /// Tests: no DHT, no local service discovery — only `extra_peers`.
    pub local_only: bool,
}

#[derive(Debug, Clone)]
pub struct TorrentPolicy {
    pub fastresume: bool,
    pub upload_bps: Option<u32>,
    pub download_bps: Option<u32>,
    pub seeding: SeedingPolicy,
    pub delete_on_release: bool,
    /// Optional BSD interface name (e.g. `utun3`) passed to `librqbit` `bind_device_name`.
    pub bind_device_name: Option<String>,
    /// When true, refuses starting a torrent session if `bind_device_name` is unset or offline.
    pub vpn_kill_switch: bool,
}

impl Default for TorrentPolicy {
    fn default() -> Self {
        Self {
            fastresume: false,
            upload_bps: None,
            download_bps: None,
            seeding: SeedingPolicy::Off,
            delete_on_release: true,
            bind_device_name: None,
            vpn_kill_switch: false,
        }
    }
}

pub struct RqbitEngine {
    config: TorrentConfig,
    policy: Mutex<TorrentPolicy>,
    runtime: Handle,
    session: Mutex<Option<Arc<Session>>>,
    /// Torrents in the session and why they are there. Streams are temporary:
    /// they are deleted with their files when playback stops, so they never
    /// linger on disk or come back through session persistence.
    roles: std::sync::Mutex<TorrentRoles>,
    /// Open "Play Now" readers per download; a finished download waits for
    /// them before it leaves the session and its file is moved.
    viewers: Arc<Viewers>,
}

#[derive(Default)]
struct Viewers {
    counts: std::sync::Mutex<std::collections::HashMap<String, usize>>,
    changed: tokio::sync::Notify,
}

impl Viewers {
    fn count(&self, hash: &str) -> usize {
        self.counts
            .lock()
            .map(|c| c.get(hash).copied().unwrap_or(0))
            .unwrap_or(0)
    }
}

/// A download's bytes for "Play Now"; counts as a viewer until dropped.
struct WatchedSource {
    inner: Arc<dyn ByteSource>,
    hash: String,
    viewers: Arc<Viewers>,
}

impl Drop for WatchedSource {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.viewers.counts.lock()
            && let Some(n) = counts.get_mut(&self.hash)
        {
            *n = n.saturating_sub(1);
            if *n == 0 {
                counts.remove(&self.hash);
            }
        }
        self.viewers.changed.notify_waiters();
    }
}

impl ByteSource for WatchedSource {
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        self.inner.open_reader()
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn filename(&self) -> Option<&str> {
        self.inner.filename()
    }
    fn available_ranges(&self) -> Option<RangeSet> {
        self.inner.available_ranges()
    }
    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        self.inner.try_read_at(offset, buf)
    }
}

#[derive(Default)]
struct TorrentRoles {
    streams: HashSet<String>,
    downloads: HashSet<String>,
    /// Streams a previous run left behind (crash); deleted when the session starts.
    leftover: HashSet<String>,
}

/// Info hashes of torrents opened for streaming, kept on disk so a crash
/// mid-stream is cleaned up the next time the session starts.
const STREAMS_MARKER: &str = "streams.json";

impl RqbitEngine {
    pub fn new(config: TorrentConfig, runtime: Handle) -> Self {
        Self::with_policy(config, runtime, TorrentPolicy::default())
    }

    pub fn with_policy(config: TorrentConfig, runtime: Handle, policy: TorrentPolicy) -> Self {
        let leftover: HashSet<String> = std::fs::read(config.root.join(STREAMS_MARKER))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            config,
            policy: Mutex::new(policy),
            runtime,
            session: Mutex::new(None),
            roles: std::sync::Mutex::new(TorrentRoles {
                leftover,
                ..TorrentRoles::default()
            }),
            viewers: Arc::new(Viewers::default()),
        }
    }

    fn streams_marker(&self) -> PathBuf {
        self.config.root.join(STREAMS_MARKER)
    }

    fn write_streams_marker(&self, roles: &TorrentRoles) {
        let path = self.streams_marker();
        let mut list: Vec<&String> = roles.streams.union(&roles.leftover).collect();
        if list.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        list.sort();
        let _ = std::fs::create_dir_all(&self.config.root);
        if let Ok(json) = serde_json::to_vec(&list) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    fn mark_stream(&self, info_hash: &str) {
        if let Ok(mut roles) = self.roles.lock()
            && !roles.downloads.contains(info_hash)
            && roles.streams.insert(info_hash.to_owned())
        {
            self.write_streams_marker(&roles);
        }
    }

    pub(crate) fn mark_download(&self, info_hash: &str) {
        if let Ok(mut roles) = self.roles.lock() {
            roles.downloads.insert(info_hash.to_owned());
            if roles.streams.remove(info_hash) {
                self.write_streams_marker(&roles);
            }
        }
    }

    pub(crate) fn unmark_download(&self, info_hash: &str) {
        if let Ok(mut roles) = self.roles.lock() {
            roles.downloads.remove(info_hash);
        }
    }

    /// Deletes every streaming torrent (and its files) from the running session.
    /// Torrents that are also being downloaded are kept.
    pub async fn release_streams(&self) {
        let hashes: Vec<String> = match self.roles.lock() {
            Ok(mut roles) => {
                let hashes = roles.streams.drain().collect();
                self.write_streams_marker(&roles);
                hashes
            }
            Err(_) => return,
        };
        let session = self.session.lock().await.clone();
        if let Some(session) = session {
            delete_torrents(&session, &hashes).await;
        }
    }

    /// Applies new settings. Rate limits change live on the running session,
    /// so playback and downloads continue. Only a different network
    /// interface (VPN binding / kill switch) needs a new session; that one
    /// change interrupts active torrents, by design.
    pub async fn update_policy(&self, policy: TorrentPolicy) {
        let (rebind, limits_changed) = {
            let mut g = self.policy.lock().await;
            let rebind = g.bind_device_name != policy.bind_device_name
                || g.vpn_kill_switch != policy.vpn_kill_switch;
            let limits_changed = effective_upload_bps(&g) != effective_upload_bps(&policy)
                || g.download_bps != policy.download_bps;
            *g = policy.clone();
            (rebind, limits_changed)
        };
        if rebind {
            self.release_streams().await;
            let old = self.session.lock().await.take();
            if let Some(session) = old {
                session.stop().await;
            }
            return;
        }
        if limits_changed && let Some(session) = self.session.lock().await.clone() {
            session
                .ratelimits
                .set_upload_bps(effective_upload_bps(&policy).and_then(NonZeroU32::new));
            session
                .ratelimits
                .set_download_bps(policy.download_bps.and_then(NonZeroU32::new));
        }
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.config.root.join("downloads")
    }

    pub(crate) async fn session(&self) -> Result<Arc<Session>, TorrentError> {
        let mut guard = self.session.lock().await;
        if let Some(session) = guard.as_ref() {
            return Ok(Arc::clone(session));
        }
        let policy = self.policy.lock().await.clone();
        let bind_dev = policy
            .bind_device_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if policy.vpn_kill_switch && bind_dev.is_none() {
            return Err(TorrentError::Engine(
                "VPN kill switch is enabled but no VPN network interface is selected".into(),
            ));
        }
        let downloads = self.downloads_dir();
        std::fs::create_dir_all(&downloads).map_err(|e| TorrentError::Engine(e.to_string()))?;
        let session_dir = self.config.root.join("session");
        if policy.fastresume {
            let _ = std::fs::create_dir_all(&session_dir);
        }
        let upload_bps = effective_upload_bps(&policy);
        let options = SessionOptions {
            fastresume: policy.fastresume,
            persistence: policy.fastresume.then_some(SessionPersistenceConfig::Json {
                folder: Some(session_dir),
            }),
            dht: (!self.config.local_only).then(|| dht_config(self.config.root.join("dht.json"))),
            bind_device_name: bind_dev,
            disable_local_service_discovery: self.config.local_only,
            peer_limit: Some(40),
            ratelimits: LimitsConfig {
                upload_bps: upload_bps.and_then(NonZeroU32::new),
                download_bps: policy.download_bps.and_then(NonZeroU32::new),
            },
            connect: Some(ConnectionOptions {
                enable_tcp: true,
                proxy_url: None,
                peer_opts: Some(PeerConnectionOptions {
                    connect_timeout: Some(Duration::from_secs(4)),
                    read_write_timeout: Some(Duration::from_secs(8)),
                    keep_alive_interval: None,
                    // Smaller request window lets peers pivot quickly after a seek.
                    max_request_window: Some(16),
                }),
            }),
            ..Default::default()
        };
        let session = Session::new_with_opts(downloads, options).await?;
        // Streams left over from a crash were restored by persistence: delete them.
        let leftover: Vec<String> = self
            .roles
            .lock()
            .map(|r| {
                r.leftover
                    .iter()
                    .filter(|h| !r.streams.contains(*h) && !r.downloads.contains(*h))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if !leftover.is_empty() {
            delete_torrents(&session, &leftover).await;
        }
        if let Ok(mut roles) = self.roles.lock()
            && !roles.leftover.is_empty()
        {
            roles.leftover.clear();
            self.write_streams_marker(&roles);
        }
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }

    async fn open(&self, request: TorrentFileRequest) -> Result<Arc<dyn ByteSource>, TorrentError> {
        let info_hash = normalize_info_hash(&request.info_hash)?;
        // Mark before adding, so a crash during add is cleaned up next start.
        self.mark_stream(&info_hash);
        self.open_with_handle(request, None)
            .await
            .map(|(_, s, _)| s)
    }

    /// Opens a torrent for downloading into `staging` (a folder we own: the
    /// torrent may write any file name inside it). If the torrent is already
    /// in the session (e.g. it was streaming), it stays where it is; callers
    /// must use the returned path, not assume `staging`.
    pub async fn open_download(
        &self,
        request: TorrentFileRequest,
        staging: &std::path::Path,
    ) -> Result<(Arc<ManagedTorrent>, Arc<dyn ByteSource>, PathBuf), TorrentError> {
        let info_hash = normalize_info_hash(&request.info_hash)?;
        // Streaming the same torrent: drop the stream (and its cache file)
        // so the download is written once, in its own staging folder.
        let was_stream = self
            .roles
            .lock()
            .map(|r| r.streams.contains(&info_hash))
            .unwrap_or(false);
        if was_stream && let Some(session) = self.session.lock().await.clone() {
            delete_torrents(&session, std::slice::from_ref(&info_hash)).await;
        }
        self.mark_download(&info_hash);
        let (handle, source, path) = self.open_with_handle(request, Some(staging)).await?;
        let path = path.ok_or(TorrentError::NoFiles)?;
        Ok((handle, source, path))
    }

    /// "Play Now" for a download in progress. The download will not leave the
    /// session or move its file until every returned source is dropped.
    pub async fn watch_download(
        &self,
        request: TorrentFileRequest,
        staging: &std::path::Path,
    ) -> Result<Arc<dyn ByteSource>, TorrentError> {
        let info_hash = normalize_info_hash(&request.info_hash)?;
        let (_, inner, _) = self.open_download(request, staging).await?;
        if let Ok(mut counts) = self.viewers.counts.lock() {
            *counts.entry(info_hash.clone()).or_default() += 1;
        }
        Ok(Arc::new(WatchedSource {
            inner,
            hash: info_hash,
            viewers: Arc::clone(&self.viewers),
        }))
    }

    /// Waits until no "Play Now" source for `info_hash` is open.
    pub async fn wait_until_unwatched(&self, info_hash: &str) {
        loop {
            let changed = self.viewers.changed.notified();
            if self.viewers.count(info_hash) == 0 {
                return;
            }
            changed.await;
        }
    }

    /// Removes a torrent from the session but keeps its files (pause, finish).
    pub async fn forget(&self, handle: &Arc<ManagedTorrent>) {
        let session = self.session.lock().await.clone();
        if let Some(session) = session
            && let Err(e) = session
                .delete(librqbit::api::TorrentIdOrHash::Id(handle.id()), false)
                .await
        {
            eprintln!("[cinma] torrent: could not detach download: {e}");
        }
    }

    /// Lists video files inside a torrent (`list_only`, without downloading content) and parses S/E tags.
    pub async fn list_files(
        &self,
        raw_info_hash: &str,
        extra_trackers: &[String],
    ) -> Result<Vec<TorrentFileItem>, TorrentError> {
        let info_hash = normalize_info_hash(raw_info_hash)?;
        let session = self.session().await?;
        let mut trackers = extra_trackers.to_vec();
        if !self.config.local_only {
            for tr in FALLBACK_TRACKERS {
                if !trackers.iter().any(|existing| existing == tr) {
                    trackers.push((*tr).to_owned());
                }
            }
        }
        let magnet = magnet_uri(&info_hash, &trackers);
        let initial_peers =
            (!self.config.extra_peers.is_empty()).then(|| self.config.extra_peers.clone());
        let response = tokio::time::timeout(
            METADATA_TIMEOUT,
            session.add_torrent(
                AddTorrent::from_url(magnet),
                Some(AddTorrentOptions {
                    list_only: true,
                    initial_peers,
                    ..Default::default()
                }),
            ),
        )
        .await
        .map_err(|_| TorrentError::MetadataTimeout)??;
        let files: Vec<(String, u64)> = match response {
            AddTorrentResponse::ListOnly(list) => list
                .info
                .iter_file_details()
                .map(|f| (f.filename.to_string(), f.len))
                .collect(),
            AddTorrentResponse::AlreadyManaged(_, handle)
            | AddTorrentResponse::Added(_, handle) => handle.with_metadata(|m| {
                m.file_infos
                    .iter()
                    .map(|f| (f.relative_filename.to_string_lossy().into_owned(), f.len))
                    .collect::<Vec<_>>()
            })?,
        };
        let mut items: Vec<TorrentFileItem> = files
            .into_iter()
            .enumerate()
            .filter(|(_, (name, _))| is_video_filename(name))
            .map(|(index, (name, size_bytes))| {
                let (season, episode) = parse_episode_from_filename(&name);
                TorrentFileItem {
                    index,
                    name,
                    size_bytes,
                    season,
                    episode,
                }
            })
            .collect();
        items.sort_by_key(|it| {
            (
                it.season.unwrap_or(u32::MAX),
                it.episode.unwrap_or(u32::MAX),
                it.index,
            )
        });
        Ok(items)
    }

    pub(crate) async fn open_with_handle(
        &self,
        request: TorrentFileRequest,
        output_folder: Option<&std::path::Path>,
    ) -> Result<(Arc<ManagedTorrent>, Arc<dyn ByteSource>, Option<PathBuf>), TorrentError> {
        let info_hash = normalize_info_hash(&request.info_hash)?;
        let session = self.session().await?;
        let mut trackers = request.trackers.clone();
        if !self.config.local_only {
            for tr in FALLBACK_TRACKERS {
                if !trackers.iter().any(|existing| existing == tr) {
                    trackers.push((*tr).to_owned());
                }
            }
        }
        let magnet = magnet_uri(&info_hash, &trackers);
        let mut initial_peers =
            (!self.config.extra_peers.is_empty()).then(|| self.config.extra_peers.clone());
        let mut torrent_bytes = None;

        let file_idx = match request.file_idx {
            Some(idx) => idx,
            None => {
                let response = tokio::time::timeout(
                    METADATA_TIMEOUT,
                    session.add_torrent(
                        AddTorrent::from_url(magnet.clone()),
                        Some(AddTorrentOptions {
                            list_only: true,
                            initial_peers: initial_peers.clone(),
                            ..Default::default()
                        }),
                    ),
                )
                .await
                .map_err(|_| TorrentError::MetadataTimeout)??;
                match response {
                    AddTorrentResponse::ListOnly(list) => {
                        let files: Vec<(String, u64)> = list
                            .info
                            .iter_file_details()
                            .map(|f| (f.filename.to_string(), f.len))
                            .collect();
                        if !list.seen_peers.is_empty() {
                            let merged = initial_peers.get_or_insert_with(Vec::new);
                            merged.extend(list.seen_peers);
                        }
                        torrent_bytes = Some(list.torrent_bytes);
                        pick_video_file(&files).ok_or(TorrentError::NoFiles)?
                    }
                    AddTorrentResponse::AlreadyManaged(_, handle)
                    | AddTorrentResponse::Added(_, handle) => {
                        let files = handle.with_metadata(|m| {
                            m.file_infos
                                .iter()
                                .map(|f| {
                                    (f.relative_filename.to_string_lossy().into_owned(), f.len)
                                })
                                .collect::<Vec<_>>()
                        })?;
                        pick_video_file(&files).ok_or(TorrentError::NoFiles)?
                    }
                }
            }
        };

        let add_source = match torrent_bytes {
            Some(bytes) => AddTorrent::from_bytes(bytes),
            None => AddTorrent::from_url(magnet),
        };
        let response = tokio::time::timeout(
            METADATA_TIMEOUT,
            session.add_torrent(
                add_source,
                Some(AddTorrentOptions {
                    only_files: Some(vec![file_idx]),
                    // Safe: streams go to our cache folder and downloads to a
                    // per-torrent staging folder we own, never a user folder.
                    overwrite: true,
                    output_folder: output_folder.map(|p| p.to_string_lossy().into_owned()),
                    initial_peers,
                    trackers: Some(trackers),
                    ..Default::default()
                }),
            ),
        )
        .await
        .map_err(|_| TorrentError::MetadataTimeout)??;

        let (handle, already_managed) = match response {
            AddTorrentResponse::Added(_, handle) => (handle, false),
            AddTorrentResponse::AlreadyManaged(_, handle) => (handle, true),
            AddTorrentResponse::ListOnly(_) => {
                return Err(TorrentError::Engine("unexpected list-only response".into()));
            }
        };
        if already_managed {
            let mut only: HashSet<usize> = handle
                .only_files()
                .unwrap_or_default()
                .into_iter()
                .collect();
            if only.insert(file_idx) {
                session.update_only_files(&handle, &only).await?;
            }
        }
        handle.wait_until_initialized().await?;

        let (len, count, name) = handle.with_metadata(|m| {
            let count = m.file_infos.len();
            let info = m.file_infos.get(file_idx);
            (
                info.map(|f| f.len),
                count,
                info.map(|f| f.relative_filename.to_string_lossy().into_owned()),
            )
        })?;
        let len = len.ok_or(TorrentError::FileIndex {
            index: file_idx,
            count,
        })?;
        let name = name.unwrap_or_default();
        let disk_path = (!name.is_empty()).then(|| handle.output_folder().join(&name));
        let filename = std::path::Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        let available = Arc::new(std::sync::Mutex::new(RangeSet::new()));
        let warmup = Arc::new(std::sync::Mutex::new(Some(spawn_head_tail_warmup(
            &self.runtime,
            Arc::clone(&handle),
            file_idx,
            len,
            Arc::clone(&available),
        ))));
        let file_path = disk_path.clone();
        Ok((
            Arc::clone(&handle),
            Arc::new(TorrentSource {
                handle,
                file_idx,
                len,
                name,
                filename,
                disk_path,
                available,
                runtime: self.runtime.clone(),
                warmup,
            }),
            file_path,
        ))
    }

    /// Stops the session. Under Phase 5 DiskPolicy (`delete_on_release == false`),
    /// downloaded files stay managed by LRU quota instead of unconditional delete.
    async fn release(&self) {
        self.release_streams().await;
        let delete = self.policy.lock().await.delete_on_release;
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            session.stop().await;
            drop(session);
            if delete {
                let downloads = self.config.root.join("downloads");
                if let Err(e) = tokio::fs::remove_dir_all(&downloads).await
                    && e.kind() != io::ErrorKind::NotFound
                {
                    eprintln!(
                        "[cinma] torrent: could not clear {}: {e}",
                        downloads.display()
                    );
                }
            }
        }
    }
}

impl TorrentEngine for RqbitEngine {
    fn open_file(
        &self,
        request: TorrentFileRequest,
    ) -> BoxFuture<'_, Result<Arc<dyn ByteSource>, TorrentError>> {
        Box::pin(self.open(request))
    }

    fn release_all(&self) -> BoxFuture<'_, ()> {
        Box::pin(self.release())
    }
}

/// Upload cap actually applied: with seeding off, only minimal protocol chatter.
fn effective_upload_bps(policy: &TorrentPolicy) -> Option<u32> {
    match policy.seeding {
        SeedingPolicy::Off => policy.upload_bps.or(Some(1024)),
        _ => policy.upload_bps,
    }
}

/// Removes the given torrents and their files from `session` (best effort).
async fn delete_torrents(session: &Arc<Session>, info_hashes: &[String]) {
    for hash in info_hashes {
        let id = session.with_torrents(|torrents| {
            let mut found = None;
            for (id, t) in torrents {
                if t.info_hash().as_string() == *hash {
                    found = Some(id);
                    break;
                }
            }
            found
        });
        if let Some(id) = id
            && let Err(e) = session
                .delete(librqbit::api::TorrentIdOrHash::Id(id), true)
                .await
        {
            eprintln!("[cinma] torrent: could not remove stream {hash}: {e}");
        }
    }
}

fn dht_config(state_file: PathBuf) -> DhtSessionConfig {
    DhtSessionConfig {
        persistence: Some(DhtPersistenceConfig {
            dump_interval: None,
            config_filename: Some(state_file),
        }),
        ..Default::default()
    }
}

/// Lower-cases and validates a 40-char hex info hash.
pub fn normalize_info_hash(input: &str) -> Result<String, TorrentError> {
    let hash = input.trim().to_ascii_lowercase();
    if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(TorrentError::InvalidInfoHash(input.to_owned()))
    }
}

pub fn magnet_uri(info_hash: &str, trackers: &[String]) -> String {
    let mut uri = format!("magnet:?xt=urn:btih:{info_hash}");
    for tracker in trackers {
        uri.push_str("&tr=");
        uri.push_str(&percent_encode(tracker));
    }
    uri
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Largest file with a video extension; falls back to the largest file.
pub fn pick_video_file(files: &[(String, u64)]) -> Option<usize> {
    let largest = |videos_only: bool| {
        files
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| !videos_only || is_video_filename(name))
            .max_by_key(|(_, (_, len))| *len)
            .map(|(i, _)| i)
    };
    largest(true).or_else(|| largest(false))
}

fn is_video_filename(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFileItem {
    pub index: usize,
    pub name: String,
    pub size_bytes: u64,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

/// Extracts `(season, episode)` from typical scene/anime filenames (`S01E02`, `1x02`, `- 05`).
pub fn parse_episode_from_filename(name: &str) -> (Option<u32>, Option<u32>) {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b's'
            && let Some((s, s_len)) = read_digits(&bytes[i + 1..], 2)
            && bytes.get(i + 1 + s_len) == Some(&b'e')
            && let Some((e, _)) = read_digits(&bytes[i + 2 + s_len..], 3)
        {
            return (Some(s), Some(e));
        }
    }
    for i in 1..bytes.len() {
        if bytes[i] == b'x'
            && bytes[i - 1].is_ascii_digit()
            && let Some((e, _)) = read_digits(&bytes[i + 1..], 3)
        {
            let mut start = i;
            while start > 0 && bytes[start - 1].is_ascii_digit() {
                start -= 1;
            }
            if let Ok(s) = lower[start..i].parse::<u32>()
                && s <= 99
            {
                return (Some(s), Some(e));
            }
        }
    }
    if let Some(pos) = lower.find(" - ")
        && let Some((e, e_len)) = read_digits(&bytes[pos + 3..], 3)
        && bytes
            .get(pos + 3 + e_len)
            .is_some_and(|b| !b.is_ascii_alphanumeric())
    {
        return (Some(1), Some(e));
    }
    (None, None)
}

fn read_digits(slice: &[u8], max_len: usize) -> Option<(u32, usize)> {
    let mut len = 0usize;
    while len < slice.len() && len < max_len && slice[len].is_ascii_digit() {
        len += 1;
    }
    if len == 0 {
        return None;
    }
    let val = std::str::from_utf8(&slice[..len]).ok()?.parse().ok()?;
    Some((val, len))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTorrentInput {
    pub info_hash: String,
    pub name: Option<String>,
    pub trackers: Vec<String>,
}

/// Parses a `magnet:?xt=urn:btih:...` URI into a normalized 40-char hex `info_hash`,
/// optional display name, and tracker list.
pub fn parse_magnet_uri_meta(uri: &str) -> Result<ParsedTorrentInput, TorrentError> {
    let parsed = librqbit::Magnet::parse(uri.trim())
        .map_err(|e| TorrentError::InvalidInfoHash(e.to_string()))?;
    let hash_id = parsed
        .as_id20()
        .ok_or_else(|| TorrentError::InvalidInfoHash("magnet URI missing v1 info_hash".into()))?;
    let info_hash = hash_id.as_string();
    let name = parsed.name.filter(|s| !s.trim().is_empty());
    let trackers = parsed
        .trackers
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    Ok(ParsedTorrentInput {
        info_hash,
        name,
        trackers,
    })
}

/// Parses raw `.torrent` metainfo bytes into a normalized 40-char hex `info_hash`,
/// optional torrent title, and tracker list.
pub fn parse_torrent_file_meta(bytes: &[u8]) -> Result<ParsedTorrentInput, TorrentError> {
    let parsed = librqbit::torrent_from_bytes(bytes)
        .map_err(|e| TorrentError::InvalidInfoHash(e.to_string()))?;
    let info_hash = parsed.info_hash.as_string();
    let name = parsed
        .info
        .data
        .name
        .as_ref()
        .and_then(|b| std::str::from_utf8(b.as_ref()).ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let trackers: Vec<String> = parsed
        .iter_announce()
        .filter_map(|u| std::str::from_utf8(u.as_ref()).ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(ParsedTorrentInput {
        info_hash,
        name,
        trackers,
    })
}

struct TorrentSource {
    handle: Arc<ManagedTorrent>,
    file_idx: usize,
    len: u64,
    name: String,
    filename: String,
    disk_path: Option<PathBuf>,
    available: Arc<std::sync::Mutex<RangeSet>>,
    runtime: Handle,
    warmup: Arc<std::sync::Mutex<Option<AbortHandle>>>,
}

impl Drop for TorrentSource {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.warmup.lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
    }
}

fn spawn_head_tail_warmup(
    runtime: &Handle,
    handle: Arc<ManagedTorrent>,
    file_idx: usize,
    len: u64,
    available: Arc<std::sync::Mutex<RangeSet>>,
) -> AbortHandle {
    runtime
        .spawn(async move {
            if len == 0 {
                return;
            }
            let head_len = (len as usize).min(OPENSUBTITLES_CHUNK_SIZE);
            let avail_head = Arc::clone(&available);
            let h = Arc::clone(&handle);
            let head = async move {
                if let Ok(mut s) = h.stream(file_idx).await {
                    let mut buf = vec![0u8; head_len];
                    let mut read_total = 0usize;
                    while read_total < head_len {
                        match s.read(&mut buf[read_total..]).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if let Ok(mut guard) = avail_head.lock() {
                                    guard.insert(read_total as u64..(read_total + n) as u64);
                                }
                                read_total += n;
                            }
                        }
                    }
                }
            };
            let tail_window = len.min(WARMUP_TAIL_WINDOW.max(OPENSUBTITLES_CHUNK_SIZE as u64));
            let tail_offset = len.saturating_sub(tail_window);
            let avail_tail = Arc::clone(&available);
            let tail = async move {
                if tail_offset > 0
                    && let Ok(mut s) = handle.stream(file_idx).await
                    && s.seek(SeekFrom::Start(tail_offset)).await.is_ok()
                {
                    let want = (len - tail_offset) as usize;
                    let mut buf = vec![0u8; want];
                    let mut read_total = 0usize;
                    while read_total < want {
                        match s.read(&mut buf[read_total..]).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let abs = tail_offset + read_total as u64;
                                if let Ok(mut guard) = avail_tail.lock() {
                                    guard.insert(abs..abs + n as u64);
                                }
                                read_total += n;
                            }
                        }
                    }
                }
            };
            let _ = tokio::time::timeout(WARMUP_TIMEOUT, async {
                tokio::join!(head, tail);
            })
            .await;
        })
        .abort_handle()
}

impl ByteSource for TorrentSource {
    fn size(&self) -> u64 {
        self.len
    }

    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        // Called from mpv's demuxer thread (never a runtime worker).
        let stream = self
            .runtime
            .block_on(self.handle.clone().stream(self.file_idx))
            .map_err(|e| io::Error::other(format!("{e:#}")))?;
        Ok(Box::new(TorrentReader {
            stream: Box::new(stream),
            position: 0,
            len: self.len,
            runtime: self.runtime.clone(),
            warmup: Arc::clone(&self.warmup),
            available: Arc::clone(&self.available),
        }))
    }

    fn describe(&self) -> String {
        format!(
            "torrent {} #{} {}",
            self.handle.info_hash().as_string(),
            self.file_idx,
            self.name
        )
    }

    fn filename(&self) -> Option<&str> {
        (!self.filename.is_empty()).then_some(&self.filename)
    }

    fn available_ranges(&self) -> Option<RangeSet> {
        self.available.lock().ok().map(|r| r.clone())
    }

    fn stats(&self) -> Option<SourceStats> {
        let st = self.handle.stats();
        let (dl_bps, ul_bps, live_peers, queued_peers, seen_peers) = match st.live.as_ref() {
            Some(live) => (
                (live.download_speed.mbps * 1_048_576.0).max(0.0) as u64,
                (live.upload_speed.mbps * 1_048_576.0).max(0.0) as u64,
                live.snapshot.peer_stats.live,
                live.snapshot.peer_stats.queued + live.snapshot.peer_stats.connecting,
                live.snapshot.peer_stats.seen,
            ),
            None => (0, 0, 0, 0, 0),
        };
        Some(SourceStats {
            is_torrent: true,
            info_hash: Some(self.handle.info_hash().as_string()),
            download_speed_bps: dl_bps,
            upload_speed_bps: ul_bps,
            peers_connected: live_peers,
            peers_queued: queued_peers,
            peers_seen: seen_peers,
            downloaded_bytes: st.progress_bytes.min(self.len),
            total_bytes: self.len,
            finished: st.finished,
        })
    }

    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        if offset >= self.len {
            return Some(0);
        }
        if buf.is_empty() {
            return Some(0);
        }
        let run_end = self.available.lock().ok()?.run_end(offset)?;
        let n = (run_end - offset).min(buf.len() as u64) as usize;
        if n == 0 {
            return Some(0);
        }
        let path = self.disk_path.as_ref()?;
        let file = std::fs::File::open(path).ok()?;
        file.read_exact_at(&mut buf[..n], offset).ok()?;
        Some(n)
    }
}

struct TorrentReader {
    stream: Box<dyn AsyncReadSeek>,
    /// Current stream position; `u64::MAX` when unknown (after a cancel).
    position: u64,
    len: u64,
    runtime: Handle,
    warmup: Arc<std::sync::Mutex<Option<AbortHandle>>>,
    available: Arc<std::sync::Mutex<RangeSet>>,
}

impl SourceReader for TorrentReader {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        // Once the player seeks into the middle of the file, cancel the head/tail
        // warmup streams so they don't compete with the active seek cursor.
        const HEAD_TAIL_GUARD: u64 = 4 * 1024 * 1024;
        if offset > HEAD_TAIL_GUARD
            && offset < self.len.saturating_sub(HEAD_TAIL_GUARD)
            && let Ok(mut guard) = self.warmup.lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
        let Self {
            stream,
            position,
            runtime,
            ..
        } = self;
        let result = runtime.block_on(async {
            tokio::select! {
                biased;
                () = cancel.cancelled() => Err(cancelled_error()),
                r = async {
                    if *position != offset {
                        *position = u64::MAX;
                        stream.seek(SeekFrom::Start(offset)).await?;
                        *position = offset;
                    }
                    let n = stream.read(buf).await?;
                    *position += n as u64;
                    Ok(n)
                } => r,
            }
        });
        match &result {
            Ok(n) if *n > 0 => {
                if let Ok(mut guard) = self.available.lock() {
                    guard.insert(offset..offset + *n as u64);
                }
            }
            Err(_) => {
                self.position = u64::MAX;
            }
            _ => {}
        }
        result
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn info_hash_validation() {
        assert_eq!(
            normalize_info_hash(" 08ADA5A7A6183AAE1E09D831DF6748D566095A10 ").unwrap(),
            "08ada5a7a6183aae1e09d831df6748d566095a10"
        );
        assert!(normalize_info_hash("xyz").is_err());
        assert!(normalize_info_hash(&"g".repeat(40)).is_err());
    }

    #[test]
    fn magnet_encodes_trackers() {
        let m = magnet_uri("ab", &["udp://t.example:1337/announce".into()]);
        assert_eq!(
            m,
            "magnet:?xt=urn:btih:ab&tr=udp%3A%2F%2Ft.example%3A1337%2Fannounce"
        );
    }

    #[test]
    fn picks_largest_video() {
        let files = vec![
            ("sample.mkv".to_owned(), 50),
            ("movie.MKV".to_owned(), 5_000),
            ("extras.iso".to_owned(), 9_000),
            ("readme.txt".to_owned(), 1),
        ];
        assert_eq!(pick_video_file(&files), Some(1));
        assert_eq!(
            pick_video_file(&[("a.bin".into(), 3), ("b.bin".into(), 7)]),
            Some(1)
        );
        assert_eq!(pick_video_file(&[]), None);
    }

    #[test]
    fn parses_season_and_episode_from_filenames() {
        assert_eq!(
            parse_episode_from_filename("Show.S02E14.1080p.WEB-DL.mkv"),
            (Some(2), Some(14))
        );
        assert_eq!(
            parse_episode_from_filename("Series 1x05 Finale.mp4"),
            (Some(1), Some(5))
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Anime - 12 [1080p].mkv"),
            (Some(1), Some(12))
        );
        assert_eq!(
            parse_episode_from_filename("Standalone.Movie.2024.2160p.mkv"),
            (None, None)
        );
    }
}
