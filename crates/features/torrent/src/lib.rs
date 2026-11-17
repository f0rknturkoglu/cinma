//! Torrent streaming behind the [`TorrentEngine`] trait (librqbit today).
//!
//! The session is created lazily on the first torrent and torn down by
//! [`TorrentEngine::release_all`] so DHT/peer traffic stops when nothing plays.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::future::Future;
use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cinma_blobstore::{ByteSource, CancellationToken, SourceReader, cancelled_error};
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ManagedTorrent, Session,
    SessionOptions,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};
use tokio::runtime::Handle;
use tokio::sync::Mutex;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

const METADATA_TIMEOUT: Duration = Duration::from_secs(90);
const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv"];

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

pub struct RqbitEngine {
    config: TorrentConfig,
    runtime: Handle,
    session: Mutex<Option<Arc<Session>>>,
}

impl RqbitEngine {
    pub fn new(config: TorrentConfig, runtime: Handle) -> Self {
        Self {
            config,
            runtime,
            session: Mutex::new(None),
        }
    }

    async fn session(&self) -> Result<Arc<Session>, TorrentError> {
        let mut guard = self.session.lock().await;
        if let Some(session) = guard.as_ref() {
            return Ok(Arc::clone(session));
        }
        let downloads = self.config.root.join("downloads");
        std::fs::create_dir_all(&downloads).map_err(|e| TorrentError::Engine(e.to_string()))?;
        let options = SessionOptions {
            fastresume: false,
            persistence: None,
            dht: (!self.config.local_only).then(|| dht_config(self.config.root.join("dht.json"))),
            disable_local_service_discovery: self.config.local_only,
            ..Default::default()
        };
        let session = Session::new_with_opts(downloads, options).await?;
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }

    async fn open(&self, request: TorrentFileRequest) -> Result<Arc<dyn ByteSource>, TorrentError> {
        let info_hash = normalize_info_hash(&request.info_hash)?;
        let session = self.session().await?;
        let magnet = magnet_uri(&info_hash, &request.trackers);
        let initial_peers =
            (!self.config.extra_peers.is_empty()).then(|| self.config.extra_peers.clone());

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
                        pick_video_file(&files).ok_or(TorrentError::NoFiles)?
                    }
                    AddTorrentResponse::AlreadyManaged(_, handle)
                    | AddTorrentResponse::Added(_, handle) => {
                        let files = handle.with_metadata(|m| {
                            m.file_infos
                                .iter()
                                .map(|f| (f.relative_filename.to_string_lossy().into_owned(), f.len))
                                .collect::<Vec<_>>()
                        })?;
                        pick_video_file(&files).ok_or(TorrentError::NoFiles)?
                    }
                }
            }
        };

        let response = tokio::time::timeout(
            METADATA_TIMEOUT,
            session.add_torrent(
                AddTorrent::from_url(magnet),
                Some(AddTorrentOptions {
                    only_files: Some(vec![file_idx]),
                    overwrite: true,
                    initial_peers,
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
            let mut only: HashSet<usize> = handle.only_files().unwrap_or_default().into_iter().collect();
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
        let len = len.ok_or(TorrentError::FileIndex { index: file_idx, count })?;
        Ok(Arc::new(TorrentSource {
            handle,
            file_idx,
            len,
            name: name.unwrap_or_default(),
            runtime: self.runtime.clone(),
        }))
    }

    async fn release(&self) {
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            session.stop().await;
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
    let is_video = |name: &str| {
        name.rsplit_once('.')
            .is_some_and(|(_, ext)| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
    };
    let largest = |videos_only: bool| {
        files
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| !videos_only || is_video(name))
            .max_by_key(|(_, (_, len))| *len)
            .map(|(i, _)| i)
    };
    largest(true).or_else(|| largest(false))
}

struct TorrentSource {
    handle: Arc<ManagedTorrent>,
    file_idx: usize,
    len: u64,
    name: String,
    runtime: Handle,
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
            runtime: self.runtime.clone(),
        }))
    }

    fn describe(&self) -> String {
        format!("torrent {} #{} {}", self.handle.info_hash().as_string(), self.file_idx, self.name)
    }
}

struct TorrentReader {
    stream: Box<dyn AsyncReadSeek>,
    /// Current stream position; `u64::MAX` when unknown (after a cancel).
    position: u64,
    runtime: Handle,
}

impl SourceReader for TorrentReader {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        let Self { stream, position, runtime } = self;
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
        if result.is_err() {
            self.position = u64::MAX;
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
        assert_eq!(m, "magnet:?xt=urn:btih:ab&tr=udp%3A%2F%2Ft.example%3A1337%2Fannounce");
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
        assert_eq!(pick_video_file(&[("a.bin".into(), 3), ("b.bin".into(), 7)]), Some(1));
        assert_eq!(pick_video_file(&[]), None);
    }
}
