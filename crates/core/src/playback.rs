//! Playback session: resolves a stream to bytes and drives the player.

use std::sync::Arc;

use cinma_player::{Player, PlayerEvents};
use cinma_stremio::Stream;
use cinma_torrent::{TorrentEngine, TorrentFileRequest};

use crate::{Core, CoreError};

/// One player bound to a host view. Dropping it (or `stop`) destroys mpv.
pub struct Playback {
    core: Arc<Core>,
    player: Player,
}

impl Core {
    /// Creates the player (loads libmpv on first use). Blocking — call off
    /// the main thread.
    pub fn create_playback(self: &Arc<Self>, events: Arc<dyn PlayerEvents>) -> Result<Playback, CoreError> {
        Ok(Playback {
            core: Arc::clone(self),
            player: Player::new(events)?,
        })
    }
}

impl Playback {
    /// Resolves `stream` (torrent → byte source, URL → direct) and starts it.
    pub async fn play(&self, stream: Stream) -> Result<(), CoreError> {
        self.player.begin_load();
        if let Some(info_hash) = stream.info_hash.clone() {
            let engine = self.core.torrents()?;
            let request = TorrentFileRequest {
                info_hash,
                file_idx: stream.file_idx.map(|i| i as usize),
                trackers: stream.trackers(),
            };
            let source = self
                .core
                .spawn(async move { Ok(engine.open_file(request).await?) })
                .await?;
            self.player.load_source(source)?;
            Ok(())
        } else if let Some(url) = stream.url.as_deref() {
            self.player.load_url(url)?;
            Ok(())
        } else {
            Err(CoreError::Invalid("stream has neither infoHash nor url".into()))
        }
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    /// Destroys mpv and stops torrent activity (DHT, peers) so the app goes
    /// back to idle.
    pub async fn stop(&self) -> Result<(), CoreError> {
        self.player.shutdown();
        if let Some(engine) = self.core.torrents.get().cloned() {
            self.core
                .spawn(async move {
                    engine.release_all().await;
                    Ok(())
                })
                .await?;
        }
        Ok(())
    }
}
