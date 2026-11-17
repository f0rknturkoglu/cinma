-- 0004: Profile-scoped library and watch progress (docs/adr/0010).

CREATE TABLE library_item (
    profile_id  INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    meta_id     TEXT    NOT NULL,
    media_type  TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    poster      TEXT,
    added_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_id, meta_id, media_type)
) STRICT, WITHOUT ROWID;

CREATE INDEX library_by_added ON library_item (profile_id, added_at DESC);

CREATE TABLE watch_progress (
    profile_id  INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    video_id    TEXT    NOT NULL,
    meta_id     TEXT    NOT NULL,
    media_type  TEXT    NOT NULL,
    title       TEXT    NOT NULL DEFAULT '',
    artwork     TEXT,
    season      INTEGER,
    episode     INTEGER,
    position_s  REAL    NOT NULL,
    duration_s  REAL    NOT NULL,
    watched     INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0, 1)),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_id, video_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX watch_progress_continue ON watch_progress (profile_id, watched, updated_at DESC);
CREATE INDEX watch_progress_meta ON watch_progress (profile_id, meta_id);
