#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_stremio::{
    CatalogQuery, Manifest, catalog_url, meta_url, normalize_manifest_url, parse_catalog,
    parse_meta, parse_streams, stream_url,
};

const TORRENTIO_STREAMS: &str = include_str!("fixtures/streams_torrent.json");
const TORRENTIO_MANIFEST: &str = include_str!("fixtures/manifest.json");
const CINEMETA_MANIFEST: &str = include_str!("fixtures/manifest_cinemeta.json");
const CATALOG_MOVIES: &str = include_str!("fixtures/catalog_movies.json");
const META_SERIES: &str = include_str!("fixtures/meta_series.json");

#[test]
fn parses_torrent_streams() {
    let streams = parse_streams(TORRENTIO_STREAMS.as_bytes()).unwrap();
    assert_eq!(streams.len(), 3);
    let s = &streams[0];
    assert_eq!(
        s.info_hash.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(s.file_idx, Some(2));
    assert_eq!(
        s.trackers(),
        vec!["udp://tracker.example.org:1337/announce".to_owned()]
    );
    let hints = s.behavior_hints.as_ref().unwrap();
    assert_eq!(
        hints.filename.as_deref(),
        Some("Example.Movie.2024.2160p.WEB-DL.DV.HDR.mkv")
    );
    assert_eq!(hints.video_size, Some(15_032_385_536));
    assert_eq!(hints.binge_group.as_deref(), Some("example|2160p|DV"));
    assert!(streams[1].file_idx.is_none());
    assert_eq!(
        streams[2].url.as_deref(),
        Some("https://cdn.example.org/video.mp4")
    );
}

#[test]
fn tolerates_unknown_and_missing_fields() {
    let streams =
        parse_streams(br#"{"streams":[{"infoHash":"aa","extra":1}],"cacheMaxAge":5}"#).unwrap();
    assert_eq!(streams[0].info_hash.as_deref(), Some("aa"));
    assert!(parse_streams(b"{}").unwrap().is_empty());
    assert!(parse_streams(b"not json").is_err());
}

#[test]
fn parses_manifest_and_checks_support() {
    let m: Manifest = serde_json::from_str(TORRENTIO_MANIFEST).unwrap();
    assert_eq!(m.id, "org.example.fixture");
    assert!(m.supports("stream", "movie", "tt1234567"));
    assert!(m.supports("stream", "series", "tt1234567:1:2"));
    assert!(!m.supports("stream", "movie", "kitsu:1"));
    assert!(!m.supports("stream", "tv", "tt1"));
    assert!(!m.supports("subtitles", "movie", "tt1"));
}

#[test]
fn full_resource_overrides_types_and_prefixes() {
    let m: Manifest = serde_json::from_str(
        r#"{"id":"x","name":"x","types":["movie"],"resources":[
            {"name":"stream","types":["series"],"idPrefixes":["kitsu:"]}]}"#,
    )
    .unwrap();
    assert!(m.supports("stream", "series", "kitsu:5"));
    assert!(!m.supports("stream", "movie", "kitsu:5"));
    assert!(!m.supports("stream", "series", "tt1"));
}

#[test]
fn cinemeta_manifest_catalogs_and_meta_support() {
    let m: Manifest = serde_json::from_str(CINEMETA_MANIFEST).unwrap();
    assert!(m.supports("meta", "movie", "tt0133093"));
    assert!(m.supports("meta", "series", "tt0944947"));
    assert!(!m.supports("meta", "movie", "kitsu:1"));

    let top = m.supports_catalog("movie", "top").unwrap();
    assert!(top.is_browsable());
    assert!(top.is_searchable());
    assert_eq!(top.genres(), &["Action", "Drama", "Sci-Fi"]);

    let search_only = m.supports_catalog("movie", "search_only").unwrap();
    assert!(!search_only.is_browsable());
    assert!(search_only.is_searchable());
    assert!(search_only.requires_extra("search"));

    assert_eq!(m.browsable_catalogs().count(), 2);
    assert_eq!(m.searchable_catalogs().count(), 3);
}

