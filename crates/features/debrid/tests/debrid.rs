#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use cinma_debrid::{DebridClient, DebridProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_mock_debrid(routes: Vec<(&'static str, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let routes = Arc::clone(&routes);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                let target = first.split_whitespace().nth(1).unwrap_or("/");
                let body = routes
                    .iter()
                    .find(|(prefix, _)| target.starts_with(prefix))
                    .map(|(_, b)| b.as_str())
                    .unwrap_or("{}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn real_debrid_cache_and_resolve_roundtrip() {
    let rd = spawn_mock_debrid(vec![
        (
            "/torrents/instantAvailability/",
            r#"{"aaaa1111":{"rd":[{"1":{"filename":"Movie.mkv","filesize":1000}}]}}"#.into(),
        ),
        ("/torrents/addMagnet", r#"{"id":"T42"}"#.into()),
        (
            "/torrents/selectFiles/",
            r#"{}"#.into(),
        ),
        (
            "/torrents/info/T42",
            r#"{"files":[{"id":1,"path":"/Movie.mkv","bytes":5000}],"links":["https://rd.example/x"]}"#.into(),
        ),
        (
            "/unrestrict/link",
            r#"{"download":"https://cdn.rd.example/Movie.mkv","filename":"Movie.mkv"}"#.into(),
        ),
    ])
    .await;

    let client = DebridClient::with_base_urls(rd.clone(), rd.clone(), rd.clone(), rd).unwrap();
    let cached = client
        .check_instant_cache(
            DebridProvider::RealDebrid,
            "rd_key",
            &["AAAA1111".into(), "BBBB2222".into()],
        )
        .await
        .unwrap();
    assert!(cached.contains("aaaa1111"));
    assert!(!cached.contains("bbbb2222"));

    let resolved = client
        .resolve_torrent(DebridProvider::RealDebrid, "rd_key", "aaaa1111", None)
        .await
        .unwrap();
    assert_eq!(resolved.url, "https://cdn.rd.example/Movie.mkv");
    assert_eq!(resolved.filename.as_deref(), Some("Movie.mkv"));
}

#[tokio::test]
async fn alldebrid_premiumize_and_torbox_roundtrips() {
    let ad = spawn_mock_debrid(vec![
        (
            "/magnet/instant",
            r#"{"data":{"magnets":[{"hash":"deadbeef","instant":true}]}}"#.into(),
        ),
        (
            "/magnet/upload",
            r#"{"data":{"magnets":[{"id":99}]}}"#.into(),
        ),
        (
            "/magnet/status",
            r#"{"data":{"magnets":{"links":[{"link":"https://ad.link/1","filename":"Show.S01E01.mkv","size":900}]}}}"#.into(),
        ),
        (
            "/link/unlock",
            r#"{"data":{"link":"https://cdn.ad.example/Show.S01E01.mkv","filename":"Show.S01E01.mkv"}}"#.into(),
        ),
    ])
    .await;

    let pm = spawn_mock_debrid(vec![
        ("/cache/check", r#"{"response":[true,false]}"#.into()),
        (
            "/transfer/directdl",
            r#"{"content":[{"path":"Film.mp4","link":"https://cdn.pm.example/Film.mp4","size":700}]}"#.into(),
        ),
    ])
    .await;

    let tb = spawn_mock_debrid(vec![
        (
            "/torrents/checkcached",
            r#"{"data":[{"hash":"cafebabe"}]}"#.into(),
        ),
        (
            "/torrents/createtorrent",
            r#"{"data":{"torrent_id":77}}"#.into(),
        ),
        (
            "/torrents/mylist",
            r#"{"data":{"files":[{"id":3,"name":"Anime.mkv","size":800}]}}"#.into(),
        ),
        (
            "/torrents/requestdl",
            r#"{"data":"https://cdn.tb.example/Anime.mkv"}"#.into(),
        ),
    ])
    .await;

    let client = DebridClient::with_base_urls(ad.clone(), ad, pm, tb).unwrap();

    let ad_cached = client
        .check_instant_cache(DebridProvider::AllDebrid, "k", &["deadbeef".into()])
        .await
        .unwrap();
    assert!(ad_cached.contains("deadbeef"));
    let ad_res = client
        .resolve_torrent(DebridProvider::AllDebrid, "k", "deadbeef", None)
        .await
        .unwrap();
    assert_eq!(ad_res.url, "https://cdn.ad.example/Show.S01E01.mkv");

    let pm_cached = client
        .check_instant_cache(
            DebridProvider::Premiumize,
            "k",
            &["1111".into(), "2222".into()],
        )
        .await
        .unwrap();
    assert!(pm_cached.contains("1111"));
    assert!(!pm_cached.contains("2222"));
    let pm_res = client
        .resolve_torrent(DebridProvider::Premiumize, "k", "1111", None)
        .await
        .unwrap();
    assert_eq!(pm_res.url, "https://cdn.pm.example/Film.mp4");

    let tb_cached = client
        .check_instant_cache(DebridProvider::TorBox, "k", &["cafebabe".into()])
        .await
        .unwrap();
    assert!(tb_cached.contains("cafebabe"));
    let tb_res = client
        .resolve_torrent(DebridProvider::TorBox, "k", "cafebabe", None)
        .await
        .unwrap();
    assert_eq!(tb_res.url, "https://cdn.tb.example/Anime.mkv");
}
