#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use cinma_core::{
    Core, PlayerPreferences, PreferredSeriesSource, Stream, StreamCandidate, SubtitleSearchRequest,
    SubtitleSettings, VideoOffsets, WatchProgressInput,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn subtitle_settings_offsets_addon_fetch_and_cp1254_prepare_work() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            for _ in 0..3 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                if first.contains("/manifest.json") {
                    let body = r#"{"id":"subs.test","name":"SubsTest","version":"1.0.0","resources":["subtitles"],"types":["movie"]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).await.unwrap();
                } else if first.contains("/subtitles/movie/tt999") {
                    let body = format!(
                        r#"{{"subtitles":[{{"id":"s1","url":"http://{addr}/sub.srt","lang":"tur","SubEncoding":"windows-1254"}}]}}"#
                    );
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).await.unwrap();
                } else {
                    let sample = "1\n00:00:01,000 --> 00:00:02,000\nYağmur ve Güneş\n";
                    let (encoded, _, _) = encoding_rs::WINDOWS_1254.encode(sample);
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        encoded.len()
                    );
                    sock.write_all(header.as_bytes()).await.unwrap();
                    sock.write_all(&encoded).await.unwrap();
                }
            }
        });
    });

    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();
    let client_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let sub_settings = SubtitleSettings {
        srt_font_size: 52,
        ..Default::default()
    };
    core.set_subtitle_settings(&sub_settings).unwrap();
    assert_eq!(core.subtitle_settings().unwrap().srt_font_size, 52);

    let prefs = PlayerPreferences {
        autoplay_countdown_s: 7,
        ..Default::default()
    };
    core.set_player_preferences(&prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap().autoplay_countdown_s, 7);

    core.set_video_offsets(
        "tt999",
        &VideoOffsets {
            sub_delay: -0.5,
            audio_delay: 0.25,
        },
    )
    .unwrap();
    let offsets = core.video_offsets("tt999").unwrap();
    assert!((offsets.sub_delay + 0.5).abs() < 1e-6);

    client_rt.block_on(async {
        core.install_addon(format!("http://{addr}/manifest.json"))
            .await
            .unwrap();

        let found = core
            .fetch_subtitles(SubtitleSearchRequest {
                media_type: "movie".into(),
                video_id: "tt999".into(),
                video_hash: Some("0123456789abcdef".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        let prepared = core
            .download_and_prepare_subtitle(found.into_iter().next().unwrap())
            .await
            .unwrap();
        assert_eq!(prepared.format, "srt");
        let text = std::fs::read_to_string(prepared.local_path).unwrap();
        assert!(text.contains("Yağmur ve Güneş"));
    });
    server_thread.join().unwrap();
}

#[test]
fn wave1_settings_persist_and_deserialize_legacy_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    // Verify legacy partial JSON deserializes cleanly via #[serde(default)].
    let legacy_sub: SubtitleSettings =
        serde_json::from_str(r#"{"srt_font_size":50,"srt_bold":true}"#).unwrap();
    assert_eq!(legacy_sub.srt_font_size, 50);
    assert!(legacy_sub.srt_bold);
    assert_eq!(legacy_sub.srt_opacity, 100);
    assert_eq!(legacy_sub.srt_box_opacity, 0);
    assert_eq!(legacy_sub.srt_align_x, "center");
    assert_eq!(legacy_sub.secondary_pos, 12);
    assert!(legacy_sub.lift_above_controls);
    assert!(!legacy_sub.off_by_default);
    assert!(!legacy_sub.prefer_embedded);
    assert!(!legacy_sub.forced_when_native_audio);

    let legacy_prefs: PlayerPreferences = serde_json::from_str(r#"{"exact_seek":false}"#).unwrap();
    assert!(!legacy_prefs.exact_seek);
    assert_eq!(legacy_prefs.seek_back_step_s, 10);
    assert_eq!(legacy_prefs.seek_forward_step_s, 10);
    assert_eq!(legacy_prefs.default_speed_pct, 100);
    assert_eq!(legacy_prefs.volume_boost_max, 150);
    assert!(legacy_prefs.pause_minimized);
    assert!(!legacy_prefs.pause_unfocused);
    assert!(!legacy_prefs.auto_skip_intro);
    assert!(legacy_prefs.show_skip_button);
    assert!(legacy_prefs.auto_next_stream_on_stall);
    assert_eq!(legacy_prefs.stall_timeout_s, 12);
    assert_eq!(legacy_prefs.crop_mode, "fit");
    assert!(!legacy_prefs.audio_normalize);
    assert!(!legacy_prefs.downmix_stereo);

    // Verify all Wave 1 fields roundtrip through SQLite.
    let updated_sub = SubtitleSettings {
        srt_opacity: 80,
        srt_box_opacity: 60,
        srt_align_x: "left".into(),
        secondary_pos: 88,
        lift_above_controls: false,
        off_by_default: true,
        prefer_embedded: true,
        forced_when_native_audio: true,
        ..Default::default()
    };
    core.set_subtitle_settings(&updated_sub).unwrap();
    assert_eq!(core.subtitle_settings().unwrap(), updated_sub);

    let updated_prefs = PlayerPreferences {
        seek_back_step_s: 5,
        seek_forward_step_s: 30,
        default_speed_pct: 125,
        volume_boost_max: 200,
        pause_minimized: false,
        pause_unfocused: true,
        auto_skip_intro: true,
        auto_skip_recap: true,
        auto_skip_outro: true,
        show_skip_button: false,
        auto_next_stream_on_stall: false,
        stall_timeout_s: 20,
        crop_mode: "21:9".into(),
        audio_normalize: true,
        downmix_stereo: true,
        ..Default::default()
    };
    core.set_player_preferences(&updated_prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), updated_prefs);
}

#[test]
fn wave2_settings_source_lock_watched_filter_and_stream_sorting() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    let mut sub = core.subtitle_settings().unwrap();
    sub.hide_sdh = true;
    core.set_subtitle_settings(&sub).unwrap();
    assert!(core.subtitle_settings().unwrap().hide_sdh);

    let w2_prefs = PlayerPreferences {
        hide_spoilers: true,
        remember_last_stream: false,
        keep_source_next_episode: false,
        track_block_words: vec!["HDCAM".into(), "CAM".into()],
        stream_sort: "seeds".into(),
        picker_show_filename: false,
        fullscreen_clock: false,
        fullscreen_clock_end_time: false,
        next_episode_lead_s: 45,
        still_watching_after_episodes: 3,
        hide_watched_in_catalogs: true,
        hide_unreleased: true,
        episode_sort_asc: false,
        episode_compact_layout: true,
        confirm_on_leave: true,
        resume_behavior: "ask".into(),
        volume_hud: false,
        screen_lock_enabled: false,
        ..Default::default()
    };
    core.set_player_preferences(&w2_prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), w2_prefs);

    let src = PreferredSeriesSource {
        addon_name: Some("Torrentio".into()),
        binge_group: Some("bg-1080p".into()),
        resolution: Some("1080p".into()),
    };
    core.set_series_source("tt1234", &src).unwrap();
    assert_eq!(core.series_source("tt1234").unwrap(), src);

    // Watched movie vs partially watched series: only fully completed meta without active CW appears.
    core.save_watch_progress(WatchProgressInput {
        video_id: "tt111".into(),
        meta_id: "tt111".into(),
        media_type: "movie".into(),
        title: "Done Movie".into(),
        artwork: None,
        season: None,
        episode: None,
        position_s: 5800.0,
        duration_s: 6000.0,
    })
    .unwrap();
    core.save_watch_progress(WatchProgressInput {
        video_id: "tt222:1:1".into(),
        meta_id: "tt222".into(),
        media_type: "series".into(),
        title: "Series Ep1".into(),
        artwork: None,
        season: Some(1),
        episode: Some(1),
        position_s: 1200.0,
        duration_s: 1200.0,
    })
    .unwrap();
    core.save_watch_progress(WatchProgressInput {
        video_id: "tt222:1:2".into(),
        meta_id: "tt222".into(),
        media_type: "series".into(),
        title: "Series Ep2".into(),
        artwork: None,
        season: Some(1),
        episode: Some(2),
        position_s: 300.0,
        duration_s: 1200.0,
    })
    .unwrap();
    let hidden = core.watched_meta_ids().unwrap();
    assert_eq!(hidden, vec!["tt111".to_owned()]);

    // Stream block-word filter + seeders sort.
    let s_cam: Stream =
        serde_json::from_str(r#"{"name":"CAM","title":"Movie.2025.HDCAM.mp4","url":"http://a"}"#)
            .unwrap();
    let s_low: Stream = serde_json::from_str(
        r#"{"name":"1080p","title":"Movie.1080p.WEB-DL 👤 12 💾 2.0 GB","infoHash":"1111111111111111111111111111111111111111"}"#,
    )
    .unwrap();
    let s_high_seeds: Stream = serde_json::from_str(
        r#"{"name":"720p","title":"Movie.720p.WEB-DL 👤 240 💾 1.2 GB","infoHash":"2222222222222222222222222222222222222222"}"#,
    )
    .unwrap();
    let ranked = core.rank_streams(
        vec![
            StreamCandidate {
                addon_name: "Test".into(),
                stream: s_cam,
            },
            StreamCandidate {
                addon_name: "Test".into(),
                stream: s_low,
            },
            StreamCandidate {
                addon_name: "Test".into(),
                stream: s_high_seeds,
            },
        ],
        None,
    );
    assert_eq!(ranked.ranked.len(), 2);
    assert_eq!(ranked.ranked[0].parsed.seeders, Some(240));
}

#[test]
fn wave3_settings_debrid_only_cw_caught_up_and_disk_download_limit() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    // Debrid-only mode filters out uncached P2P torrents while keeping HTTP direct streams.
    let w3_prefs = PlayerPreferences {
        debrid_only_mode: true,
        cw_hide_caught_up: true,
        addon_timeout_s: 25,
        max_addon_rails: 10,
        cw_at_top: false,
        poster_scale_pct: 110,
        poster_radius_pt: 16,
        hide_poster_titles: true,
        show_imdb_badge: false,
        show_type_badge: false,
        show_watched_badge: true,
        show_library_badge: true,
        seek_bar_chapters: false,
        player_title_series_first: false,
        ui_language: "tr".into(),
        player_always_on_top: true,
        discord_hide_title: true,
        discord_show_when_paused: false,
        discord_show_when_browsing: true,
        week_starts_monday: false,
        custom_sleep_minutes: 90,
        show_p2p_speed_chip: false,
        ..Default::default()
    };
    core.set_player_preferences(&w3_prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), w3_prefs);

    let s_p2p: Stream = serde_json::from_str(
        r#"{"name":"1080p","title":"Show.S01E01.1080p.WEB-DL 👤 40","infoHash":"1111111111111111111111111111111111111111"}"#,
    )
    .unwrap();
    let s_direct: Stream = serde_json::from_str(
        r#"{"name":"1080p","title":"Show.S01E01.Direct","url":"https://cdn.example/ep1.mp4"}"#,
    )
    .unwrap();
    let ranked = core.rank_streams(
        vec![
            StreamCandidate {
                addon_name: "Torrentio".into(),
                stream: s_p2p,
            },
            StreamCandidate {
                addon_name: "Direct".into(),
                stream: s_direct,
            },
        ],
        None,
    );
    assert_eq!(ranked.ranked.len(), 1);
    assert!(!ranked.ranked[0].parsed.is_torrent);

    // Continue Watching hides items >= 85% watched when cw_hide_caught_up is enabled.
    core.save_watch_progress(WatchProgressInput {
        video_id: "ep_nearly".into(),
        meta_id: "tt333".into(),
        media_type: "series".into(),
        title: "Almost Done".into(),
        artwork: None,
        season: Some(1),
        episode: Some(1),
        position_s: 880.0,
        duration_s: 1000.0,
    })
    .unwrap();
    core.save_watch_progress(WatchProgressInput {
        video_id: "ep_mid".into(),
        meta_id: "tt444".into(),
        media_type: "series".into(),
        title: "In Progress".into(),
        artwork: None,
        season: Some(1),
        episode: Some(1),
        position_s: 400.0,
        duration_s: 1000.0,
    })
    .unwrap();
    let cw = core.continue_watching(10).unwrap();
    assert_eq!(cw.len(), 1);
    assert_eq!(cw[0].meta_id, "tt444");

    // Disk policy download cap maps cleanly to TorrentPolicy.download_bps.
    let mut disk = core.disk_policy_settings().unwrap();
    disk.download_limit_kbps = 2048;
    core.set_disk_policy_settings(&disk).unwrap();
    let reloaded_disk = core.disk_policy_settings().unwrap();
    assert_eq!(reloaded_disk.download_limit_kbps, 2048);
    assert_eq!(
        reloaded_disk.torrent_policy(true).download_bps,
        Some(2048 * 1024)
    );
}

