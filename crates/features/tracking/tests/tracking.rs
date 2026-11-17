#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_tracking::anime_map::{BUNDLED_STARTER_JSON, parse_anime_id_dataset};
use cinma_tracking::letterboxd::{LetterboxdCsvRow, export_csv, parse_rss};
use cinma_tracking::stremio_import::fetch_stremio_account_snapshot;
use cinma_tracking::{ScrobblePayload, TrackingClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn letterboxd_rss_and_csv_and_anime_map_work() {
    let xml = r#"<rss><channel>
        <item>
            <letterboxd:filmTitle>Parasite</letterboxd:filmTitle>
            <letterboxd:filmYear>2019</letterboxd:filmYear>
            <letterboxd:watchedDate>2026-11-10</letterboxd:watchedDate>
            <letterboxd:memberRating>4.5</letterboxd:memberRating>
            <link>https://letterboxd.com/u/film/parasite/</link>
        </item>
    </channel></rss>"#;
    let items = parse_rss(xml);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Parasite");
    assert_eq!(items[0].year, Some(2019));

    let csv = export_csv(&[LetterboxdCsvRow {
        title: "Hello, World".into(),
        year: Some(2024),
        watched_date: Some("2026-11-17".into()),
        rating10: Some(9.0),
        review: Some("Great \"film\"".into()),
    }]);
    assert!(csv.starts_with("Title,Year,WatchedDate,Rating10,Review\n"));
    assert!(csv.contains("\"Hello, World\",2024,2026-11-17,9.0,\"Great \"\"film\"\"\""));

    let anime = parse_anime_id_dataset(BUNDLED_STARTER_JSON.as_bytes()).unwrap();
    assert!(
        anime
            .iter()
            .any(|a| a.imdb_id.as_deref() == Some("tt0245429") && a.mal_id == Some(199))
    );
}

#[tokio::test]
async fn trakt_simkl_anilist_mal_and_stremio_import_work_against_mock_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        for _ in 0..10 {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let first = req.lines().next().unwrap_or("");
            let body = if first.contains("/oauth/device/code") {
                r#"{"device_code":"dev1","user_code":"ABCD","verification_url":"https://trakt.tv/activate","expires_in":600,"interval":5}"#
            } else if first.contains("/oauth/device/token") {
                r#"{"access_token":"tok_trakt"}"#
            } else if first.contains("/recommendations/movies") {
                r#"[{"title":"Stalker","year":1979,"ids":{"imdb":"tt0079944","tmdb":1398}}]"#
            } else if first.contains("/login") {
                r#"{"result":{"authKey":"auth123"}}"#
            } else if first.contains("/addonCollectionGet") {
                r#"{"result":{"addons":[{"transportUrl":"https://cinemeta.example/manifest.json"}]}}"#
            } else if first.contains("/datastoreGet") {
                r#"{"result":[{"_id":"tt6751668","type":"movie","name":"Parasite","poster":"https://img/p.jpg","removed":false}]}"#
            } else {
                r#"{"ok":true}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        }
    });

    let base = format!("http://{addr}");
    let client = TrackingClient::with_base_urls(
        base.clone(),
        base.clone(),
        base.clone(),
        base.clone(),
        base.clone(),
    )
    .unwrap();
    let code = client.trakt_device_code("cid").await.unwrap();
    assert_eq!(code.user_code, "ABCD");
    let tok = client
        .trakt_poll_token("cid", "csec", &code.device_code)
        .await
        .unwrap()
        .unwrap();
    let payload = ScrobblePayload {
        media_type: "movie".into(),
        title: "Stalker".into(),
        year: Some(1979),
        imdb_id: Some("tt0079944".into()),
        tmdb_id: Some(1398),
        tvdb_id: None,
        mal_id: Some(199),
        anilist_id: Some(199),
        season: None,
        episode: None,
        progress_pct: 95.0,
    };
    client
        .trakt_scrobble("cid", &tok, "stop", &payload)
        .await
        .unwrap();
    let recs = client.trakt_recommendations("cid", &tok).await.unwrap();
    assert_eq!(recs[0].title, "Stalker");
    client
        .simkl_sync_history("scid", "stok", &payload)
        .await
        .unwrap();
    client
        .anilist_update_progress("atok", 199, 1, true)
        .await
        .unwrap();
    client
        .mal_update_progress("mtok", 199, 1, true)
        .await
        .unwrap();

    let snap = fetch_stremio_account_snapshot(Some(&base), "u@e.com", "pw")
        .await
        .unwrap();
    assert_eq!(
        snap.addon_urls,
        vec!["https://cinemeta.example/manifest.json"]
    );
    assert_eq!(snap.library[0].id, "tt6751668");

    srv.await.unwrap();
}