#[test]
fn parses_catalog_response_with_mixed_types_and_nulls() {
    let page = parse_catalog(CATALOG_MOVIES.as_bytes()).unwrap();
    assert_eq!(page.has_more, Some(true));
    assert_eq!(page.metas.len(), 2);
    assert_eq!(page.metas[0].name, "The Matrix");
    assert_eq!(page.metas[0].imdb_rating.as_deref(), Some("8.7"));
    assert_eq!(page.metas[0].genres, vec!["Action", "Sci-Fi"]);
    // Numeric imdbRating and null genres are tolerated cleanly.
    assert_eq!(page.metas[1].imdb_rating.as_deref(), Some("8.7"));
    assert!(page.metas[1].genres.is_empty());
}

#[test]
fn parses_series_meta_with_videos_and_trailers() {
    let meta = parse_meta(META_SERIES.as_bytes()).unwrap().unwrap();
    assert_eq!(meta.id, "tt0944947");
    assert_eq!(meta.videos.len(), 2);
    assert_eq!(meta.videos[0].display_title(), Some("Winter Is Coming"));
    assert_eq!(meta.videos[1].display_title(), Some("The Kingsroad"));
    assert!(meta.director.is_empty());
    assert_eq!(meta.trailers.len(), 1);
    assert_eq!(meta.trailers[0].source, "rlR4PJn8b8I");
    assert!(parse_meta(br#"{"meta": null}"#).unwrap().is_none());
}

#[test]
fn normalizes_urls() {
    let (manifest, base) =
        normalize_manifest_url("stremio://addon.example.org/cfg=a|b/manifest.json").unwrap();
    assert_eq!(
        manifest.as_str(),
        "https://addon.example.org/cfg=a|b/manifest.json"
    );
    assert_eq!(base.as_str(), "https://addon.example.org/cfg=a|b");
    assert!(normalize_manifest_url("https://example.org/foo").is_err());
    assert!(normalize_manifest_url("ftp://example.org/manifest.json").is_err());
}

#[test]
fn builds_stream_meta_and_catalog_urls() {
    let (_, base) = normalize_manifest_url("https://a.example/sub/manifest.json").unwrap();
    assert_eq!(
        stream_url(&base, "series", "tt1:1:2").unwrap().as_str(),
        "https://a.example/sub/stream/series/tt1:1:2.json"
    );
    assert_eq!(
        meta_url(&base, "series", "tt0944947").unwrap().as_str(),
        "https://a.example/sub/meta/series/tt0944947.json"
    );
    assert_eq!(
        catalog_url(&base, "movie", "top", &CatalogQuery::default())
            .unwrap()
            .as_str(),
        "https://a.example/sub/catalog/movie/top.json"
    );
    assert_eq!(
        catalog_url(
            &base,
            "movie",
            "top",
            &CatalogQuery {
                search: Some("star & wars".into()),
                genre: None,
                skip: Some(20),
            },
        )
        .unwrap()
        .as_str(),
        "https://a.example/sub/catalog/movie/top/search=star%20%26%20wars&skip=20.json"
    );

    let (_, root) = normalize_manifest_url("http://127.0.0.1:7000/manifest.json").unwrap();
    assert_eq!(
        stream_url(&root, "movie", "tt1").unwrap().as_str(),
        "http://127.0.0.1:7000/stream/movie/tt1.json"
    );
    assert_eq!(
        catalog_url(
            &root,
            "movie",
            "top",
            &CatalogQuery {
                search: None,
                genre: Some("Sci-Fi".into()),
                skip: None,
            },
        )
        .unwrap()
        .as_str(),
        "http://127.0.0.1:7000/catalog/movie/top/genre=Sci-Fi.json"
    );
}