#[test]
fn wave4_lightweight_settings_persist_and_subtitle_provider_toggles_work() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    let sub = SubtitleSettings {
        sub_scale_pct: 120,
        ass_scale_with_window: false,
        enable_opensubtitles: false,
        enable_addon_subtitles: false,
        ..Default::default()
    };
    core.set_subtitle_settings(&sub).unwrap();
    let sub_loaded = core.subtitle_settings().unwrap();
    assert_eq!(sub_loaded, sub);

    let prefs = PlayerPreferences {
        prefer_original_titles: true,
        tmdb_image_lang: "null".into(),
        theme_accent: "cyan".into(),
        reduce_glass_effect: true,
        backdrop_dim_pct: 75,
        hidden_nav_tabs: vec!["calendar".into(), "downloads".into()],
        external_player_enabled: true,
        external_player_path: "/opt/homebrew/bin/mpv".into(),
        confirm_stremio_deeplink: false,
        mal_count_rewatches: true,
        ..Default::default()
    };
    core.set_player_preferences(&prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), prefs);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let empty_subs = rt
        .block_on(core.fetch_subtitles(SubtitleSearchRequest {
            media_type: "movie".into(),
            video_id: "tt0133093".into(),
            imdb_id: Some("tt0133093".into()),
            ..Default::default()
        }))
        .unwrap();
    assert!(empty_subs.is_empty());
}

