#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_db::{Database, DiskEntryRow, DiskKind, SCHEMA_VERSION};

#[test]
fn disk_policy_and_outbox_migrations_work() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cinma.sqlite3");
    let db = Database::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);

    db.upsert_disk_entry(&DiskEntryRow {
        entry_key: "blob:abc".into(),
        kind: DiskKind::Blob,
        meta_id: Some("tt123".into()),
        media_type: Some("movie".into()),
        path: "/tmp/abc.blob".into(),
        bytes: 1024,
        last_accessed_at: 0,
        pinned: false,
    })
    .unwrap();
    db.set_keep_rule(1, "tt123", "movie", true).unwrap();
    assert_eq!(db.is_item_kept(1, "tt123", "movie").unwrap(), Some(true));
    assert!(db.any_profile_keeps_item("tt123", "movie").unwrap());
    assert_eq!(db.disk_entries_lru().unwrap().len(), 1);

    let out_id = db
        .enqueue_outbox(1, "trakt", "scrobble", r#"{"imdb":"tt123"}"#)
        .unwrap();
    assert_eq!(db.due_outbox_rows(i64::MAX, 10).unwrap().len(), 1);
    db.schedule_outbox_retry(out_id, 1, 9_999_999_999, "timeout")
        .unwrap();
    assert!(db.due_outbox_rows(1_000_000, 10).unwrap().is_empty());
    db.delete_outbox_row(out_id).unwrap();
    assert_eq!(db.outbox_count(1).unwrap(), 0);

    db.set_profile_pin_hash(1, Some("pbkdf2$salt$hash"))
        .unwrap();
    assert_eq!(
        db.profile_pin_hash(1).unwrap().as_deref(),
        Some("pbkdf2$salt$hash")
    );

    let backup_path = dir.path().join("backup.sqlite3");
    db.backup_to(&backup_path).unwrap();
    assert_eq!(
        Database::validate_backup_file(&backup_path).unwrap(),
        SCHEMA_VERSION
    );
}
