//! Phase 9 Integrations (`docs/ROADMAP.md` §8, `docs/adr/0017`):
//! - Skip segments (`Feature::SkipSegments`): AniSkip + IntroDB with SQLite 7-day TTL
//! - Debrid resolvers (`Feature::Debrid`): Real-Debrid, AllDebrid, Premiumize, TorBox
//! - Tracking OAuth PKCE (Simkl, AniList, MAL) & two-way Trakt pull + Anime ID refresh
//! - OpenSubtitles account login & quota
//! - Fanart.tv logos/backdrops, TMDB watch providers, Kitsu anime fallback
//! - Live TV / IPTV (`Feature::Iptv`): M3U, Xtream Codes, XMLTV EPG
//! - Discord Rich Presence (`Feature::Discord`): in-process Unix domain socket IPC
//! - VPN interface binding & kill switch settings update

mod debrid_skip;
mod iptv_discord;
mod tracking_extras;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cinma_debrid::DebridClient;
use cinma_discord::DiscordPresenceClient;
use cinma_iptv::IptvClient;
use cinma_metadata::{SkipSegmentClient, WatchProvider};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::{Core, CoreError, Feature};

pub(super) const SKIP_TTL_SECS: i64 = 7 * 24 * 3600;
pub(super) const KEY_PINNED_SHELVES: &str = "pinned_shelves";
pub(super) const KEY_HOME_CATALOGS: &str = "home.catalogs";

pub(super) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ManualSkipMarkers {
    pub intro_end_s: Option<f64>,
    pub outro_last_s: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebridProviderStatus {
    pub provider: String,
    pub display_name: String,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthStartResult {
    pub provider: String,
    pub authorize_url: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraktPullSummary {
    pub watchlist_items: u32,
    pub rating_items: u32,
    pub history_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPresenceInput {
    pub title: String,
    pub subtitle: Option<String>,
    pub duration_s: Option<u64>,
    pub position_s: u64,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleExtras {
    pub clearlogo: Option<String>,
    pub fanart_backdrop: Option<String>,
    pub watch_providers: Vec<WatchProvider>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PinnedShelf {
    pub id: String,
    pub title: String,
    pub addon_id: String,
    pub media_type: String,
    pub catalog_id: String,
    pub genre: Option<String>,
}

impl Core {
    // =========================================================================
    // Lazy feature accessors (never constructed when disabled)
    // =========================================================================

    pub(super) fn skip_client(&self) -> Result<SkipSegmentClient, CoreError> {
        self.require(Feature::SkipSegments)?;
        let mut guard = self.skip_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = SkipSegmentClient::new()?;
        *guard = Some(c.clone());
        Ok(c)
    }

    pub(super) fn debrid_client(&self) -> Result<DebridClient, CoreError> {
        self.require(Feature::Debrid)?;
        let mut guard = self.debrid_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = DebridClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        *guard = Some(c.clone());
        Ok(c)
    }

    pub(super) fn iptv_client(&self) -> Result<IptvClient, CoreError> {
        self.require(Feature::Iptv)?;
        let mut guard = self.iptv_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = IptvClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        *guard = Some(c.clone());
        Ok(c)
    }

    pub(super) fn discord_client(
        &self,
    ) -> Result<Arc<AsyncMutex<DiscordPresenceClient>>, CoreError> {
        self.require(Feature::Discord)?;
        let mut guard = self
            .discord_client
            .lock()
            .map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(Arc::clone(c));
        }
        let c = Arc::new(AsyncMutex::new(DiscordPresenceClient::new(None)));
        *guard = Some(Arc::clone(&c));
        Ok(c)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod p1_debrid_ranker_tests {
    use std::collections::HashSet;

    use super::*;
    use crate::PlayerPreferences;
    use crate::addons::StreamCandidate;

    #[test]
    fn rank_streams_for_debrid_applies_block_words_debrid_only_and_sort() {
        let dir = tempfile::tempdir().expect("tempdir");
        let core = Core::open(dir.path()).expect("open core");
        let prefs = PlayerPreferences {
            track_block_words: vec!["camrip".to_owned()],
            debrid_only_mode: true,
            stream_sort: "size_asc".to_owned(),
            ..Default::default()
        };
        core.set_player_preferences(&prefs).expect("set prefs");

        let cached_big = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let cached_small = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let uncached_hash = "cccccccccccccccccccccccccccccccccccccccc";
        let candidates = vec![
            StreamCandidate {
                addon_name: "Torrentio".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("1080p".to_owned()),
                    title: Some("Show 1080p 💾 5.0 GB".to_owned()),
                    info_hash: Some(cached_big.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "Torrentio".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("1080p".to_owned()),
                    title: Some("Show 1080p 💾 1.5 GB".to_owned()),
                    info_hash: Some(cached_small.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "TorrentioCAM".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("720p CAMRip".to_owned()),
                    title: Some("Show CAMRip 💾 500 MB".to_owned()),
                    info_hash: Some(cached_small.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "P2P".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("4K".to_owned()),
                    title: Some("Show 2160p 👤 999 💾 2.0 GB".to_owned()),
                    info_hash: Some(uncached_hash.to_owned()),
                    ..Default::default()
                },
            },
        ];

        let mut cached = HashSet::new();
        cached.insert(cached_big.to_owned());
        cached.insert(cached_small.to_owned());

        let ranked = core.rank_streams_for_debrid(candidates, &cached, None);
        assert_eq!(ranked.ranked.len(), 2);
        assert_eq!(
            ranked.ranked[0].stream.info_hash.as_deref(),
            Some(cached_small)
        );
        assert_eq!(
            ranked.ranked[1].stream.info_hash.as_deref(),
            Some(cached_big)
        );
    }

    #[test]
    fn p2_batch_epg_returns_max_two_upcoming_rows_per_channel() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(core.set_feature_enabled(Feature::Iptv, true))
            .unwrap();
        let now = now_unix();
        {
            let db = core.db.lock().unwrap();
            db.upsert_iptv_epg(
                &[
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now - 100,
                        stop_ts: now + 100,
                        title: "CH1 Now".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now + 100,
                        stop_ts: now + 200,
                        title: "CH1 Next".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now + 200,
                        stop_ts: now + 300,
                        title: "CH1 Later".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch2".into(),
                        start_ts: now - 50,
                        stop_ts: now + 50,
                        title: "CH2 Now".into(),
                        description: None,
                    },
                ],
                now - 3600,
            )
            .unwrap();
        }
        let rows = core.iptv_all_epg_now_next().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].title, "CH1 Now");
        assert_eq!(rows[1].title, "CH1 Next");
        assert_eq!(rows[2].title, "CH2 Now");
    }
}
