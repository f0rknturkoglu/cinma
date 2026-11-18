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
