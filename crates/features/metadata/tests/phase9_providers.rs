#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use cinma_metadata::{FanartClient, KitsuClient, SkipKind, SkipSegmentClient, TmdbClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_mock(routes: Vec<(&'static str, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let routes = Arc::clone(&routes);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let target = req
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/");
                let body = routes
                    .iter()
                    .find(|(p, _)| target.starts_with(p))
                    .map(|(_, b)| b.as_str())
                    .unwrap_or("{}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn aniskip_and_introdb_parse_segments() {
    let url = spawn_mock(vec![
        (
            "/skip-times/16498/1",
            r#"{"found":true,"results":[{"skipType":"op","interval":{"startTime":10.5,"endTime":100.5}},{"skipType":"ed","interval":{"startTime":1320.0,"endTime":1410.0}}]}"#.into(),
        ),
        (
            "/segments",
            r#"{"segments":[{"type":"intro","start_sec":12.0,"end_sec":54.0}]}"#.into(),
        ),
    ])
    .await;

    let client = SkipSegmentClient::with_base_urls(url.clone(), url).unwrap();
    let ani = client.fetch_aniskip(16498, 1).await.unwrap();
    assert_eq!(ani.len(), 2);
    assert_eq!(ani[0].kind, SkipKind::Intro);
    assert_eq!(ani[0].source, "AniSkip");
    assert_eq!(ani[1].kind, SkipKind::Outro);

    let introdb = client
        .fetch_introdb("tt0903747", Some(1), Some(1))
        .await
        .unwrap();
    assert_eq!(introdb.len(), 1);
    assert_eq!(introdb[0].source, "IntroDB");
    assert_eq!(introdb[0].start_s, 12.0);
}

#[tokio::test]
async fn fanart_kitsu_and_tmdb_watch_providers() {
    let url = spawn_mock(vec![
        (
            "/movies/tt0133093",
            r#"{"hdmovielogo":[{"url":"https://img.fanart/logo_en.png","lang":"en"}],"moviebackground":[{"url":"https://img.fanart/bg.jpg","lang":"00"}]}"#.into(),
        ),
        (
            "/anime/7442",
            r#"{"data":{"attributes":{"canonicalTitle":"Attack on Titan","subtype":"TV","averageRating":"85.4","episodeLength":24}}}"#.into(),
        ),
        (
            "/movie/603/watch/providers",
            r#"{"results":{"TR":{"flatrate":[{"provider_id":8,"provider_name":"Netflix","logo_path":"/n.png"}]}}}"#.into(),
        ),
    ])
    .await;

    let fanart = FanartClient::with_base_url(url.clone()).unwrap();
    let art = fanart
        .fetch_art("fk", "movie", "tt0133093", "tr")
        .await
        .unwrap();
    assert_eq!(art.logo.as_deref(), Some("https://img.fanart/logo_en.png"));
    assert_eq!(art.backdrop.as_deref(), Some("https://img.fanart/bg.jpg"));

    let kitsu = KitsuClient::with_base_url(url.clone()).unwrap();
    let rich = kitsu.anime_details(7442).await.unwrap();
    assert_eq!(rich.meta.name, "Attack on Titan");
    assert_eq!(rich.id_map.kitsu_id, Some(7442));
    assert_eq!(rich.meta.imdb_rating.as_deref(), Some("8.5"));

    let tmdb = TmdbClient::with_base_url(&url).unwrap();
    let providers = tmdb
        .watch_providers("tk", "movie", 603, "TR")
        .await
        .unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, "Netflix");
    assert_eq!(providers[0].kind, "flatrate");
}
