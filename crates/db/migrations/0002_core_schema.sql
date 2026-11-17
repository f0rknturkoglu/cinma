-- 0002: Phase 2 core schema (docs/adr/0008). Conventions:
--   * user data carries profile_id (FK, ON DELETE CASCADE)
--   * STRICT tables, unix-second INTEGER timestamps
--   * payloads from outside (addon manifests, download sources) stay JSON text

ALTER TABLE profile ADD COLUMN avatar TEXT;
ALTER TABLE profile ADD COLUMN pin_hash TEXT;

-- Addons are installed globally and enabled per profile (§5.10).
CREATE TABLE addon (
    id             TEXT    PRIMARY KEY,             -- manifest id
    transport_url  TEXT    NOT NULL UNIQUE,         -- manifest URL
    manifest_json  TEXT    NOT NULL,
    position       INTEGER NOT NULL,                -- user order
    installed_at   INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE profile_addon (
    profile_id  INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    addon_id    TEXT    NOT NULL REFERENCES addon(id) ON DELETE CASCADE,
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    PRIMARY KEY (profile_id, addon_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE profile_setting (
    profile_id  INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    key         TEXT    NOT NULL,
    value       TEXT    NOT NULL,
    PRIMARY KEY (profile_id, key)
) STRICT, WITHOUT ROWID;

-- Unified download queue (Phase 5 fills in the engines).
CREATE TABLE download (
    id           INTEGER PRIMARY KEY,
    profile_id   INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL CHECK (kind IN ('torrent', 'http', 'ytdlp')),
    title        TEXT    NOT NULL,
    source_json  TEXT    NOT NULL,
    state        TEXT    NOT NULL DEFAULT 'queued'
                 CHECK (state IN ('queued', 'active', 'paused', 'completed', 'failed')),
    priority     INTEGER NOT NULL DEFAULT 0,
    blob_id      TEXT,
    bytes_total  INTEGER,
    bytes_done   INTEGER NOT NULL DEFAULT 0,
    error        TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE INDEX download_by_state ON download (profile_id, state, priority DESC, id);

-- Move the Phase 1 addon list (JSON in app_setting) into the tables and
-- enable every existing addon for the default profile.
INSERT INTO addon (id, transport_url, manifest_json, position)
SELECT json_extract(value, '$.manifest.id'),
       json_extract(value, '$.transport_url'),
       json(json_extract(value, '$.manifest')),
       CAST(key AS INTEGER)
FROM json_each((SELECT value FROM app_setting WHERE key = 'addons.v1'));

INSERT INTO profile_addon (profile_id, addon_id)
SELECT 1, id FROM addon;

DELETE FROM app_setting WHERE key = 'addons.v1';
