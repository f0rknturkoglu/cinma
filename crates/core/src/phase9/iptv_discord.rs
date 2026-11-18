use std::sync::Arc;

use cinma_db::{IptvChannelRow, IptvEpgRow, IptvSourceRow};
use cinma_discord::PresenceActivity;

use super::{DiscordPresenceInput, now_unix};
use crate::{Core, CoreError, Feature};

impl Core {
    // =========================================================================
    // 9. Live TV / IPTV (`Feature::Iptv`)
    // =========================================================================

    pub async fn add_m3u_source(
        self: &Arc<Self>,
        name: String,
        url: String,
        epg_url: Option<String>,
    ) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let fetch_url = url.clone();
        let channels = self
            .spawn(async move {
                client
                    .fetch_m3u(&fetch_url)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let profile = self.active_profile()?;
        let count = channels.len() as u32;
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let sid = db.upsert_iptv_source(
                profile,
                name.trim(),
                "m3u",
                url.trim(),
                None,
                epg_url.as_deref(),
            )?;
            db.replace_iptv_channels(profile, sid, &channels)?;
        }
        if let Some(epg) = epg_url.filter(|s| !s.trim().is_empty()) {
            let _ = self.refresh_xmltv(epg).await;
        }
        Ok(count)
    }

    pub async fn add_xtream_source(
        self: &Arc<Self>,
        name: String,
        base_url: String,
        username: String,
        password: String,
    ) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let (u, pw, base) = (username.clone(), password.clone(), base_url.clone());
        let channels = self
            .spawn(async move {
                client
                    .fetch_xtream_live(&base, &u, &pw)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let profile = self.active_profile()?;
        let count = channels.len() as u32;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let epg_url = format!(
            "{}/xmltv.php?username={username}&password={password}",
            base_url.trim_end_matches('/')
        );
        let sid = db.upsert_iptv_source(
            profile,
            name.trim(),
            "xtream",
            base_url.trim(),
            Some(username.trim()),
            Some(&epg_url),
        )?;
        db.replace_iptv_channels(profile, sid, &channels)?;
        Ok(count)
    }

    pub async fn refresh_xmltv(self: &Arc<Self>, url: String) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let rows = self
            .spawn(async move {
                client
                    .fetch_xmltv(&url)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let count = rows.len() as u32;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_iptv_epg(&rows, now_unix() - 3600)?;
        Ok(count)
    }

    pub fn list_iptv_sources(&self) -> Result<Vec<IptvSourceRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .list_iptv_sources(profile)?)
    }

    pub fn remove_iptv_source(&self, source_id: i64) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .remove_iptv_source(profile, source_id)?)
    }

    pub fn list_iptv_channels(
        &self,
        group: Option<String>,
        query: Option<String>,
    ) -> Result<Vec<IptvChannelRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .list_iptv_channels(profile, group.as_deref(), query.as_deref())?)
    }

    pub fn set_iptv_favorite(&self, channel_id: i64, favorite: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_iptv_channel_favorite(profile, channel_id, favorite)?)
    }

    pub fn iptv_epg_now_next(&self, epg_channel_id: String) -> Result<Vec<IptvEpgRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? || epg_channel_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .current_and_next_epg(&epg_channel_id, now_unix())?)
    }

    pub fn iptv_all_epg_now_next(&self) -> Result<Vec<IptvEpgRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .current_and_next_epg_all(now_unix())?)
    }

    // =========================================================================
    // 10. Discord Rich Presence (`Feature::Discord`)
    // =========================================================================

    pub async fn update_discord_presence(
        self: &Arc<Self>,
        input: DiscordPresenceInput,
    ) -> Result<(), CoreError> {
        if !self.feature_enabled(Feature::Discord)? {
            return Ok(());
        }
        let prefs = self.player_preferences().unwrap_or_default();
        if input.paused && !prefs.discord_show_when_paused {
            return self.clear_discord_presence().await;
        }
        let client = self.discord_client()?;
        let now = now_unix();
        let start_ts = (!input.paused).then(|| now - input.position_s as i64);
        let end_ts = match (input.paused, input.duration_s) {
            (false, Some(dur)) if dur > input.position_s => {
                Some(now + (dur - input.position_s) as i64)
            }
            _ => None,
        };
        let (details, state) = if prefs.discord_hide_title {
            ("Watching on Cinma".to_owned(), None)
        } else {
            (input.title, input.subtitle)
        };
        let act = PresenceActivity {
            details,
            state,
            start_ts,
            end_ts,
            paused: input.paused,
        };
        self.spawn(async move {
            let mut g = client.lock().await;
            // Best-effort: Discord desktop client may not be open.
            let _ = g.set_activity(&act).await;
            Ok(())
        })
        .await
    }

    pub async fn clear_discord_presence(self: &Arc<Self>) -> Result<(), CoreError> {
        let existing = self
            .discord_client
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
            .map(Arc::clone);
        let Some(client) = existing else {
            return Ok(());
        };
        self.spawn(async move {
            let mut g = client.lock().await;
            let _ = g.clear().await;
            Ok(())
        })
        .await
    }

    // =========================================================================
    // 11. VPN Interface Binding & Kill Switch
    // =========================================================================

    pub async fn set_vpn_torrent_binding(
        self: &Arc<Self>,
        bind_device_name: Option<String>,
        vpn_kill_switch: bool,
    ) -> Result<(), CoreError> {
        let mut policy_settings = self.disk_policy_settings()?;
        policy_settings.bind_device_name = bind_device_name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        policy_settings.vpn_kill_switch = vpn_kill_switch;
        self.set_disk_policy_settings(&policy_settings)?;

        let existing = {
            self.torrents
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .clone()
        };
        if let Some(engine) = existing {
            let tp = self.disk_policy_settings()?.torrent_policy(false);
            self.spawn(async move {
                engine.update_policy(tp).await;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }
}
