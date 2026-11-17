#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;

use cinma_subtitles::opensubtitles::OpenSubtitlesClient;
use cinma_subtitles::{
    SubtitleCandidate, SubtitleQueryParams, decode_subtitle_bytes, normalize_lang_code,
    prepare_subtitle_file, rank_candidates,
};
use flate2::Compression;
use flate2::write::GzEncoder;

#[test]
fn detects_turkish_windows_1254_and_converts_to_utf8() {
    // "1\n00:00:01,000 --> 00:00:03,000\nYağmur yağışlı ve güneşli\n" encoded in CP1254
    let sample = "1\n00:00:01,000 --> 00:00:03,000\nYağmur yağışlı ve İğneada\n";
    let (encoded, _, had_errors) = encoding_rs::WINDOWS_1254.encode(sample);
    assert!(!had_errors);

    let decoded = decode_subtitle_bytes(&encoded, None, None);
    assert_eq!(decoded.detected_encoding, "windows-1254");
    assert_eq!(decoded.text, sample);
}

#[test]
fn unpacks_gzipped_subtitles_and_preserves_ass_without_restyle() {
    let ass = "[Script Info]\nTitle: Example\n\n[V4+ Styles]\nStyle: Default,Arial,48,&H00FFFFFF\n\n[Events]\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Merhaba Dünya\n";
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(ass.as_bytes()).unwrap();
    let gz = encoder.finish().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let prepared =
        prepare_subtitle_file(&gz, dir.path(), "test-ass", Some("ass"), None, Some("tr")).unwrap();
    assert_eq!(prepared.format, "ass");
    assert!(!prepared.restyled_ass);
    let written = std::fs::read_to_string(prepared.local_path).unwrap();
    assert_eq!(written, ass);
}

#[test]
fn ranks_preferred_languages_and_hash_match_first() {
    let mut items = vec![
        SubtitleCandidate {
            id: "1".into(),
            provider: "A".into(),
            lang: normalize_lang_code("en"),
            title: Some("English".into()),
            download_url: "https://x/en.srt".into(),
            opensubtitles_file_id: None,
            encoding_hint: None,
            is_sdh: false,
            is_forced: false,
            hash_matched: true,
        },
        SubtitleCandidate {
            id: "2".into(),
            provider: "A".into(),
            lang: normalize_lang_code("tr"),
            title: Some("Turkish SDH".into()),
            download_url: "https://x/tr-sdh.srt".into(),
            opensubtitles_file_id: None,
            encoding_hint: None,
            is_sdh: true,
            is_forced: false,
            hash_matched: false,
        },
        SubtitleCandidate {
            id: "3".into(),
            provider: "OpenSubtitles".into(),
            lang: normalize_lang_code("tur"),
            title: Some("Turkish hash match".into()),
            download_url: "https://x/tr.srt".into(),
            opensubtitles_file_id: None,
            encoding_hint: None,
            is_sdh: false,
            is_forced: false,
            hash_matched: true,
        },
    ];
    rank_candidates(&mut items, &["tr-TR".into(), "eng".into()]);
    assert_eq!(
        items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["3", "2", "1"]
    );
}

#[tokio::test]
async fn opensubtitles_client_searches_and_downloads_against_mock_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let (status, ctype, body): (&str, &str, &str) = if req.starts_with("GET /subtitles?") {
                (
                    "200 OK",
                    "application/json",
                    r#"{"data":[{"attributes":{"language":"tr","release":"Film.2024.1080p","hearing_impaired":false,"foreign_parts_only":false,"moviehash_match":true,"files":[{"file_id":42,"file_name":"film.srt"}]}}]}"#,
                )
            } else if req.starts_with("POST /download ") {
                let dl = format!(r#"{{"link":"http://{addr}/raw.srt","file_name":"film.srt"}}"#);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    dl.len(),
                    dl
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
                continue;
            } else {
                (
                    "200 OK",
                    "text/plain",
                    "1\n00:00:01,000 --> 00:00:02,000\nSelam\n",
                )
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        }
    });

    let client =
        OpenSubtitlesClient::with_base_url(format!("http://{addr}"), "test-api-key".into(), None)
            .unwrap();
    let results = client
        .search(&SubtitleQueryParams {
            imdb_id: Some("tt1254207".into()),
            video_hash: Some("0123456789abcdef".into()),
            languages: vec!["tur".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].hash_matched);
    assert_eq!(results[0].opensubtitles_file_id, Some(42));

    let (bytes, ext) = client.download_file(42).await.unwrap();
    assert_eq!(ext, "srt");
    assert!(String::from_utf8_lossy(&bytes).contains("Selam"));

    server.await.unwrap();
}
