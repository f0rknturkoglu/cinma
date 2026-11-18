use std::sync::Arc;

use cinma_core::{EndReason, PlayerEvent, PlayerEvents};

use crate::StreamInfo;

#[derive(uniffi::Enum)]
pub enum PlaybackEndReason {
    Eof,
    Stopped,
    Error,
}

#[derive(uniffi::Enum)]
pub enum PlaybackEvent {
    FileLoaded,
    PlaybackRestart,
    Position {
        seconds: f64,
    },
    Duration {
        seconds: f64,
    },
    Paused {
        paused: bool,
    },
    Buffering {
        buffering: bool,
    },
    VideoGamma {
        gamma: String,
    },
    Metric {
        name: String,
        millis: f64,
    },
    TracksChanged,
    ChaptersChanged,
    EndFile {
        reason: PlaybackEndReason,
        error: Option<String>,
    },
    Shutdown,
}

#[derive(uniffi::Record)]
pub struct MediaTrackInfo {
    pub id: i64,
    pub track_type: String,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    pub external: bool,
    pub forced: bool,
    pub hearing_impaired: bool,
}

impl From<cinma_core::MediaTrack> for MediaTrackInfo {
    fn from(t: cinma_core::MediaTrack) -> Self {
        Self {
            id: t.id,
            track_type: t.track_type,
            title: t.title,
            lang: t.lang,
            codec: t.codec,
            selected: t.selected,
            external: t.external,
            forced: t.forced,
            hearing_impaired: t.hearing_impaired,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ChapterInfo {
    pub title: String,
    pub start_s: f64,
}

impl From<cinma_core::Chapter> for ChapterInfo {
    fn from(c: cinma_core::Chapter) -> Self {
        Self {
            title: c.title,
            start_s: c.start_s,
        }
    }
}

#[derive(uniffi::Record)]
pub struct StreamStatsInfo {
    pub is_torrent: bool,
    pub info_hash: Option<String>,
    pub filename: Option<String>,
    pub source_url: Option<String>,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub peers_connected: u32,
    pub peers_queued: u32,
    pub peers_seen: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
}

impl From<cinma_core::StreamStats> for StreamStatsInfo {
    fn from(s: cinma_core::StreamStats) -> Self {
        Self {
            is_torrent: s.is_torrent,
            info_hash: s.info_hash,
            filename: s.filename,
            source_url: s.source_url,
            download_speed_bps: s.download_speed_bps,
            upload_speed_bps: s.upload_speed_bps,
            peers_connected: s.peers_connected,
            peers_queued: s.peers_queued,
            peers_seen: s.peers_seen,
            downloaded_bytes: s.downloaded_bytes,
            total_bytes: s.total_bytes,
            finished: s.finished,
        }
    }
}

#[derive(uniffi::Record)]
pub struct PlayerStatsInfo {
    pub video_codec: String,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub estimated_fps: f64,
    pub hwdec: String,
    pub gamma: String,
    pub audio_codec: String,
    pub audio_channels: i64,
    pub cache_seconds: f64,
    pub dropped_frames: i64,
    pub ab_loop_a: Option<f64>,
    pub ab_loop_b: Option<f64>,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    pub sub_delay: f64,
    pub audio_delay: f64,
}

impl From<cinma_core::PlayerStats> for PlayerStatsInfo {
    fn from(s: cinma_core::PlayerStats) -> Self {
        Self {
            video_codec: s.video_codec,
            width: s.width,
            height: s.height,
            fps: s.fps,
            estimated_fps: s.estimated_fps,
            hwdec: s.hwdec,
            gamma: s.gamma,
            audio_codec: s.audio_codec,
            audio_channels: s.audio_channels,
            cache_seconds: s.cache_seconds,
            dropped_frames: s.dropped_frames,
            ab_loop_a: s.ab_loop_a,
            ab_loop_b: s.ab_loop_b,
            speed: s.speed,
            volume: s.volume,
            muted: s.muted,
            sub_delay: s.sub_delay,
            audio_delay: s.audio_delay,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ThumbnailImageInfo {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(uniffi::Record)]
pub struct VideoFileIdentityInfo {
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub filename: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleSettingsFfi {
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
    pub srt_opacity: u32,
    pub srt_box_opacity: u32,
    pub srt_align_x: String,
    pub secondary_pos: u32,
    pub lift_above_controls: bool,
    pub off_by_default: bool,
    pub prefer_embedded: bool,
    pub forced_when_native_audio: bool,
    pub hide_sdh: bool,
    pub sub_scale_pct: u32,
    pub ass_scale_with_window: bool,
    pub enable_opensubtitles: bool,
    pub enable_addon_subtitles: bool,
    pub sub_hdr_peak_nits: u32,
    pub sub_use_margins: bool,
}

impl From<cinma_core::SubtitleSettings> for SubtitleSettingsFfi {
    fn from(s: cinma_core::SubtitleSettings) -> Self {
        Self {
            preferred_langs: s.preferred_langs,
            audio_langs: s.audio_langs,
            dual_secondary_lang: s.dual_secondary_lang,
            srt_font: s.srt_font,
            srt_font_size: s.srt_font_size,
            srt_color: s.srt_color,
            srt_border_size: s.srt_border_size,
            srt_border_color: s.srt_border_color,
            srt_back_color: s.srt_back_color,
            srt_bold: s.srt_bold,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
            srt_opacity: s.srt_opacity,
            srt_box_opacity: s.srt_box_opacity,
            srt_align_x: s.srt_align_x,
            secondary_pos: s.secondary_pos,
            lift_above_controls: s.lift_above_controls,
            off_by_default: s.off_by_default,
            prefer_embedded: s.prefer_embedded,
            forced_when_native_audio: s.forced_when_native_audio,
            hide_sdh: s.hide_sdh,
            sub_scale_pct: s.sub_scale_pct,
            ass_scale_with_window: s.ass_scale_with_window,
            enable_opensubtitles: s.enable_opensubtitles,
            enable_addon_subtitles: s.enable_addon_subtitles,
            sub_hdr_peak_nits: s.sub_hdr_peak_nits,
            sub_use_margins: s.sub_use_margins,
        }
    }
}

impl From<SubtitleSettingsFfi> for cinma_core::SubtitleSettings {
    fn from(s: SubtitleSettingsFfi) -> Self {
        Self {
            preferred_langs: s.preferred_langs,
            audio_langs: s.audio_langs,
            dual_secondary_lang: s.dual_secondary_lang,
            srt_font: s.srt_font,
            srt_font_size: s.srt_font_size,
            srt_color: s.srt_color,
            srt_border_size: s.srt_border_size,
            srt_border_color: s.srt_border_color,
            srt_back_color: s.srt_back_color,
            srt_bold: s.srt_bold,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
            srt_opacity: s.srt_opacity,
            srt_box_opacity: s.srt_box_opacity,
            srt_align_x: s.srt_align_x,
            secondary_pos: s.secondary_pos,
            lift_above_controls: s.lift_above_controls,
            off_by_default: s.off_by_default,
            prefer_embedded: s.prefer_embedded,
            forced_when_native_audio: s.forced_when_native_audio,
            hide_sdh: s.hide_sdh,
            sub_scale_pct: s.sub_scale_pct,
            ass_scale_with_window: s.ass_scale_with_window,
            enable_opensubtitles: s.enable_opensubtitles,
            enable_addon_subtitles: s.enable_addon_subtitles,
            sub_hdr_peak_nits: s.sub_hdr_peak_nits,
            sub_use_margins: s.sub_use_margins,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct PlayerPreferencesFfi {
    pub exact_seek: bool,
    pub hdr_target_peak: Option<u32>,
    pub shader_preset: String,
    pub autoplay_next: bool,
    pub autoplay_countdown_s: u32,
    pub force_hdr_surface: bool,
    pub seek_back_step_s: u32,
    pub seek_forward_step_s: u32,
    pub default_speed_pct: u32,
    pub volume_boost_max: u32,
    pub pause_minimized: bool,
    pub pause_unfocused: bool,
    pub auto_skip_intro: bool,
    pub auto_skip_recap: bool,
    pub auto_skip_outro: bool,
    pub show_skip_button: bool,
    pub auto_next_stream_on_stall: bool,
    pub stall_timeout_s: u32,
    pub crop_mode: String,
    pub audio_normalize: bool,
    pub downmix_stereo: bool,
    pub hide_spoilers: bool,
    pub remember_last_stream: bool,
    pub keep_source_next_episode: bool,
    pub track_block_words: Vec<String>,
    pub stream_sort: String,
    pub picker_show_filename: bool,
    pub fullscreen_clock: bool,
    pub fullscreen_clock_end_time: bool,
    pub next_episode_lead_s: u32,
    pub still_watching_after_episodes: u32,
    pub hide_watched_in_catalogs: bool,
    pub hide_unreleased: bool,
    pub episode_sort_asc: bool,
    pub episode_compact_layout: bool,
    pub confirm_on_leave: bool,
    pub resume_behavior: String,
    pub volume_hud: bool,
    pub screen_lock_enabled: bool,
    pub hdr_tone_mapping: String,
    pub mac_edr_enabled: bool,
    pub force_yuv420p: bool,
    pub mpv_scale_preset: String,
    pub mpv_hwdec_mode: String,
    pub mpv_buffer_mb: u32,
    pub mpv_extra_options: String,
    pub debrid_only_mode: bool,
    pub addon_timeout_s: u32,
    pub cw_at_top: bool,
    pub max_addon_rails: u32,
    pub poster_scale_pct: u32,
    pub poster_radius_pt: u32,
    pub hide_poster_titles: bool,
    pub show_imdb_badge: bool,
    pub show_type_badge: bool,
    pub show_watched_badge: bool,
    pub show_library_badge: bool,
    pub seek_bar_chapters: bool,
    pub player_title_series_first: bool,
    pub ui_language: String,
    pub player_always_on_top: bool,
    pub discord_hide_title: bool,
    pub discord_show_when_paused: bool,
    pub discord_show_when_browsing: bool,
    pub week_starts_monday: bool,
    pub cw_hide_caught_up: bool,
    pub custom_sleep_minutes: u32,
    pub show_p2p_speed_chip: bool,
    pub prefer_original_titles: bool,
    pub tmdb_image_lang: String,
    pub theme_accent: String,
    pub reduce_glass_effect: bool,
    pub backdrop_dim_pct: u32,
    pub hidden_nav_tabs: Vec<String>,
    pub external_player_enabled: bool,
    pub external_player_path: String,
    pub confirm_stremio_deeplink: bool,
    pub mal_count_rewatches: bool,
    pub hold_to_boost_enabled: bool,
    pub hold_boost_speed_pct: u32,
    pub eq_brightness: i32,
    pub eq_contrast: i32,
    pub eq_gamma: i32,
    pub eq_saturation: i32,
    pub screenshot_include_subtitles: bool,
    pub trackpad_gestures_enabled: bool,
    pub haptic_feedback_enabled: bool,
    pub external_player_preset: String,
    pub split_continue_watching: bool,
    pub pause_on_audio_output_change: bool,
    pub clipboard_catcher_enabled: bool,
    pub blackout_other_screens: bool,
    pub auto_pip_on_occlusion: bool,
    pub notify_on_download_complete: bool,
    pub quick_play_enabled: bool,
    pub auto_skip_anime_fillers: bool,
    pub local_media_folders: Vec<String>,
    pub download_schedule_enabled: bool,
    pub download_schedule_start_hour: u8,
    pub download_schedule_end_hour: u8,
    pub local_api_socket_enabled: bool,
    pub gamepad_support_enabled: bool,
}

impl From<cinma_core::PlayerPreferences> for PlayerPreferencesFfi {
    fn from(p: cinma_core::PlayerPreferences) -> Self {
        Self {
            exact_seek: p.exact_seek,
            hdr_target_peak: p.hdr_target_peak,
            shader_preset: p.shader_preset,
            autoplay_next: p.autoplay_next,
            autoplay_countdown_s: p.autoplay_countdown_s,
            force_hdr_surface: p.force_hdr_surface,
            seek_back_step_s: p.seek_back_step_s,
            seek_forward_step_s: p.seek_forward_step_s,
            default_speed_pct: p.default_speed_pct,
            volume_boost_max: p.volume_boost_max,
            pause_minimized: p.pause_minimized,
            pause_unfocused: p.pause_unfocused,
            auto_skip_intro: p.auto_skip_intro,
            auto_skip_recap: p.auto_skip_recap,
            auto_skip_outro: p.auto_skip_outro,
            show_skip_button: p.show_skip_button,
            auto_next_stream_on_stall: p.auto_next_stream_on_stall,
            stall_timeout_s: p.stall_timeout_s,
            crop_mode: p.crop_mode,
            audio_normalize: p.audio_normalize,
            downmix_stereo: p.downmix_stereo,
            hide_spoilers: p.hide_spoilers,
            remember_last_stream: p.remember_last_stream,
            keep_source_next_episode: p.keep_source_next_episode,
            track_block_words: p.track_block_words,
            stream_sort: p.stream_sort,
            picker_show_filename: p.picker_show_filename,
            fullscreen_clock: p.fullscreen_clock,
            fullscreen_clock_end_time: p.fullscreen_clock_end_time,
            next_episode_lead_s: p.next_episode_lead_s,
            still_watching_after_episodes: p.still_watching_after_episodes,
            hide_watched_in_catalogs: p.hide_watched_in_catalogs,
            hide_unreleased: p.hide_unreleased,
            episode_sort_asc: p.episode_sort_asc,
            episode_compact_layout: p.episode_compact_layout,
            confirm_on_leave: p.confirm_on_leave,
            resume_behavior: p.resume_behavior,
            volume_hud: p.volume_hud,
            screen_lock_enabled: p.screen_lock_enabled,
            hdr_tone_mapping: p.hdr_tone_mapping,
            mac_edr_enabled: p.mac_edr_enabled,
            force_yuv420p: p.force_yuv420p,
            mpv_scale_preset: p.mpv_scale_preset,
            mpv_hwdec_mode: p.mpv_hwdec_mode,
            mpv_buffer_mb: p.mpv_buffer_mb,
            mpv_extra_options: p.mpv_extra_options,
            debrid_only_mode: p.debrid_only_mode,
            addon_timeout_s: p.addon_timeout_s,
            cw_at_top: p.cw_at_top,
            max_addon_rails: p.max_addon_rails,
            poster_scale_pct: p.poster_scale_pct,
            poster_radius_pt: p.poster_radius_pt,
            hide_poster_titles: p.hide_poster_titles,
            show_imdb_badge: p.show_imdb_badge,
            show_type_badge: p.show_type_badge,
            show_watched_badge: p.show_watched_badge,
            show_library_badge: p.show_library_badge,
            seek_bar_chapters: p.seek_bar_chapters,
            player_title_series_first: p.player_title_series_first,
            ui_language: p.ui_language,
            player_always_on_top: p.player_always_on_top,
            discord_hide_title: p.discord_hide_title,
            discord_show_when_paused: p.discord_show_when_paused,
            discord_show_when_browsing: p.discord_show_when_browsing,
            week_starts_monday: p.week_starts_monday,
            cw_hide_caught_up: p.cw_hide_caught_up,
            custom_sleep_minutes: p.custom_sleep_minutes,
            show_p2p_speed_chip: p.show_p2p_speed_chip,
            prefer_original_titles: p.prefer_original_titles,
            tmdb_image_lang: p.tmdb_image_lang,
            theme_accent: p.theme_accent,
            reduce_glass_effect: p.reduce_glass_effect,
            backdrop_dim_pct: p.backdrop_dim_pct,
            hidden_nav_tabs: p.hidden_nav_tabs,
            external_player_enabled: p.external_player_enabled,
            external_player_path: p.external_player_path,
            confirm_stremio_deeplink: p.confirm_stremio_deeplink,
            mal_count_rewatches: p.mal_count_rewatches,
            hold_to_boost_enabled: p.hold_to_boost_enabled,
            hold_boost_speed_pct: p.hold_boost_speed_pct,
            eq_brightness: p.eq_brightness,
            eq_contrast: p.eq_contrast,
            eq_gamma: p.eq_gamma,
            eq_saturation: p.eq_saturation,
            screenshot_include_subtitles: p.screenshot_include_subtitles,
            trackpad_gestures_enabled: p.trackpad_gestures_enabled,
            haptic_feedback_enabled: p.haptic_feedback_enabled,
            external_player_preset: p.external_player_preset,
            split_continue_watching: p.split_continue_watching,
            pause_on_audio_output_change: p.pause_on_audio_output_change,
            clipboard_catcher_enabled: p.clipboard_catcher_enabled,
            blackout_other_screens: p.blackout_other_screens,
            auto_pip_on_occlusion: p.auto_pip_on_occlusion,
            notify_on_download_complete: p.notify_on_download_complete,
            quick_play_enabled: p.quick_play_enabled,
            auto_skip_anime_fillers: p.auto_skip_anime_fillers,
            local_media_folders: p.local_media_folders,
            download_schedule_enabled: p.download_schedule_enabled,
            download_schedule_start_hour: p.download_schedule_start_hour,
            download_schedule_end_hour: p.download_schedule_end_hour,
            local_api_socket_enabled: p.local_api_socket_enabled,
            gamepad_support_enabled: p.gamepad_support_enabled,
        }
    }
}

impl From<PlayerPreferencesFfi> for cinma_core::PlayerPreferences {
    fn from(p: PlayerPreferencesFfi) -> Self {
        Self {
            exact_seek: p.exact_seek,
            hdr_target_peak: p.hdr_target_peak,
            shader_preset: p.shader_preset,
            autoplay_next: p.autoplay_next,
            autoplay_countdown_s: p.autoplay_countdown_s,
            force_hdr_surface: p.force_hdr_surface,
            seek_back_step_s: p.seek_back_step_s,
            seek_forward_step_s: p.seek_forward_step_s,
            default_speed_pct: p.default_speed_pct,
            volume_boost_max: p.volume_boost_max,
            pause_minimized: p.pause_minimized,
            pause_unfocused: p.pause_unfocused,
            auto_skip_intro: p.auto_skip_intro,
            auto_skip_recap: p.auto_skip_recap,
            auto_skip_outro: p.auto_skip_outro,
            show_skip_button: p.show_skip_button,
            auto_next_stream_on_stall: p.auto_next_stream_on_stall,
            stall_timeout_s: p.stall_timeout_s,
            crop_mode: p.crop_mode,
            audio_normalize: p.audio_normalize,
            downmix_stereo: p.downmix_stereo,
            hide_spoilers: p.hide_spoilers,
            remember_last_stream: p.remember_last_stream,
            keep_source_next_episode: p.keep_source_next_episode,
            track_block_words: p.track_block_words,
            stream_sort: p.stream_sort,
            picker_show_filename: p.picker_show_filename,
            fullscreen_clock: p.fullscreen_clock,
            fullscreen_clock_end_time: p.fullscreen_clock_end_time,
            next_episode_lead_s: p.next_episode_lead_s,
            still_watching_after_episodes: p.still_watching_after_episodes,
            hide_watched_in_catalogs: p.hide_watched_in_catalogs,
            hide_unreleased: p.hide_unreleased,
            episode_sort_asc: p.episode_sort_asc,
            episode_compact_layout: p.episode_compact_layout,
            confirm_on_leave: p.confirm_on_leave,
            resume_behavior: p.resume_behavior,
            volume_hud: p.volume_hud,
            screen_lock_enabled: p.screen_lock_enabled,
            hdr_tone_mapping: p.hdr_tone_mapping,
            mac_edr_enabled: p.mac_edr_enabled,
            force_yuv420p: p.force_yuv420p,
            mpv_scale_preset: p.mpv_scale_preset,
            mpv_hwdec_mode: p.mpv_hwdec_mode,
            mpv_buffer_mb: p.mpv_buffer_mb,
            mpv_extra_options: p.mpv_extra_options,
            debrid_only_mode: p.debrid_only_mode,
            addon_timeout_s: p.addon_timeout_s,
            cw_at_top: p.cw_at_top,
            max_addon_rails: p.max_addon_rails,
            poster_scale_pct: p.poster_scale_pct,
            poster_radius_pt: p.poster_radius_pt,
            hide_poster_titles: p.hide_poster_titles,
            show_imdb_badge: p.show_imdb_badge,
            show_type_badge: p.show_type_badge,
            show_watched_badge: p.show_watched_badge,
            show_library_badge: p.show_library_badge,
            seek_bar_chapters: p.seek_bar_chapters,
            player_title_series_first: p.player_title_series_first,
            ui_language: p.ui_language,
            player_always_on_top: p.player_always_on_top,
            discord_hide_title: p.discord_hide_title,
            discord_show_when_paused: p.discord_show_when_paused,
            discord_show_when_browsing: p.discord_show_when_browsing,
            week_starts_monday: p.week_starts_monday,
            cw_hide_caught_up: p.cw_hide_caught_up,
            custom_sleep_minutes: p.custom_sleep_minutes,
            show_p2p_speed_chip: p.show_p2p_speed_chip,
            prefer_original_titles: p.prefer_original_titles,
            tmdb_image_lang: p.tmdb_image_lang,
            theme_accent: p.theme_accent,
            reduce_glass_effect: p.reduce_glass_effect,
            backdrop_dim_pct: p.backdrop_dim_pct,
            hidden_nav_tabs: p.hidden_nav_tabs,
            external_player_enabled: p.external_player_enabled,
            external_player_path: p.external_player_path,
            confirm_stremio_deeplink: p.confirm_stremio_deeplink,
            mal_count_rewatches: p.mal_count_rewatches,
            hold_to_boost_enabled: p.hold_to_boost_enabled,
            hold_boost_speed_pct: p.hold_boost_speed_pct,
            eq_brightness: p.eq_brightness,
            eq_contrast: p.eq_contrast,
            eq_gamma: p.eq_gamma,
            eq_saturation: p.eq_saturation,
            screenshot_include_subtitles: p.screenshot_include_subtitles,
            trackpad_gestures_enabled: p.trackpad_gestures_enabled,
            haptic_feedback_enabled: p.haptic_feedback_enabled,
            external_player_preset: p.external_player_preset,
            split_continue_watching: p.split_continue_watching,
            pause_on_audio_output_change: p.pause_on_audio_output_change,
            clipboard_catcher_enabled: p.clipboard_catcher_enabled,
            blackout_other_screens: p.blackout_other_screens,
            auto_pip_on_occlusion: p.auto_pip_on_occlusion,
            notify_on_download_complete: p.notify_on_download_complete,
            quick_play_enabled: p.quick_play_enabled,
            auto_skip_anime_fillers: p.auto_skip_anime_fillers,
            local_media_folders: p.local_media_folders,
            download_schedule_enabled: p.download_schedule_enabled,
            download_schedule_start_hour: p.download_schedule_start_hour,
            download_schedule_end_hour: p.download_schedule_end_hour,
            local_api_socket_enabled: p.local_api_socket_enabled,
            gamepad_support_enabled: p.gamepad_support_enabled,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct PreferredSeriesSourceFfi {
    pub addon_name: Option<String>,
    pub binge_group: Option<String>,
    pub resolution: Option<String>,
}

impl From<cinma_core::PreferredSeriesSource> for PreferredSeriesSourceFfi {
    fn from(s: cinma_core::PreferredSeriesSource) -> Self {
        Self {
            addon_name: s.addon_name,
            binge_group: s.binge_group,
            resolution: s.resolution,
        }
    }
}

impl From<PreferredSeriesSourceFfi> for cinma_core::PreferredSeriesSource {
    fn from(s: PreferredSeriesSourceFfi) -> Self {
        Self {
            addon_name: s.addon_name,
            binge_group: s.binge_group,
            resolution: s.resolution,
        }
    }
}

#[derive(uniffi::Record)]
pub struct VideoOffsetsFfi {
    pub sub_delay: f64,
    pub audio_delay: f64,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleSearchInput {
    pub media_type: String,
    pub video_id: String,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleCandidateInfo {
    pub id: String,
    pub provider: String,
    pub lang: String,
    pub title: Option<String>,
    pub download_url: String,
    pub opensubtitles_file_id: Option<i64>,
    pub encoding_hint: Option<String>,
    pub is_sdh: bool,
    pub is_forced: bool,
    pub hash_matched: bool,
}

impl From<cinma_core::SubtitleCandidate> for SubtitleCandidateInfo {
    fn from(c: cinma_core::SubtitleCandidate) -> Self {
        Self {
            id: c.id,
            provider: c.provider,
            lang: c.lang,
            title: c.title,
            download_url: c.download_url,
            opensubtitles_file_id: c.opensubtitles_file_id,
            encoding_hint: c.encoding_hint,
            is_sdh: c.is_sdh,
            is_forced: c.is_forced,
            hash_matched: c.hash_matched,
        }
    }
}

impl From<SubtitleCandidateInfo> for cinma_core::SubtitleCandidate {
    fn from(c: SubtitleCandidateInfo) -> Self {
        Self {
            id: c.id,
            provider: c.provider,
            lang: c.lang,
            title: c.title,
            download_url: c.download_url,
            opensubtitles_file_id: c.opensubtitles_file_id,
            encoding_hint: c.encoding_hint,
            is_sdh: c.is_sdh,
            is_forced: c.is_forced,
            hash_matched: c.hash_matched,
        }
    }
}

#[derive(uniffi::Record)]
pub struct PreparedSubtitleInfo {
    pub local_path: String,
    pub format: String,
    pub detected_encoding: String,
    pub restyled_ass: bool,
}

#[derive(uniffi::Record)]
pub struct NextEpisodePrefetchInfo {
    pub video_id: String,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub still: Option<String>,
    pub best_stream: Option<StreamInfo>,
}

impl From<PlayerEvent> for PlaybackEvent {
    fn from(e: PlayerEvent) -> Self {
        match e {
            PlayerEvent::FileLoaded => Self::FileLoaded,
            PlayerEvent::PlaybackRestart => Self::PlaybackRestart,
            PlayerEvent::Position { seconds } => Self::Position { seconds },
            PlayerEvent::Duration { seconds } => Self::Duration { seconds },
            PlayerEvent::Paused(paused) => Self::Paused { paused },
            PlayerEvent::Buffering(buffering) => Self::Buffering { buffering },
            PlayerEvent::VideoGamma(gamma) => Self::VideoGamma { gamma },
            PlayerEvent::Metric { name, millis } => Self::Metric {
                name: name.to_owned(),
                millis,
            },
            PlayerEvent::TracksChanged => Self::TracksChanged,
            PlayerEvent::ChaptersChanged => Self::ChaptersChanged,
            PlayerEvent::EndFile { reason, error } => Self::EndFile {
                reason: match reason {
                    EndReason::Eof => PlaybackEndReason::Eof,
                    EndReason::Stopped => PlaybackEndReason::Stopped,
                    EndReason::Error => PlaybackEndReason::Error,
                },
                error,
            },
            PlayerEvent::Shutdown => Self::Shutdown,
        }
    }
}

/// Implemented in Swift. Called on a background thread; must not block.
#[uniffi::export(with_foreign)]
pub trait PlaybackObserver: Send + Sync {
    fn on_event(&self, event: PlaybackEvent);
}

/// Implemented in Swift: asks the video layer to redraw. Called on an mpv
/// thread for every new frame; must not block.
#[uniffi::export(with_foreign)]
pub trait RenderObserver: Send + Sync {
    fn needs_display(&self);
}

pub(crate) struct ObserverAdapter(pub(crate) Arc<dyn PlaybackObserver>);

impl PlayerEvents for ObserverAdapter {
    fn on_event(&self, event: PlayerEvent) {
        self.0.on_event(event.into());
    }
}