#[test]
fn surpass_wave1_preferences_and_commands_persist() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    let sub = SubtitleSettings {
        sub_hdr_peak_nits: 220,
        sub_use_margins: false,
        ..Default::default()
    };
    core.set_subtitle_settings(&sub).unwrap();
    assert_eq!(core.subtitle_settings().unwrap(), sub);

    let prefs = PlayerPreferences {
        hold_to_boost_enabled: false,
        hold_boost_speed_pct: 250,
        eq_brightness: 8,
        eq_contrast: 4,
        eq_gamma: 14,
        eq_saturation: -5,
        screenshot_include_subtitles: false,
        trackpad_gestures_enabled: false,
        haptic_feedback_enabled: false,
        external_player_preset: "iina".into(),
        split_continue_watching: true,
        ..Default::default()
    };
    core.set_player_preferences(&prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), prefs);

    assert_eq!(
        core.resolve_key(cinma_core::CommandContext::Player, ".")
            .unwrap(),
        Some("player.frame_step")
    );
    assert_eq!(
        core.resolve_key(cinma_core::CommandContext::Player, ",")
            .unwrap(),
        Some("player.frame_back_step")
    );
}

#[test]
fn surpass_wave2_manual_skip_dialogue_search_batch_watched_and_prefs() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    // 1. Wave 2 PlayerPreferences persistence
    let prefs = PlayerPreferences {
        pause_on_audio_output_change: false,
        clipboard_catcher_enabled: true,
        blackout_other_screens: true,
        auto_pip_on_occlusion: true,
        notify_on_download_complete: false,
        quick_play_enabled: false,
        ..Default::default()
    };
    core.set_player_preferences(&prefs).unwrap();
    assert_eq!(core.player_preferences().unwrap(), prefs);

    // 2. Manual intro/outro skip markers synthesize Intro segment even without remote lookup
    core.set_manual_skip_markers(
        "tt999888",
        &cinma_core::ManualSkipMarkers {
            intro_end_s: Some(88.5),
            outro_last_s: Some(95.0),
        },
    )
    .unwrap();
    let loaded = core.manual_skip_markers("tt999888:1:2").unwrap();
    assert_eq!(loaded.intro_end_s, Some(88.5));
    assert_eq!(loaded.outro_last_s, Some(95.0));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let segs = rt
        .block_on(core.skip_segments("series".into(), "tt999888:1:2".into(), Some(1), Some(2)))
        .unwrap();
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].kind, cinma_core::SkipKind::Intro);
    assert!((segs[0].end_s - 88.5).abs() < 0.01);

    // 3. Subtitle cue search
    let srt_path = dir.path().join("sample.srt");
    std::fs::write(
        &srt_path,
        "1\n00:00:04,000 --> 00:00:06,000\nWelcome to Cinma\n\n2\n00:01:12,000 --> 00:01:15,000\nSecret clue appears here\n",
    )
    .unwrap();
    let hits = core
        .search_subtitle_cues(&srt_path, "secret CLUE", 20)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!((hits[0].start_s - 72.0).abs() < 0.01);

    // 4. Batch mark episodes watched
    core.mark_episodes_watched(
        "tt999888",
        "series",
        &[
            ("tt999888:1:1".into(), "Pilot".into(), 1, 1),
            ("tt999888:1:2".into(), "Second".into(), 1, 2),
        ],
        true,
    )
    .unwrap();
    let rows = core.progress_for_meta("tt999888").unwrap();
    assert_eq!(rows.iter().filter(|r| r.watched).count(), 2);

    // 5. Keybindings
    assert_eq!(
        core.resolve_key(cinma_core::CommandContext::Player, "cmd+f")
            .unwrap(),
        Some("player.search_dialogue")
    );
    assert_eq!(
        core.resolve_key(cinma_core::CommandContext::Player, "t")
            .unwrap(),
        Some("player.live_text")
    );
    assert_eq!(
        core.resolve_key(cinma_core::CommandContext::Global, "cmd+u")
            .unwrap(),
        Some("file.open_url")
    );
}

