-- 0001: scaffold only. Real schema lands in Phase 2 (see PROGRESS.md).
-- Every user-data table carries profile_id (CLAUDE.md §5.10).

CREATE TABLE profile (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

INSERT INTO profile (id, name) VALUES (1, 'Default');

-- Global app settings (feature toggles live here).
CREATE TABLE app_setting (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
) STRICT, WITHOUT ROWID;
