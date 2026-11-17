#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cinma_core::{
    CatalogBatch, CatalogBatchListener, CatalogQuery, Core, CoreError, Feature, StreamBatch,
    StreamBatchListener,
};

struct MockRoute {
    status: u16,
    body: String,
    delay: Duration,
}

struct MockServer {
    base_url: String,
}

impl MockServer {
    fn start(routes: HashMap<String, MockRoute>) -> Self {
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
                    let path = first_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                            break;
                        }
                    }

                    let (status, body, delay) = match routes.get(&path) {
                        Some(r) => (r.status, r.body.as_str(), r.delay),
                        None => (404, r#"{"error":"not found"}"#, Duration::ZERO),
                    };
                    if !delay.is_zero() {
                        thread::sleep(delay);
                    }
                    let mut stream = stream;
                    let response = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                });
            }
        });

        Self { base_url: format!("http://{addr}") }
    }
}

#[test]
fn inspect_addon_does_not_persist_and_catalog_meta_search_work() {
    let mut routes = HashMap::new();
    routes.insert(
        "/cinemeta/manifest.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{
                "id": "com.linvo.cinemeta",
                "version": "3.0.0",
                "name": "Cinemeta",
                "resources": ["catalog", "meta"],
                "types": ["movie", "series"],
                "idPrefixes": ["tt"],
                "catalogs": [
                    {
                        "type": "movie",
                        "id": "top",
                        "name": "Popular",
                        "extra": [
                            {"name": "genre", "options": ["Action", "Sci-Fi"]},
                            {"name": "skip"},
                            {"name": "search"}
                        ]
                    },
                    {
                        "type": "movie",
                        "id": "search_only",
                        "name": "Search",
                        "extra": [{"name": "search", "isRequired": true}]
                    }
                ]
            }"#
            .into(),
        },
    );
    routes.insert(
        "/cinemeta/catalog/movie/top.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"metas":[{"id":"tt0133093","type":"movie","name":"The Matrix","imdbRating":8.7}],"hasMore":true}"#.into(),
        },
    );
    routes.insert(
        "/cinemeta/catalog/movie/top/genre=Sci-Fi&skip=20.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"metas":[{"id":"tt0816692","type":"movie","name":"Interstellar"}],"hasMore":false}"#.into(),
        },
    );
    routes.insert(
        "/cinemeta/catalog/movie/top/search=matrix%20reloaded.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::from_millis(80),
            body: r#"{"metas":[{"id":"tt0234215","type":"movie","name":"The Matrix Reloaded"}]}"#.into(),
        },
    );
    routes.insert(
        "/cinemeta/catalog/movie/search_only/search=matrix%20reloaded.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"metas":[{"id":"tt0234215","type":"movie","name":"The Matrix Reloaded"}]}"#.into(),
        },
    );
    routes.insert(
        "/cinemeta/meta/movie/tt0133093.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"meta":{"id":"tt0133093","type":"movie","name":"The Matrix","trailers":[{"source":"vKQi3bBA1y8","type":"Trailer"}]}}"#.into(),
        },
    );

    let server = MockServer::start(routes);
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // inspect_addon must not write to the DB.
    let preview = rt
        .block_on(core.inspect_addon(format!("{}/cinemeta/manifest.json", server.base_url)))
        .unwrap();
    assert_eq!(preview.manifest.name, "Cinemeta");
    assert!(!preview.enabled);
    assert!(core.addons().unwrap().is_empty());

    // install_addon persists and enables it.
    let installed = rt
        .block_on(core.install_addon(format!("{}/cinemeta/manifest.json", server.base_url)))
        .unwrap();
    assert!(installed.enabled);
    assert_eq!(core.catalogs().unwrap().len(), 2);

    // Default catalog page.
    let page = rt
        .block_on(core.catalog(
            "com.linvo.cinemeta".into(),
            "movie".into(),
            "top".into(),
            CatalogQuery::default(),
        ))
        .unwrap();
    assert!(page.has_more);
    assert_eq!(page.items[0].name, "The Matrix");

    // Filtered + paginated catalog page.
    let filtered = rt
        .block_on(core.catalog(
            "com.linvo.cinemeta".into(),
            "movie".into(),
            "top".into(),
            CatalogQuery {
                search: None,
                genre: Some("Sci-Fi".into()),
                skip: Some(20),
            },
        ))
        .unwrap();
    assert!(!filtered.has_more);
    assert_eq!(filtered.items[0].name, "Interstellar");

    // browse_catalogs skips search-only catalogs.
    let browsed = rt.block_on(core.browse_catalogs(Some("movie".into()), None)).unwrap();
    assert_eq!(browsed.len(), 1);
    assert_eq!(browsed[0].catalog_id, "top");

    // search_catalogs queries both searchable catalogs and emits partial batches in arrival order.
    struct CatalogRecorder(Mutex<Vec<CatalogBatch>>);
    impl CatalogBatchListener for CatalogRecorder {
        fn on_batch(&self, batch: CatalogBatch) {
            self.0.lock().unwrap().push(batch);
        }
    }
    let recorder = Arc::new(CatalogRecorder(Mutex::new(Vec::new())));
    let searched = rt
        .block_on(core.search_catalogs(
            "matrix reloaded".into(),
            Some("movie".into()),
            Some(recorder.clone()),
        ))
        .unwrap();
    assert_eq!(searched.len(), 2);
    // Final result preserves manifest catalog order (`top` then `search_only`).
    assert_eq!(searched[0].catalog_id, "top");
    assert_eq!(searched[1].catalog_id, "search_only");
    // Partial batches arrive as each finishes (`search_only` was faster than `top`).
    let batches = recorder.0.lock().unwrap();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].catalog_id, "search_only");
    assert_eq!(batches[1].catalog_id, "top");

    // meta() fetches full metadata.
    let meta = rt.block_on(core.meta("movie".into(), "tt0133093".into())).unwrap();
    assert_eq!(meta.meta.name, "The Matrix");
    assert_eq!(meta.meta.trailers[0].source, "vKQi3bBA1y8");
}

