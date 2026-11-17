-- 0003: Cross-provider ID map and TTL metadata cache (docs/adr/0010).

CREATE TABLE id_map (
    id          INTEGER PRIMARY KEY,
    media_kind  TEXT    NOT NULL CHECK (media_kind IN ('movie', 'series', 'episode')),
    imdb_id     TEXT,
    tmdb_id     INTEGER,
    tvdb_id     INTEGER,
    trakt_id    INTEGER,
    simkl_id    INTEGER,
    mal_id      INTEGER,
    anilist_id  INTEGER,
    kitsu_id    INTEGER,
    season      INTEGER,
    episode     INTEGER,
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE UNIQUE INDEX id_map_imdb ON id_map (media_kind, imdb_id) WHERE imdb_id IS NOT NULL;
CREATE UNIQUE INDEX id_map_tmdb ON id_map (media_kind, tmdb_id) WHERE tmdb_id IS NOT NULL;
CREATE UNIQUE INDEX id_map_tvdb ON id_map (media_kind, tvdb_id) WHERE tvdb_id IS NOT NULL;
CREATE INDEX id_map_trakt ON id_map (media_kind, trakt_id) WHERE trakt_id IS NOT NULL;
CREATE INDEX id_map_simkl ON id_map (media_kind, simkl_id) WHERE simkl_id IS NOT NULL;
CREATE INDEX id_map_mal ON id_map (media_kind, mal_id) WHERE mal_id IS NOT NULL;
CREATE INDEX id_map_anilist ON id_map (media_kind, anilist_id) WHERE anilist_id IS NOT NULL;
CREATE INDEX id_map_kitsu ON id_map (media_kind, kitsu_id) WHERE kitsu_id IS NOT NULL;

CREATE TABLE meta_cache (
    cache_key   TEXT    PRIMARY KEY,
    source      TEXT    NOT NULL,
    json        TEXT    NOT NULL,
    expires_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT, WITHOUT ROWID;

CREATE INDEX meta_cache_expires ON meta_cache (expires_at);
