#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use cinma_core::{Core, WatchProgressInput};

fn start_server(routes: HashMap<String, String>) -> (String, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let routes = Arc::new(routes);
    let hits = Arc::new(Mutex::new(0usize));
    let hits_ref = Arc::clone(&hits);

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let routes = Arc::clone(&routes);
            let hits_ref = Arc::clone(&hits_ref);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first_line = String::new();
                if reader.read_line(&mut first_line).is_err() {
                    return;
                }
                let raw_path = first_line.split_whitespace().nth(1).unwrap_or("/");
                let path = raw_path.split('?').next().unwrap_or("/").to_owned();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                }
                *hits_ref.lock().unwrap() += 1;
                let (status, body) = match routes.get(&path) {
                    Some(b) => (200, b.as_str()),
                    None => (404, r#"{"error":"not found"}"#),
                };
                let mut s = stream;
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });

    (format!("http://{addr}"), hits)
}

#[test]
fn tmdb_primary_caches_and_populates_calendar_then_falls_back_to_addon() {
    let mut routes = HashMap::new();
    routes.insert(
        "/find/tt0944947".into(),
        r#"{"movie_results":[],"tv_results":[{"id":1399}]}"#.into(),
    );
    routes.insert(
        "/tv/1399".into(),
        r#"{
            "name": "Kuzey Hattı",
            "overview": "Test dizi",
            "poster_path": "/p.jpg",
            "backdrop_path": "/b.jpg",
            "first_air_date": "2024-01-01",
            "in_production": true,
            "seasons": [{"season_number": 1}],
            "next_episode_to_air": {
                "season_number": 1,
                "episode_number": 5,
                "name": "Sinyal Kulesi",
                "air_date": "2026-11-25"
            },
            "external_ids": {"imdb_id": "tt0944947"}
        }"#
        .into(),
    );
    routes.insert(
        "/tv/1399/season/1".into(),
        r#"{"episodes":[{"season_number":1,"episode_number":1,"name":"Pilot","air_date":"2024-01-01"}]}"#.into(),
    );
    routes.insert(
        "/addon/manifest.json".into(),
        r#"{"id":"com.example.meta","name":"Cinemeta","version":"1.0","resources":["meta"],"types":["movie"],"idPrefixes":["tt"]}"#.into(),
    );
    routes.insert(
        "/addon/meta/movie/tt0133093.json".into(),
        r#"{"meta":{"id":"tt0133093","type":"movie","name":"The Matrix"}}"#.into(),
    );

    let (base, hits) = start_server(routes);
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // 1. With TMDB key set: resolves via TMDB, caches in meta_cache, populates calendar.
    core.configure_tmdb_for_test(&base, Some("test-key".into()))
        .unwrap();
    let rich = rt
        .block_on(core.rich_meta("series".into(), "tt0944947".into()))
        .unwrap();
    assert_eq!(rich.source, "tmdb");
    assert_eq!(rich.meta.name, "Kuzey Hattı");
    assert_eq!(rich.meta.videos[0].id, "tt0944947:1:1");
    let hits_after_first = *hits.lock().unwrap();

    // Second call hits SQLite meta_cache (0 extra HTTP requests).
    let cached = rt
        .block_on(core.rich_meta("series".into(), "tt0944947".into()))
        .unwrap();
    assert_eq!(cached.source, "cache");
    assert_eq!(*hits.lock().unwrap(), hits_after_first);

    // Add series to library -> calendar returns its upcoming episode.
    core.add_to_library("tt0944947".into(), "series".into(), "Kuzey Hattı".into(), None)
        .unwrap();
    let cal = rt.block_on(core.calendar_entries()).unwrap();
    assert_eq!(cal.len(), 1);
    assert_eq!(cal[0].episode_name, "Sinyal Kulesi");

    // 2. Keyless fallback -> Stremio addon meta.
    core.configure_tmdb_for_test(&base, None).unwrap();
    rt.block_on(core.install_addon(format!("{base}/addon/manifest.json")))
        .unwrap();
    let fallback = rt
        .block_on(core.rich_meta("movie".into(), "tt0133093".into()))
        .unwrap();
    assert_eq!(fallback.source, "Cinemeta");
    assert_eq!(fallback.meta.name, "The Matrix");
}

#[test]
fn profiles_and_watch_progress_work_and_auto_mark_watched() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();

    core.save_watch_progress(WatchProgressInput {
        video_id: "tt1:1:1".into(),
        meta_id: "tt1".into(),
        media_type: "series".into(),
        title: "Show".into(),
        artwork: None,
        season: Some(1),
        episode: Some(1),
        position_s: 500.0,
        duration_s: 1000.0,
    })
    .unwrap();
    assert_eq!(core.continue_watching(10).unwrap().len(), 1);

    // Reaching >= 90% automatically marks watched and drops it from Continue Watching.
    core.save_watch_progress(WatchProgressInput {
        video_id: "tt1:1:1".into(),
        meta_id: "tt1".into(),
        media_type: "series".into(),
        title: "Show".into(),
        artwork: None,
        season: Some(1),
        episode: Some(1),
        position_s: 920.0,
        duration_s: 1000.0,
    })
    .unwrap();
    assert!(core.continue_watching(10).unwrap().is_empty());
    assert!(core.get_watch_progress("tt1:1:1").unwrap().unwrap().watched);

    // Switching profile isolates continue watching & library.
    let p2 = core.create_profile("Misafir".into(), Some("M".into())).unwrap();
    core.set_active_profile(p2).unwrap();
    assert_eq!(core.active_profile().unwrap(), p2);
    assert!(core.library_items().unwrap().is_empty());
    assert!(core.progress_for_meta("tt1").unwrap().is_empty());
}
