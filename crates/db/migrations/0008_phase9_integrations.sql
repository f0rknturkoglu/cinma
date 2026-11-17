-- Migration 0008: Phase 9 integrations (skip segment TTL cache & Live TV/IPTV).

CREATE TABLE skip_segment_cache (
    cache_key TEXT PRIMARY KEY,
    segments_json TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
) STRICT;

CREATE TABLE iptv_source (
    id INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('m3u', 'xtream')),
    url TEXT NOT NULL,
    username TEXT,
    epg_url TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE iptv_channel (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES iptv_source(id) ON DELETE CASCADE,
    profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
    stream_id TEXT NOT NULL,
    name TEXT NOT NULL,
    group_title TEXT,
    logo_url TEXT,
    stream_url TEXT NOT NULL,
    epg_channel_id TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1))
) STRICT;

CREATE INDEX idx_iptv_channel_profile ON iptv_channel(profile_id, is_favorite DESC, name ASC);
CREATE INDEX idx_iptv_channel_source ON iptv_channel(source_id);

CREATE TABLE iptv_epg (
    channel_id TEXT NOT NULL,
    start_ts INTEGER NOT NULL,
    stop_ts INTEGER NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    PRIMARY KEY (channel_id, start_ts)
) STRICT;

CREATE INDEX idx_iptv_epg_stop ON iptv_epg(stop_ts);
