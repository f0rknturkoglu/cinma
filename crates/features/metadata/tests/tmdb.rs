#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use cinma_metadata::{TmdbClient, parse_tmdb_prefixed_id};

fn start_mock_tmdb(routes: HashMap<String, String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let routes = Arc::new(routes);

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let routes = Arc::clone(&routes);
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
                let (status, body) = match routes.get(&path) {
                    Some(b) => (200, b.as_str()),
                    None => (404, r#"{"status_message":"not found"}"#),
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

    format!("http://{addr}")
}

#[test]
fn tmdb_find_movie_tv_season_and_trending() {
    let mut routes = HashMap::new();
    routes.insert(
        "/find/tt0133093".into(),
        r#"{"movie_results":[{"id":603}],"tv_results":[]}"#.into(),
    );
    routes.insert(
        "/movie/603".into(),
        r#"{
            "title": "Matrix",
            "overview": "Gerçekliğin doğasını keşfeden bir bilgisayar korsanı.",
            "poster_path": "/poster.jpg",
            "backdrop_path": "/bg.jpg",
            "release_date": "1999-03-31",
            "runtime": 136,
            "vote_average": 8.2,
            "genres": [{"name": "Aksiyon"}, {"name": "Bilim-Kurgu"}],
            "external_ids": {"imdb_id": "tt0133093"},
            "credits": {
                "cast": [{"name": "Keanu Reeves"}],
                "crew": [{"name": "Lana Wachowski", "job": "Director"}]
            },
            "videos": {
                "results": [{"key": "vKQi3bBA1y8", "site": "YouTube", "type": "Trailer"}]
            },
            "images": {
                "logos": [
                    {"file_path": "/logo-en.png", "iso_639_1": "en"},
                    {"file_path": "/logo-tr.png", "iso_639_1": "tr"}
                ]
            }
        }"#
        .into(),
    );
    routes.insert(
        "/tv/1399".into(),
        r#"{
            "name": "Game of Thrones",
            "first_air_date": "2011-04-17",
            "last_air_date": "2019-05-19",
            "in_production": false,
            "episode_run_time": [57],
            "vote_average": 8.4,
            "genres": [{"name": "Dram"}],
            "seasons": [{"season_number": 0}, {"season_number": 1}, {"season_number": 2}],
            "next_episode_to_air": {
                "season_number": 2,
                "episode_number": 1,
                "name": "The North Remembers",
                "air_date": "2026-12-01"
            },
            "external_ids": {"imdb_id": "tt0944947", "tvdb_id": 121361}
        }"#
        .into(),
    );
    routes.insert(
        "/tv/1399/season/1".into(),
        r#"{
            "episodes": [{
                "season_number": 1,
                "episode_number": 1,
                "name": "Kış Geliyor",
                "air_date": "2011-04-17",
                "still_path": "/s1e1.jpg"
            }]
        }"#
        .into(),
    );
    routes.insert(
        "/trending/movie/week".into(),
        r#"{"results":[{"id":603,"media_type":"movie","title":"Matrix","poster_path":"/p.jpg","release_date":"1999-03-31","vote_average":8.2}]}"#.into(),
    );

    let base = start_mock_tmdb(routes);
    let client = TmdbClient::with_base_url(&base).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let found = rt.block_on(client.find_by_imdb("k", "tt0133093", "tr-TR")).unwrap().unwrap();
    assert_eq!(found.tmdb_id, 603);
    assert_eq!(found.media_type, "movie");

    let movie = rt.block_on(client.movie_details("k", 603, "tr-TR")).unwrap();
    assert_eq!(movie.meta.id, "tt0133093");
    assert_eq!(movie.meta.name, "Matrix");
    assert_eq!(movie.meta.logo.as_deref(), Some("https://image.tmdb.org/t/p/w500/logo-tr.png"));
    assert_eq!(movie.meta.trailers[0].source, "vKQi3bBA1y8");
    assert_eq!(movie.id_map.tmdb_id, Some(603));

    let tv = rt.block_on(client.tv_details("k", 1399, "tr-TR")).unwrap();
    assert_eq!(tv.meta.id, "tt0944947");
    assert_eq!(tv.season_numbers, vec![1, 2]);
    assert_eq!(tv.id_map.tvdb_id, Some(121361));
    assert_eq!(tv.next_episode.unwrap().air_date, "2026-12-01");

    let eps = rt.block_on(client.tv_season("k", 1399, "tt0944947", 1, "tr-TR")).unwrap();
    assert_eq!(eps[0].id, "tt0944947:1:1");
    assert_eq!(eps[0].title.as_deref(), Some("Kış Geliyor"));

    let trending = rt.block_on(client.trending("k", "movie", "tr-TR")).unwrap();
    assert_eq!(trending[0].id, "tmdb:603");
    assert_eq!(parse_tmdb_prefixed_id(&trending[0].id), Some(603));
}