#[test]
fn wave3_dalga3_shelves_local_scanner_schedule_and_socket() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = Core::open(dir.path()).unwrap();

    // 1. Pinned Shelves (#N20)
    assert!(core.pinned_shelves().unwrap().is_empty());
    let shelf = core
        .pin_shelf(
            "Top Anime".into(),
            "kitsu".into(),
            "series".into(),
            "top".into(),
            Some("Action".into()),
        )
        .unwrap();
    // Pinning duplicate replaces cleanly rather than duplicating.
    core.pin_shelf(
        "Top Anime".into(),
        "kitsu".into(),
        "series".into(),
        "top".into(),
        Some("Action".into()),
    )
    .unwrap();
    assert_eq!(core.pinned_shelves().unwrap().len(), 1);
    core.unpin_shelf(&shelf.id).unwrap();
    assert!(core.pinned_shelves().unwrap().is_empty());

    // 2. Local Media Library Scanner (#N23)
    let media_root = dir.path().join("Media");
    std::fs::create_dir_all(media_root.join("Shows")).unwrap();
    let dummy_video = vec![0u8; 520 * 1024];
    std::fs::write(
        media_root.join("Shows/Severance.S02E03.1080p.WEB-DL.mkv"),
        &dummy_video,
    )
    .unwrap();
    std::fs::write(
        media_root.join("Dune.Part.Two.2024.2160p.mp4"),
        &dummy_video,
    )
    .unwrap();
    std::fs::write(media_root.join("sample.mkv"), b"tiny-sample").unwrap();
    std::fs::write(media_root.join("notes.txt"), &dummy_video).unwrap();

    let mut prefs_scan = core.player_preferences().unwrap();
    prefs_scan.local_media_folders = vec![media_root.to_string_lossy().into_owned()];
    core.set_player_preferences(&prefs_scan).unwrap();

    let entries = core.scan_local_folders().unwrap();
    assert_eq!(entries.len(), 2);
    let series_ep = entries
        .iter()
        .find(|e| e.media_type == "series")
        .expect("series entry");
    assert_eq!(series_ep.title, "Severance");
    assert_eq!(series_ep.season, Some(2));
    assert_eq!(series_ep.episode, Some(3));
    let movie = entries
        .iter()
        .find(|e| e.media_type == "movie")
        .expect("movie entry");
    assert_eq!(movie.title, "Dune Part Two");
    assert_eq!(movie.year.as_deref(), Some("2024"));
    assert_eq!(core.local_media_entries().unwrap().len(), 2);

    // 3. Download Schedule Window (#N25)
    assert!(cinma_core::within_download_schedule(2, 1, 7));
    assert!(!cinma_core::within_download_schedule(12, 1, 7));
    assert!(cinma_core::within_download_schedule(23, 22, 6));
    assert!(cinma_core::within_download_schedule(3, 22, 6));
    assert!(!cinma_core::within_download_schedule(14, 22, 6));
    assert!(cinma_core::within_download_schedule(10, 5, 5));

    let dl_id = core
        .add_http_download(
            "Night Job".into(),
            "https://example.invalid/video.mp4".into(),
            Default::default(),
            None,
            None,
            10,
        )
        .unwrap();
    let mut prefs = core.player_preferences().unwrap();
    prefs.download_schedule_enabled = true;
    prefs.download_schedule_start_hour = 1;
    prefs.download_schedule_end_hour = 6;
    prefs.local_api_socket_enabled = true;
    core.set_player_preferences(&prefs).unwrap();

    // Outside window (hour 12) pauses queued download.
    core.enforce_download_schedule(Some(12)).unwrap();
    let paused_row = core
        .downloads()
        .unwrap()
        .into_iter()
        .find(|r| r.id == dl_id)
        .unwrap();
    assert_eq!(paused_row.state, cinma_core::DownloadState::Paused);

    // Inside window (hour 3) resumes it.
    core.enforce_download_schedule(Some(3)).unwrap();
    let resumed_row = core
        .downloads()
        .unwrap()
        .into_iter()
        .find(|r| r.id == dl_id)
        .unwrap();
    assert_ne!(resumed_row.state, cinma_core::DownloadState::Paused);

    // 4. Opt-in Local Unix Socket (`cinma.sock`, #N25)
    let sock_path = core.local_api_socket_path();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for _ in 0..20 {
            if std::path::Path::new(&sock_path).exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let stream = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&sock_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "socket permissions must be 0600, got {mode:o}");
        }
        let (r, mut w) = stream.into_split();
        w.write_all(b"{\"id\":1,\"method\":\"status\"}\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(r);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(val["id"], 1);
        assert!(val["result"]["version"].is_string());
        assert_eq!(val["result"]["downloads"], 1);

        // An endless line is refused instead of buffered.
        let big = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        let (r2, mut w2) = big.into_split();
        let junk = vec![b'x'; 200 * 1024];
        let _ = w2.write_all(&junk).await;
        let mut reader2 = BufReader::new(r2);
        let mut reply = String::new();
        reader2.read_line(&mut reply).await.unwrap();
        assert!(reply.contains("request too large"), "got {reply}");
    });
}

