//! Subtitle & player profile settings, per-video timing offsets, Stremio +
//! OpenSubtitles aggregation, and UTF-8 / Turkish encoding normalization.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cinma_stremio::SubtitleQuery;
use cinma_subtitles::{
    OpenSubtitlesClient, PreparedSubtitle, SubtitleCandidate, SubtitleQueryParams,
    candidate_from_stremio, prepare_subtitle_file_with_options, rank_candidates,
};
use serde::{Deserialize, Serialize};

use crate::keychain::{
    get_opensubtitles_api_key, get_opensubtitles_token, set_opensubtitles_api_key,
    set_opensubtitles_token,
};
use crate::settings::Feature;
use crate::{Core, CoreError};

const KEY_SUBTITLE_SETTINGS: &str = "player.subtitles";
const KEY_PLAYER_PREFS: &str = "player.prefs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SubtitleSettings {
    pub preferred_langs: Vec<String>,
    pub audio_langs: Vec<String>,
    pub dual_secondary_lang: Option<String>,
    pub srt_font: Option<String>,
    pub srt_font_size: u32,
    pub srt_color: Option<String>,
    pub srt_border_size: u32,
    pub srt_border_color: Option<String>,
    pub srt_back_color: Option<String>,
    pub srt_bold: bool,
    pub srt_pos: u32,
    pub ass_override: bool,
    /// Primary subtitle text opacity (`10..=100`).
    pub srt_opacity: u32,
    /// Primary subtitle background box opacity (`0..=100`).
    pub srt_box_opacity: u32,
    /// Horizontal alignment: `"left"` | `"center"` | `"right"`.
    pub srt_align_x: String,
    /// Vertical position (`0..=100`) for secondary subtitle (`12` top / `88` bottom).
    pub secondary_pos: u32,
    /// Temporarily lift subtitles clear of the bottom HUD controls when visible.
    pub lift_above_controls: bool,
    /// Start playback with subtitles disabled by default.
    pub off_by_default: bool,
    /// Prefer embedded container subtitle tracks over external files when available.
    pub prefer_embedded: bool,
    /// Only auto-select forced subtitles when the active audio matches a preferred native/audio language.
    pub forced_when_native_audio: bool,
    /// Filter out SDH / hearing-impaired tracks and strip `[...]` / `(...)` cues from SRT files.
    pub hide_sdh: bool,
    /// Subtitle scale multiplier percentage (`50..=150`, `100` = 1.0x).
    pub sub_scale_pct: u32,
    /// Scale ASS/SSA subtitle font size with video window height.
    pub ass_scale_with_window: bool,
    /// Query OpenSubtitles REST API when performing online subtitle searches.
    pub enable_opensubtitles: bool,
    /// Query installed Stremio subtitle addons when performing online subtitle searches.
    pub enable_addon_subtitles: bool,
    /// Cap subtitle peak luminance in HDR/EDR mode (`0` = `"auto"`, `50..=1000` nits).
    pub sub_hdr_peak_nits: u32,
    /// Allow rendering subtitles inside letterbox black margins (`sub-use-margins` + `sub-ass-force-margins`).
    pub sub_use_margins: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            preferred_langs: vec!["tur".to_owned(), "eng".to_owned()],
            audio_langs: vec!["eng".to_owned(), "jpn".to_owned(), "tur".to_owned()],
            dual_secondary_lang: Some("eng".to_owned()),
            srt_font: None,
            srt_font_size: 46,
            srt_color: Some("#FFFFFF".to_owned()),
            srt_border_size: 3,
            srt_border_color: Some("#000000".to_owned()),
            srt_back_color: None,
            srt_bold: false,
            srt_pos: 95,
            ass_override: false,
            srt_opacity: 100,
            srt_box_opacity: 0,
            srt_align_x: "center".to_owned(),
            secondary_pos: 12,
            lift_above_controls: true,
            off_by_default: false,
            prefer_embedded: false,
            forced_when_native_audio: false,
            hide_sdh: false,
            sub_scale_pct: 100,
            ass_scale_with_window: true,
            enable_opensubtitles: true,
            enable_addon_subtitles: true,
            sub_hdr_peak_nits: 0,
            sub_use_margins: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerPreferences {
    pub exact_seek: bool,
    pub hdr_target_peak: Option<u32>,
    /// `"none"` | `"sharpen"` | custom `.glsl` file path.
    pub shader_preset: String,
    pub autoplay_next: bool,
    pub autoplay_countdown_s: u32,
    /// When `false`, SDR sources use an 8-bit BT.709 layer instead of 16-bit float EDR.
    pub force_hdr_surface: bool,
    /// Seek backward step in seconds (`3..=60`).
    pub seek_back_step_s: u32,
    /// Seek forward step in seconds (`3..=60`).
    pub seek_forward_step_s: u32,
    /// Default playback speed percentage (`50..=200`, e.g. `100` = 1.0x).
    pub default_speed_pct: u32,
    /// Maximum volume boost percentage (`100` | `150` | `200`).
    pub volume_boost_max: u32,
    /// Pause playback when the player window is minimized.
    pub pause_minimized: bool,
    /// Pause playback when the app/window loses focus.
    pub pause_unfocused: bool,
    /// Automatically skip intro segments when encountered.
    pub auto_skip_intro: bool,
    /// Automatically skip recap segments when encountered.
    pub auto_skip_recap: bool,
    /// Automatically skip outro/credits segments when encountered.
    pub auto_skip_outro: bool,
    /// Show the on-screen "Skip Intro / Recap / Outro" button.
    pub show_skip_button: bool,
    /// Automatically try the next stream candidate when playback stalls.
    pub auto_next_stream_on_stall: bool,
    /// Seconds to wait before declaring a playback stall (`5..=30`).
    pub stall_timeout_s: u32,
    /// Aspect / crop mode: `"fit"` | `"fill"` | `"16:9"` | `"21:9"` | `"4:3"`.
    pub crop_mode: String,
    /// Apply mpv `lavfi=[dynaudnorm]` audio loudness normalization.
    pub audio_normalize: bool,
    /// Downmix multichannel audio to stereo (`2.0`).
    pub downmix_stereo: bool,
    /// Blur unwatched episode thumbnails and hide episode plot descriptions.
    pub hide_spoilers: bool,
    /// Remember last selected source/addon/resolution per series.
    pub remember_last_stream: bool,
    /// Keep the same addon / bingeGroup when auto-playing the next episode.
    pub keep_source_next_episode: bool,
    /// Case-insensitive keywords/release groups blocked from the stream picker.
    pub track_block_words: Vec<String>,
    /// Stream picker ordering: `"score"` | `"seeds"` | `"size_desc"` | `"size_asc"`.
    pub stream_sort: String,
    /// Show raw filename/title line inside stream picker cards.
    pub picker_show_filename: bool,
    /// Display wall-clock time in the player HUD.
    pub fullscreen_clock: bool,
    /// Include estimated finish time alongside the player HUD clock.
    pub fullscreen_clock_end_time: bool,
    /// Seconds remaining before episode end to trigger the Up Next card (`10..=90`).
    pub next_episode_lead_s: u32,
    /// Pause and ask "Still watching?" after N auto-played episodes (`0` disables).
    pub still_watching_after_episodes: u32,
    /// Hide fully watched titles in Home/Discover catalog rows.
    pub hide_watched_in_catalogs: bool,
    /// Hide titles with a future release year from catalogs.
    pub hide_unreleased: bool,
    /// Sort series episodes ascending (`1..N`) when true, descending (`N..1`) when false.
    pub episode_sort_asc: bool,
    /// Render series episode rows in a compact text-first layout.
    pub episode_compact_layout: bool,
    /// Ask for confirmation before leaving active playback.
    pub confirm_on_leave: bool,
    /// Playback resume behavior: `"always_resume"` | `"ask"` | `"always_start_over"`.
    pub resume_behavior: String,
    /// Show transient volume percentage HUD toast when changing volume.
    pub volume_hud: bool,
    /// Show the screen-lock toggle button in the player bar.
    pub screen_lock_enabled: bool,
    /// HDR-to-SDR tone-mapping algorithm (`"auto"` | `"bt.2390"` | `"reinhard"` | `"hable"` | `"clip"`).
    pub hdr_tone_mapping: String,
    /// Enable macOS Extended Dynamic Range (EDR) 16-bit HDR framebuffer when supported.
    pub mac_edr_enabled: bool,
    /// Force 8-bit `yuv420p` output via mpv video filter.
    pub force_yuv420p: bool,
    /// mpv scaler quality preset (`"fast"` | `"balanced"` | `"high"`).
    pub mpv_scale_preset: String,
    /// mpv hardware decode mode (`"videotoolbox"` | `"auto-safe"` | `"no"`).
    pub mpv_hwdec_mode: String,
    /// mpv demuxer forward cache budget in MiB (`4..=128`).
    pub mpv_buffer_mb: u32,
    /// Extra newline- or comma-separated `key=value` mpv options applied on startup.
    pub mpv_extra_options: String,
    /// Hide uncached P2P torrents and show only direct/cached Debrid streams.
    pub debrid_only_mode: bool,
    /// HTTP timeout in seconds (`3..=60`) for Stremio addon requests.
    pub addon_timeout_s: u32,
    /// Place the Continue Watching rail above Trending rails on the Home screen.
    pub cw_at_top: bool,
    /// Maximum number of dynamic addon catalog rails shown on Home (`0..=15`).
    pub max_addon_rails: u32,
    /// Poster card scale percentage (`80..=125`).
    pub poster_scale_pct: u32,
    /// Poster corner radius in points (`4..=24`).
    pub poster_radius_pt: u32,
    /// Hide title and metadata captions underneath poster cards.
    pub hide_poster_titles: bool,
    /// Show IMDb rating badge on poster cards.
    pub show_imdb_badge: bool,
    /// Show Movie/Series type badge on poster cards.
    pub show_type_badge: bool,
    /// Show Watched checkmark badge on poster cards.
    pub show_watched_badge: bool,
    /// Show In-Library badge on poster cards.
    pub show_library_badge: bool,
    /// Draw chapter tick marks on the player seek bar.
    pub seek_bar_chapters: bool,
    /// Show series name as primary title and episode info as subtitle (false swaps them).
    pub player_title_series_first: bool,
    /// Preferred app UI language (`"system"` | `"tr"` | `"en"`).
    pub ui_language: String,
    /// Keep the player window floating above other windows.
    pub player_always_on_top: bool,
    /// Mask media title in Discord Rich Presence ("Watching on Cinma").
    pub discord_hide_title: bool,
    /// Keep showing Discord Rich Presence while playback is paused.
    pub discord_show_when_paused: bool,
    /// Broadcast "Browsing Catalogs" in Discord Rich Presence on Home.
    pub discord_show_when_browsing: bool,
    /// Start calendar week formatting on Monday (`true`) vs Sunday (`false`).
    pub week_starts_monday: bool,
    /// Hide nearly finished items (`>= 85%`) from Continue Watching.
    pub cw_hide_caught_up: bool,
    /// Custom preset duration in minutes for the Sleep Timer menu.
    pub custom_sleep_minutes: u32,
    /// Show compact live torrent speed and peer count next to the Globe button.
    pub show_p2p_speed_chip: bool,
    /// Prefer original-language movie/series titles over localized TMDB titles.
    pub prefer_original_titles: bool,
    /// Preferred language for TMDB poster/logo artwork (`"auto"` | `"en"` | `"tr"` | `"null"`).
    pub tmdb_image_lang: String,
    /// Dynamic accent color preset (`"amber"` | `"crimson"` | `"cyan"` | `"emerald"` | `"violet"`).
    pub theme_accent: String,
    /// Render solid dark surfaces instead of Liquid Glass vibrancy layers to minimize GPU work.
    pub reduce_glass_effect: bool,
    /// Background scrim dimming intensity (`20..=90`) behind hero artwork on detail pages.
    pub backdrop_dim_pct: u32,
    /// Top-navigation tabs hidden by the user (`"discover"`, `"calendar"`, `"downloads"`).
    pub hidden_nav_tabs: Vec<String>,
    /// Show button on direct HTTP/Debrid streams to open in external mpv / IINA.
    pub external_player_enabled: bool,
    /// Optional explicit path to an external player binary (`/opt/homebrew/bin/mpv`, etc.).
    pub external_player_path: String,
    /// Prompt for confirmation before installing `stremio://` addon links.
    pub confirm_stremio_deeplink: bool,
    /// Pause Trakt / Simkl scrobble state updates when playback is paused.
    pub pause_scrobble_on_pause: bool,
    /// Count rewatches when syncing progress with MyAnimeList / AniList.
    pub mal_count_rewatches: bool,
    /// Hold Space or mouse press to temporarily boost playback speed.
    pub hold_to_boost_enabled: bool,
    /// Temporary speed percentage (`150..=300`) when hold-to-boost is active.
    pub hold_boost_speed_pct: u32,
    /// mpv Video EQ brightness adjustment (`-100..=100`).
    pub eq_brightness: i32,
    /// mpv Video EQ contrast adjustment (`-100..=100`).
    pub eq_contrast: i32,
    /// mpv Video EQ gamma adjustment (`-100..=100`).
    pub eq_gamma: i32,
    /// mpv Video EQ saturation adjustment (`-100..=100`).
    pub eq_saturation: i32,
    /// Include active subtitles when capturing a player screenshot.
    pub screenshot_include_subtitles: bool,
    /// Enable Mac trackpad two-finger scroll gestures for volume & seeking.
    pub trackpad_gestures_enabled: bool,
    /// Trigger subtle MacBook Force Touch haptic feedback on player detents.
    pub haptic_feedback_enabled: bool,
    /// External player preset (`"auto"` | `"iina"` | `"infuse"` | `"vlc"` | `"custom"`).
    pub external_player_preset: String,
    /// Split Home Continue Watching into separate Movies and Series rails.
    pub split_continue_watching: bool,
    /// Automatically pause playback when CoreAudio default output switches (e.g. headphone disconnect).
    pub pause_on_audio_output_change: bool,
    /// Prompt to open/download when focusing Cinma with a magnet or media URL in clipboard.
    pub clipboard_catcher_enabled: bool,
    /// Black out secondary monitors during fullscreen playback.
    pub blackout_other_screens: bool,
    /// Automatically collapse player to floating PiP mini-player when occluded by another Space/app.
    pub auto_pip_on_occlusion: bool,
    /// Send a local macOS notification when a download finishes.
    pub notify_on_download_complete: bool,
    /// Allow single-click Quick Play of the top-ranked stream directly from cards & title headers.
    pub quick_play_enabled: bool,
}

