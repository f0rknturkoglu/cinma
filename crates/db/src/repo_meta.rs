//! Cross-provider ID map and TTL'd metadata cache repositories.

use rusqlite::{OptionalExtension, Row, params};

use crate::{Database, DbError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Movie,
    Series,
    Episode,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Episode => "episode",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DbError> {
        match s {
            "movie" => Ok(Self::Movie),
            "series" => Ok(Self::Series),
            "episode" => Ok(Self::Episode),
            other => Err(DbError::Invalid(format!("unknown media_kind `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdMapEntry {
    pub media_kind: MediaKind,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tvdb_id: Option<i64>,
    pub trakt_id: Option<i64>,
    pub simkl_id: Option<i64>,
    pub mal_id: Option<i64>,
    pub anilist_id: Option<i64>,
    pub kitsu_id: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

const ID_MAP_COLS: &str = "media_kind, imdb_id, tmdb_id, tvdb_id, trakt_id, simkl_id, \
                           mal_id, anilist_id, kitsu_id, season, episode";

fn id_map_from_row(row: &Row<'_>) -> rusqlite::Result<(IdMapEntry, String)> {
    let kind_str: String = row.get(0)?;
    Ok((
        IdMapEntry {
            media_kind: MediaKind::Movie,
            imdb_id: row.get(1)?,
            tmdb_id: row.get(2)?,
            tvdb_id: row.get(3)?,
            trakt_id: row.get(4)?,
            simkl_id: row.get(5)?,
            mal_id: row.get(6)?,
            anilist_id: row.get(7)?,
            kitsu_id: row.get(8)?,
            season: row.get(9)?,
            episode: row.get(10)?,
        },
        kind_str,
    ))
}

fn finish_id_map(raw: (IdMapEntry, String)) -> Result<IdMapEntry, DbError> {
    let (mut entry, kind_str) = raw;
    entry.media_kind = MediaKind::parse(&kind_str)?;
    Ok(entry)
}

impl Database {
    pub fn find_id_map_by_imdb(
        &self,
        kind: MediaKind,
        imdb_id: &str,
    ) -> Result<Option<IdMapEntry>, DbError> {
        let sql =
            format!("SELECT {ID_MAP_COLS} FROM id_map WHERE media_kind = ?1 AND imdb_id = ?2");
        self.conn
            .query_row(&sql, params![kind.as_str(), imdb_id], id_map_from_row)
            .optional()?
            .map(finish_id_map)
            .transpose()
    }

    pub fn find_id_map_by_tmdb(
        &self,
        kind: MediaKind,
        tmdb_id: i64,
    ) -> Result<Option<IdMapEntry>, DbError> {
        let sql =
            format!("SELECT {ID_MAP_COLS} FROM id_map WHERE media_kind = ?1 AND tmdb_id = ?2");
        self.conn
            .query_row(&sql, params![kind.as_str(), tmdb_id], id_map_from_row)
            .optional()?
            .map(finish_id_map)
            .transpose()
    }

    /// Inserts or merges an ID mapping row matched by `imdb_id` or `tmdb_id`.
    pub fn upsert_id_map(&self, entry: &IdMapEntry) -> Result<(), DbError> {
        let kind = entry.media_kind.as_str();
        let existing_id: Option<i64> = if let Some(ref imdb) = entry.imdb_id {
            self.conn
                .query_row(
                    "SELECT id FROM id_map WHERE media_kind = ?1 AND imdb_id = ?2",
                    params![kind, imdb],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            None
        }
        .or_else(|| {
            entry.tmdb_id.and_then(|tmdb| {
                self.conn
                    .query_row(
                        "SELECT id FROM id_map WHERE media_kind = ?1 AND tmdb_id = ?2",
                        params![kind, tmdb],
                        |r| r.get(0),
                    )
                    .optional()
                    .ok()
                    .flatten()
            })
        });

        if let Some(id) = existing_id {
            self.conn.execute(
                "UPDATE id_map SET
                    imdb_id = COALESCE(?2, imdb_id),
                    tmdb_id = COALESCE(?3, tmdb_id),
                    tvdb_id = COALESCE(?4, tvdb_id),
                    trakt_id = COALESCE(?5, trakt_id),
                    simkl_id = COALESCE(?6, simkl_id),
                    mal_id = COALESCE(?7, mal_id),
                    anilist_id = COALESCE(?8, anilist_id),
                    kitsu_id = COALESCE(?9, kitsu_id),
                    season = COALESCE(?10, season),
                    episode = COALESCE(?11, episode),
                    updated_at = unixepoch()
                 WHERE id = ?1",
                params![
                    id,
                    entry.imdb_id,
                    entry.tmdb_id,
                    entry.tvdb_id,
                    entry.trakt_id,
                    entry.simkl_id,
                    entry.mal_id,
                    entry.anilist_id,
                    entry.kitsu_id,
                    entry.season,
                    entry.episode,
                ],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO id_map (media_kind, imdb_id, tmdb_id, tvdb_id, trakt_id, simkl_id,
                                     mal_id, anilist_id, kitsu_id, season, episode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    kind,
                    entry.imdb_id,
                    entry.tmdb_id,
                    entry.tvdb_id,
                    entry.trakt_id,
                    entry.simkl_id,
                    entry.mal_id,
                    entry.anilist_id,
                    entry.kitsu_id,
                    entry.season,
                    entry.episode,
                ],
            )?;
        }
        Ok(())
    }

    // ---- meta_cache ----

    pub fn get_meta_cache(&self, key: &str, now_unix: i64) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT json FROM meta_cache WHERE cache_key = ?1 AND expires_at > ?2",
                params![key, now_unix],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn put_meta_cache(
        &self,
        key: &str,
        source: &str,
        json: &str,
        expires_at: i64,
        now_unix: i64,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO meta_cache (cache_key, source, json, expires_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(cache_key) DO UPDATE SET
                 source = excluded.source,
                 json = excluded.json,
                 expires_at = excluded.expires_at,
                 updated_at = excluded.updated_at",
            params![key, source, json, expires_at, now_unix],
        )?;
        Ok(())
    }

    pub fn prune_expired_meta_cache(&self, now_unix: i64) -> Result<usize, DbError> {
        Ok(self
            .conn
            .execute("DELETE FROM meta_cache WHERE expires_at <= ?1", [now_unix])?)
    }
}