#[test]
fn p1_stream_serialization_local_year_titles_and_debrid_ranker() {
    // #26: `Stream::file_idx: None` must omit `"fileIdx"` so hosts do not treat null as batch item.
    let single = Stream {
        info_hash: Some("deadbeef".to_owned()),
        file_idx: None,
        ..Default::default()
    };
    let single_json = serde_json::to_string(&single).unwrap();
    assert!(!single_json.contains("fileIdx"), "got {single_json}");

    let batch = Stream {
        info_hash: Some("deadbeef".to_owned()),
        file_idx: Some(2),
        ..Default::default()
    };
    let batch_json = serde_json::to_string(&batch).unwrap();
    assert!(batch_json.contains("\"fileIdx\":2"), "got {batch_json}");

    // #13: Year-titled movies like `1917` / `2001` / `Blade Runner 2049` parse cleanly.
    let a = cinma_core::parse_local_media_filename(
        std::path::Path::new("1917.2019.1080p.BluRay.mkv"),
        1024,
    );
    assert_eq!(a.title, "1917");
    assert_eq!(a.year.as_deref(), Some("2019"));

    let b = cinma_core::parse_local_media_filename(
        std::path::Path::new("2001.A.Space.Odyssey.1968.2160p.mkv"),
        1024,
    );
    assert_eq!(b.title, "2001 A Space Odyssey");
    assert_eq!(b.year.as_deref(), Some("1968"));

    let c = cinma_core::parse_local_media_filename(
        std::path::Path::new("Blade.Runner.2049.2017.1080p.mkv"),
        1024,
    );
    assert_eq!(c.title, "Blade Runner 2049");
    assert_eq!(c.year.as_deref(), Some("2017"));

    let d = cinma_core::parse_local_media_filename(std::path::Path::new("1917.mkv"), 1024);
    assert_eq!(d.title, "1917");
    assert_eq!(d.year, None);

    // #20/#21/#25: `rank_streams_with_debrid` respects `track_block_words`, `debrid_only_mode`, and `stream_sort`.
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let prefs = PlayerPreferences {
        track_block_words: vec!["camrip".to_owned()],
        debrid_only_mode: true,
        stream_sort: "seeds".to_owned(),
        ..Default::default()
    };
    core.set_player_preferences(&prefs).unwrap();

    let candidates = vec![
        StreamCandidate {
            addon_name: "Direct".to_owned(),
            stream: Stream {
                name: Some("1080p Direct".to_owned()),
                title: Some("Movie 1080p.mp4".to_owned()),
                url: Some("https://example.com/movie.mp4".to_owned()),
                ..Default::default()
            },
        },
        StreamCandidate {
            addon_name: "DirectCAM".to_owned(),
            stream: Stream {
                name: Some("720p CAMRip".to_owned()),
                title: Some("Movie CAMRip.mp4".to_owned()),
                url: Some("https://example.com/cam.mp4".to_owned()),
                ..Default::default()
            },
        },
        StreamCandidate {
            addon_name: "P2P".to_owned(),
            stream: Stream {
                name: Some("4K Torrent".to_owned()),
                title: Some("Movie 2160p 👤 999".to_owned()),
                info_hash: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
                ..Default::default()
            },
        },
    ];

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let ranked = rt
        .block_on(core.rank_streams_with_debrid(candidates, None))
        .unwrap();
    assert_eq!(ranked.ranked.len(), 1);
    assert_eq!(ranked.ranked[0].addon_name, "Direct");
}
