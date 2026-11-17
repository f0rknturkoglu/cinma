-- Migration 0005: disk policy & keep rules across BlobStore and librqbit downloads.

CREATE TABLE disk_entry (
    entry_key TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('blob', 'torrent')),
    meta_id TEXT,
    media_type TEXT,
    path TEXT NOT NULL,
    bytes INTEGER NOT NULL DEFAULT 0,
    last_accessed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1))
) STRICT;

CREATE INDEX idx_disk_entry_lru
    ON disk_entry(pinned, last_accessed_at ASC);

CREATE TABLE keep_rule (
    profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    meta_id TEXT NOT NULL,
    media_type TEXT NOT NULL,
    keep_watched INTEGER NOT NULL CHECK (keep_watched IN (0, 1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_id, meta_id, media_type)
) STRICT;