impl Default for PlayerPreferences {
    fn default() -> Self {
        Self {
            exact_seek: true,
            hdr_target_peak: None,
            shader_preset: "none".to_owned(),
            autoplay_next: true,
            autoplay_countdown_s: 10,
            force_hdr_surface: false,
            seek_back_step_s: 10,
            seek_forward_step_s: 10,
            default_speed_pct: 100,
            volume_boost_max: 150,
            pause_minimized: true,
            pause_unfocused: false,
            auto_skip_intro: false,
            auto_skip_recap: false,
            auto_skip_outro: false,
            show_skip_button: true,
            auto_next_stream_on_stall: true,
            stall_timeout_s: 12,
            crop_mode: "fit".to_owned(),
            audio_normalize: false,
            downmix_stereo: false,
            hide_spoilers: false,
            remember_last_stream: true,
            keep_source_next_episode: true,
            track_block_words: Vec::new(),
            stream_sort: "score".to_owned(),
            picker_show_filename: true,
            fullscreen_clock: true,
            fullscreen_clock_end_time: true,
            next_episode_lead_s: 35,
            still_watching_after_episodes: 0,
            hide_watched_in_catalogs: false,
            hide_unreleased: false,
            episode_sort_asc: true,
            episode_compact_layout: false,
            confirm_on_leave: false,
            resume_behavior: "always_resume".to_owned(),
            volume_hud: true,
            screen_lock_enabled: true,
            hdr_tone_mapping: "auto".to_owned(),
            mac_edr_enabled: true,
            force_yuv420p: false,
            mpv_scale_preset: "balanced".to_owned(),
            mpv_hwdec_mode: "videotoolbox".to_owned(),
            mpv_buffer_mb: 8,
            mpv_extra_options: String::new(),
            debrid_only_mode: false,
            addon_timeout_s: 15,
            cw_at_top: true,
            max_addon_rails: 6,
            poster_scale_pct: 100,
            poster_radius_pt: 12,
            hide_poster_titles: false,
            show_imdb_badge: true,
            show_type_badge: true,
            show_watched_badge: true,
            show_library_badge: false,
            seek_bar_chapters: true,
            player_title_series_first: true,
            ui_language: "system".to_owned(),
            player_always_on_top: false,
            discord_hide_title: false,
            discord_show_when_paused: true,
            discord_show_when_browsing: false,
            week_starts_monday: true,
            cw_hide_caught_up: false,
            custom_sleep_minutes: 45,
            show_p2p_speed_chip: true,
            prefer_original_titles: false,
            tmdb_image_lang: "auto".to_owned(),
            theme_accent: "amber".to_owned(),
            reduce_glass_effect: false,
            backdrop_dim_pct: 50,
            hidden_nav_tabs: Vec::new(),
            external_player_enabled: false,
            external_player_path: String::new(),
            confirm_stremio_deeplink: true,
            pause_scrobble_on_pause: true,
            mal_count_rewatches: false,
            hold_to_boost_enabled: true,
            hold_boost_speed_pct: 200,
            eq_brightness: 0,
            eq_contrast: 0,
            eq_gamma: 0,
            eq_saturation: 0,
            screenshot_include_subtitles: true,
            trackpad_gestures_enabled: true,
            haptic_feedback_enabled: true,
            external_player_preset: "auto".to_owned(),
            split_continue_watching: false,
            pause_on_audio_output_change: true,
            clipboard_catcher_enabled: false,
            blackout_other_screens: false,
            auto_pip_on_occlusion: false,
            notify_on_download_complete: true,
            quick_play_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PreferredSeriesSource {
    pub addon_name: Option<String>,
    pub binge_group: Option<String>,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoOffsets {
    pub sub_delay: f64,
    pub audio_delay: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleSearchRequest {
    pub media_type: String,
    pub video_id: String,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
}

const CAS_SHARPEN_SHADER: &str = r#"//!HOOK MAIN
//!BIND HOOKED
//!DESC Cinma Adaptive Luma Sharpen (CAS)

vec4 hook() {
    vec2 px = HOOKED_pt;
    vec3 c = HOOKED_tex(HOOKED_pos).rgb;
    vec3 n = HOOKED_tex(HOOKED_pos + vec2(0.0, -px.y)).rgb;
    vec3 s = HOOKED_tex(HOOKED_pos + vec2(0.0,  px.y)).rgb;
    vec3 e = HOOKED_tex(HOOKED_pos + vec2( px.x, 0.0)).rgb;
    vec3 w = HOOKED_tex(HOOKED_pos + vec2(-px.x, 0.0)).rgb;
    vec3 blur = (n + s + e + w) * 0.25;
    vec3 sharp = clamp(c + (c - blur) * 0.35, 0.0, 1.0);
    return vec4(sharp, HOOKED_tex(HOOKED_pos).a);
}
"#;

impl Core {
    pub fn subtitle_settings(&self) -> Result<SubtitleSettings, CoreError> {
        self.load_profile_json(KEY_SUBTITLE_SETTINGS)
    }

    pub fn set_subtitle_settings(&self, settings: &SubtitleSettings) -> Result<(), CoreError> {
        self.save_profile_json(KEY_SUBTITLE_SETTINGS, settings)
    }

    pub fn player_preferences(&self) -> Result<PlayerPreferences, CoreError> {
        self.load_profile_json(KEY_PLAYER_PREFS)
    }

    pub fn set_player_preferences(&self, prefs: &PlayerPreferences) -> Result<(), CoreError> {
        self.save_profile_json(KEY_PLAYER_PREFS, prefs)?;
        if let Ok(mut slot) = self.http.lock() {
            *slot = None;
        }
        Ok(())
    }

    pub fn video_offsets(&self, video_id: &str) -> Result<VideoOffsets, CoreError> {
        self.load_profile_json(&format!("offset.{video_id}"))
    }

    pub fn set_video_offsets(
        &self,
        video_id: &str,
        offsets: &VideoOffsets,
    ) -> Result<(), CoreError> {
        self.save_profile_json(&format!("offset.{video_id}"), offsets)
    }

    pub fn series_source(&self, meta_id: &str) -> Result<PreferredSeriesSource, CoreError> {
        self.load_profile_json(&format!("series_source.{meta_id}"))
    }

    pub fn set_series_source(
        &self,
        meta_id: &str,
        source: &PreferredSeriesSource,
    ) -> Result<(), CoreError> {
        self.save_profile_json(&format!("series_source.{meta_id}"), source)
    }

    pub fn clear_series_source(&self, meta_id: &str) -> Result<(), CoreError> {
        self.save_profile_json(
            &format!("series_source.{meta_id}"),
            &PreferredSeriesSource::default(),
        )
    }

    pub fn search_subtitle_cues(
        &self,
        path: &Path,
        query: &str,
        limit: usize,
    ) -> Result<Vec<cinma_subtitles::SubtitleCue>, CoreError> {
        cinma_subtitles::search_subtitle_cues(path, query, limit)
            .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    pub(crate) fn load_profile_json<T: serde::de::DeserializeOwned + Default>(
        &self,
        key: &str,
    ) -> Result<T, CoreError> {
        let profile = self.active_profile()?;
        let raw = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_profile_setting(profile, key)?;
        match raw {
            Some(s) => serde_json::from_str(&s).map_err(|e| CoreError::Invalid(e.to_string())),
            None => Ok(T::default()),
        }
    }

    pub(crate) fn save_profile_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), CoreError> {
        let json = serde_json::to_string(value).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_profile_setting(profile, key, &json)?;
        Ok(())
    }

    pub fn has_opensubtitles_key(&self) -> Result<bool, CoreError> {
        Ok(get_opensubtitles_api_key()?.is_some())
    }

    pub fn set_opensubtitles_credentials(
        &self,
        api_key: Option<&str>,
        token: Option<&str>,
    ) -> Result<(), CoreError> {
        set_opensubtitles_api_key(api_key)?;
        if token.is_some() || api_key.is_none() {
            set_opensubtitles_token(token)?;
        }
        Ok(())
    }

    /// Resolves GLSL shader file paths for a preset (`"none"` | `"sharpen"` | path).
    pub fn resolve_shader_paths(&self, preset: &str) -> Result<Vec<String>, CoreError> {
        match preset.trim() {
            "" | "none" => Ok(Vec::new()),
            "sharpen" => {
                let dir = self.data_dir.join("shaders");
                std::fs::create_dir_all(&dir)?;
                let path = dir.join("cas.glsl");
                if !path.exists() {
                    std::fs::write(&path, CAS_SHARPEN_SHADER)?;
                }
                Ok(vec![path.to_string_lossy().into_owned()])
            }
            custom => {
                let p = Path::new(custom);
                if p.exists() {
                    Ok(vec![p.to_string_lossy().into_owned()])
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    /// Queries enabled Stremio `subtitles` addons + optional direct OpenSubtitles
    /// API (when configured in Keychain), ranked by preferred languages & hash match.
    pub async fn fetch_subtitles(
        self: &Arc<Self>,
        req: SubtitleSearchRequest,
    ) -> Result<Vec<SubtitleCandidate>, CoreError> {
        let settings = self.subtitle_settings()?;
        let had_hash = req.video_hash.as_deref().is_some_and(|h| !h.is_empty());
        let stremio_query = SubtitleQuery {
            video_hash: req.video_hash.clone(),
            video_size: req.video_size,
            filename: req.filename.clone(),
        };

        let mut handles = Vec::new();
        if settings.enable_addon_subtitles && self.feature_enabled(Feature::Addons)? {
            let http = self.http()?;
            let targets: Vec<_> = self
                .addons()?
                .into_iter()
                .filter(|a| {
                    a.enabled
                        && a.manifest
                            .supports("subtitles", &req.media_type, &req.video_id)
                })
                .collect();
            for addon in targets {
                let client = http.clone();
                let mt = req.media_type.clone();
                let id = req.video_id.clone();
                let q = stremio_query.clone();
                handles.push(self.runtime()?.spawn(async move {
                    let subs = client
                        .fetch_subtitles(&addon.transport_url, &mt, &id, &q)
                        .await
                        .unwrap_or_default();
                    subs.into_iter()
                        .map(|s| candidate_from_stremio(&addon.manifest.name, s, had_hash))
                        .collect::<Vec<_>>()
                }));
            }
        }

        if settings.enable_opensubtitles
            && let Some(os_key) = get_opensubtitles_api_key()?
        {
            let os_token = get_opensubtitles_token()?;
            let params = SubtitleQueryParams {
                imdb_id: req.imdb_id,
                season: req.season,
                episode: req.episode,
                video_hash: req.video_hash,
                video_size: req.video_size,
                filename: req.filename,
                languages: settings.preferred_langs.clone(),
            };
            handles.push(self.runtime()?.spawn(async move {
                let Ok(client) = OpenSubtitlesClient::new(os_key, os_token) else {
                    return Vec::new();
                };
                client.search(&params).await.unwrap_or_default()
            }));
        }

        let mut all = Vec::new();
        for handle in handles {
            if let Ok(mut batch) = handle.await {
                all.append(&mut batch);
            }
        }
        if settings.hide_sdh {
            all.retain(|c| !c.is_sdh);
        }
        rank_candidates(&mut all, &settings.preferred_langs);
        Ok(all)
    }

    /// Downloads a remote subtitle candidate (Stremio URL or OpenSubtitles `file_id`),
    /// unpacks gzip if compressed, detects/converts Turkish legacy encodings to
    /// UTF-8 while keeping ASS styles intact, and caches under `data_dir/subtitles`.
    pub async fn download_and_prepare_subtitle(
        self: &Arc<Self>,
        candidate: SubtitleCandidate,
    ) -> Result<PreparedSubtitle, CoreError> {
        let cache_dir = self.subtitles_cache_dir();
        let bytes_and_ext: (Vec<u8>, Option<String>) =
            if let Some(file_id) = candidate.opensubtitles_file_id {
                let api_key = get_opensubtitles_api_key()?.ok_or_else(|| {
                    CoreError::Invalid("OpenSubtitles API key missing in Keychain".into())
                })?;
                let token = get_opensubtitles_token()?;
                self.spawn(async move {
                    let client = OpenSubtitlesClient::new(api_key, token)
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    let (bytes, ext) = client
                        .download_file(file_id)
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    Ok((bytes, Some(ext)))
                })
                .await?
            } else {
                let url = candidate.download_url.clone();
                let ext = url
                    .split('?')
                    .next()
                    .and_then(|p| p.rsplit_once('.'))
                    .map(|(_, e)| e.to_owned())
                    .filter(|e| !e.eq_ignore_ascii_case("gz"));
                self.spawn(async move {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(20))
                        .build()
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    let resp = client
                        .get(&url)
                        .send()
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    if !resp.status().is_success() {
                        return Err(CoreError::Invalid(format!(
                            "subtitle HTTP {}",
                            resp.status()
                        )));
                    }
                    let data = resp
                        .bytes()
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?
                        .to_vec();
                    Ok((data, ext))
                })
                .await?
            };

        let strip_sdh = self
            .subtitle_settings()
            .map(|s| s.hide_sdh)
            .unwrap_or(false);
        prepare_subtitle_file_with_options(
            &bytes_and_ext.0,
            &cache_dir,
            &format!("{}_{}", candidate.lang, candidate.id),
            bytes_and_ext.1.as_deref(),
            candidate.encoding_hint.as_deref(),
            Some(&candidate.lang),
            strip_sdh,
        )
        .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    /// Normalizes a user-picked local subtitle file (converting CP1254/ISO-8859-9
    /// `.srt` to UTF-8 if needed while leaving `.ass` styling untouched).
    pub fn prepare_local_subtitle(&self, path: &Path) -> Result<PreparedSubtitle, CoreError> {
        let bytes = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("local");
        let ext = path.extension().and_then(|s| s.to_str());
        let strip_sdh = self
            .subtitle_settings()
            .map(|s| s.hide_sdh)
            .unwrap_or(false);
        prepare_subtitle_file_with_options(
            &bytes,
            &self.subtitles_cache_dir(),
            stem,
            ext,
            None,
            None,
            strip_sdh,
        )
        .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    fn subtitles_cache_dir(&self) -> PathBuf {
        self.data_dir.join("subtitles")
    }
}
