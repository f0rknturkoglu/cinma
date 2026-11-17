#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use cinma_core::{Core, PlayerPreferences, SubtitleSearchRequest, SubtitleSettings, VideoOffsets};
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
