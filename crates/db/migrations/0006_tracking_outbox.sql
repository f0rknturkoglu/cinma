-- Migration 0006: tracking outbox & adapter cursor state (Phase 6, §5.14).

CREATE TABLE sync_outbox (
    id INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    adapter TEXT NOT NULL CHECK (adapter IN ('trakt', 'simkl', 'anilist', 'mal')),
    action TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE INDEX idx_sync_outbox_due
    ON sync_outbox(next_retry_at ASC, id ASC);

CREATE TABLE sync_adapter_state (
    profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    adapter TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    last_sync_at INTEGER,
    cursor_json TEXT,
    PRIMARY KEY (profile_id, adapter)
) STRICT;
