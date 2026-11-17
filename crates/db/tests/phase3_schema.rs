#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_db::{Database, IdMapEntry, MediaKind};

#[test]
fn id_map_merges_by_imdb_and_tmdb() {
    let db = Database::open_in_memory().unwrap();
    db.upsert_id_map(&IdMapEntry {
        media_kind: MediaKind::Movie,
        imdb_id: Some("tt0133093".into()),
        tmdb_id: Some(603),
        tvdb_id: None,
        trakt_id: None,
        simkl_id: None,
        mal_id: None,
        anilist_id: None,
        kitsu_id: None,
        season: None,
        episode: None,
    })
    .unwrap();

    // Update via tmdb_id to enrich with trakt_id without losing imdb_id.
    db.upsert_id_map(&IdMapEntry {
        media_kind: MediaKind::Movie,
        imdb_id: None,
        tmdb_id: Some(603),
        tvdb_id: None,
        trakt_id: Some(481),
        simkl_id: None,
        mal_id: None,
        anilist_id: None,
        kitsu_id: None,
        season: None,
        episode: None,
    })
    .unwrap();

    let by_imdb = db.find_id_map_by_imdb(MediaKind::Movie, "tt0133093").unwrap().unwrap();
    assert_eq!(by_imdb.tmdb_id, Some(603));
    assert_eq!(by_imdb.trakt_id, Some(481));
    assert!(db.find_id_map_by_tmdb(MediaKind::Movie, 603).unwrap().is_some());
}

#[test]
fn meta_cache_respects_expiry_and_prunes() {
    let db = Database::open_in_memory().unwrap();
    db.put_meta_cache("tmdb:movie:603:tr-TR", "tmdb", r#"{"title":"Matrix"}"#, 1000, 500)
        .unwrap();
    assert_eq!(
        db.get_meta_cache("tmdb:movie:603:tr-TR", 999).unwrap().as_deref(),
        Some(r#"{"title":"Matrix"}"#)
    );
    assert_eq!(db.get_meta_cache("tmdb:movie:603:tr-TR", 1000).unwrap(), None);
    assert_eq!(db.prune_expired_meta_cache(1000).unwrap(), 1);
}

#[test]
fn library_and_progress_are_profile_isolated() {
    let mut db = Database::open_in_memory().unwrap();
    let p2 = db.create_profile_with_avatar("Misafir", Some("M")).unwrap();
    assert_eq!(db.profiles().unwrap().len(), 2);

    db.add_library_item(1, "tt0944947", "series", "Game of Thrones", Some("https://img/p.jpg"))
        .unwrap();
    assert!(db.is_in_library(1, "tt0944947", "series").unwrap());
    assert!(!db.is_in_library(p2, "tt0944947", "series").unwrap());
    assert_eq!(db.library_items(1).unwrap().len(), 1);
    assert!(db.library_items(p2).unwrap().is_empty());

    // Two episodes of the same series in profile 1: continue_watching keeps only the latest unwatched.
    db.upsert_watch_progress(
        1,
        "tt0944947:1:1",
        "tt0944947",
        "series",
        "Game of Thrones",
        Some("https://img/bg.jpg"),
        Some(1),
        Some(1),
        3200.0,
        3300.0,
        true,
    )
    .unwrap();
    db.upsert_watch_progress(
        1,
        "tt0944947:1:2",
        "tt0944947",
        "series",
        "Game of Thrones",
        Some("https://img/bg.jpg"),
        Some(1),
        Some(2),
        900.0,
        3300.0,
        false,
    )
    .unwrap();

    let cw1 = db.continue_watching(1, 10).unwrap();
    assert_eq!(cw1.len(), 1);
    assert_eq!(cw1[0].video_id, "tt0944947:1:2");
    assert_eq!(cw1[0].season, Some(1));
    assert_eq!(cw1[0].episode, Some(2));
    assert!(db.continue_watching(p2, 10).unwrap().is_empty());

    // Addon reorder test.
    db.upsert_addon(1, "a", "https://a/manifest.json", "{}").unwrap();
    db.upsert_addon(1, "b", "https://b/manifest.json", "{}").unwrap();
    db.reorder_addons(&["b".into(), "a".into()]).unwrap();
    let ordered = db.addons_for_profile(1).unwrap();
    assert_eq!(ordered[0].id, "b");
    assert_eq!(ordered[1].id, "a");
}