#[test]
fn streams_streaming_emits_partial_batches_before_slow_addon_and_preserves_order() {
    let mut routes = HashMap::new();
    routes.insert(
        "/slow/manifest.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"id":"addon.slow","name":"Slow Addon","version":"1.0","resources":["stream"],"types":["movie"],"idPrefixes":["tt"]}"#.into(),
        },
    );
    routes.insert(
        "/fast/manifest.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"id":"addon.fast","name":"Fast Addon","version":"1.0","resources":["stream"],"types":["movie"],"idPrefixes":["tt"]}"#.into(),
        },
    );
    routes.insert(
        "/slow/stream/movie/tt1.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::from_millis(80),
            body: r#"{"streams":[{"name":"Slow 4K","url":"https://slow.example/v.mp4"}]}"#.into(),
        },
    );
    routes.insert(
        "/fast/stream/movie/tt1.json".into(),
        MockRoute {
            status: 200,
            delay: Duration::ZERO,
            body: r#"{"streams":[{"name":"Fast 1080p","url":"https://fast.example/v.mp4"}]}"#.into(),
        },
    );

    let server = MockServer::start(routes);
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(core.install_addon(format!("{}/slow/manifest.json", server.base_url))).unwrap();
    rt.block_on(core.install_addon(format!("{}/fast/manifest.json", server.base_url))).unwrap();

    struct StreamRecorder(Mutex<Vec<StreamBatch>>);
    impl StreamBatchListener for StreamRecorder {
        fn on_batch(&self, batch: StreamBatch) {
            self.0.lock().unwrap().push(batch);
        }
    }
    let recorder = Arc::new(StreamRecorder(Mutex::new(Vec::new())));
    let all = rt
        .block_on(core.streams_streaming("movie".into(), "tt1".into(), Some(recorder.clone())))
        .unwrap();

    assert_eq!(all.len(), 2);
    // Final list keeps addon installation order: Slow Addon first, Fast Addon second.
    assert_eq!(all[0].addon_name, "Slow Addon");
    assert_eq!(all[1].addon_name, "Fast Addon");

    // Listener received Fast Addon first!
    let batches = recorder.0.lock().unwrap();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].addon_id, "addon.fast");
    assert_eq!(batches[1].addon_id, "addon.slow");
}

#[test]
fn disabled_addons_feature_rejects_catalog_and_meta_queries() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(core.set_feature_enabled(Feature::Addons, false)).unwrap();

    assert!(matches!(core.catalogs(), Err(CoreError::FeatureDisabled("addons"))));
    assert!(matches!(
        rt.block_on(core.meta("movie".into(), "tt1".into())),
        Err(CoreError::FeatureDisabled("addons"))
    ));
    assert!(matches!(
        rt.block_on(core.search_catalogs("matrix".into(), None, None)),
        Err(CoreError::FeatureDisabled("addons"))
    ));
}
