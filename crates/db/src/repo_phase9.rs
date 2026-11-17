//! Repositories for Phase 9 integrations: skip segment TTL cache and Live TV/IPTV.

use rusqlite::{OptionalExtension, params};

use crate::{Database, DbError, ProfileId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipCacheRow {
    pub cache_key: String,
    pub segments_json: String,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IptvSourceRow {
    pub id: i64,
    pub profile_id: ProfileId,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    pub epg_url: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIptvChannel {
    pub stream_id: String,
    pub name: String,
    pub group_title: Option<String>,
    pub logo_url: Option<String>,
    pub stream_url: String,
    pub epg_channel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IptvChannelRow {
    pub id: i64,
    pub source_id: i64,
    pub profile_id: ProfileId,
    pub stream_id: String,
    pub name: String,
    pub group_title: Option<String>,
    pub logo_url: Option<String>,
    pub stream_url: String,
    pub epg_channel_id: Option<String>,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IptvEpgRow {
    pub channel_id: String,
    pub start_ts: i64,
    pub stop_ts: i64,
    pub title: String,
    pub description: Option<String>,
}

impl Database {
    pub fn get_skip_segment_cache(
        &self,
        cache_key: &str,
        min_fetched_at: i64,
    ) -> Result<Option<SkipCacheRow>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT cache_key, segments_json, fetched_at
                 FROM skip_segment_cache
                 WHERE cache_key = ?1 AND fetched_at >= ?2",
                params![cache_key, min_fetched_at],
                |r| {
                    Ok(SkipCacheRow {
                        cache_key: r.get(0)?,
                        segments_json: r.get(1)?,
                        fetched_at: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_skip_segment_cache(
        &self,
        cache_key: &str,
        segments_json: &str,
        fetched_at: i64,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO skip_segment_cache (cache_key, segments_json, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(cache_key) DO UPDATE SET
               segments_json = excluded.segments_json,
               fetched_at = excluded.fetched_at",
            params![cache_key, segments_json, fetched_at],
        )?;
        Ok(())
    }

    pub fn upsert_iptv_source(
        &self,
        profile: ProfileId,
        name: &str,
        kind: &str,
        url: &str,
        username: Option<&str>,
        epg_url: Option<&str>,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO iptv_source (profile_id, name, kind, url, username, epg_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![profile, name, kind, url, username, epg_url],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_iptv_sources(&self, profile: ProfileId) -> Result<Vec<IptvSourceRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, profile_id, name, kind, url, username, epg_url, created_at
             FROM iptv_source WHERE profile_id = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([profile], |r| {
                Ok(IptvSourceRow {
                    id: r.get(0)?,
                    profile_id: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    url: r.get(4)?,
                    username: r.get(5)?,
                    epg_url: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn remove_iptv_source(&self, profile: ProfileId, source_id: i64) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM iptv_source WHERE id = ?1 AND profile_id = ?2",
            params![source_id, profile],
        )?;
        Ok(())
    }

    pub fn replace_iptv_channels(
        &self,
        profile: ProfileId,
        source_id: i64,
        channels: &[NewIptvChannel],
    ) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;
        // Preserve existing favorites by stream_id across playlist refreshes.
        let mut favs = std::collections::HashSet::new();
        {
            let mut stmt = tx.prepare(
                "SELECT stream_id FROM iptv_channel
                 WHERE source_id = ?1 AND profile_id = ?2 AND is_favorite = 1",
            )?;
            let mut rows = stmt.query(params![source_id, profile])?;
            while let Some(row) = rows.next()? {
                favs.insert(row.get::<_, String>(0)?);
            }
        }
        tx.execute(
            "DELETE FROM iptv_channel WHERE source_id = ?1 AND profile_id = ?2",
            params![source_id, profile],
        )?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO iptv_channel
                 (source_id, profile_id, stream_id, name, group_title, logo_url, stream_url, epg_channel_id, is_favorite)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for ch in channels {
                let fav = i64::from(favs.contains(&ch.stream_id));
                ins.execute(params![
                    source_id,
                    profile,
                    ch.stream_id,
                    ch.name,
                    ch.group_title,
                    ch.logo_url,
                    ch.stream_url,
                    ch.epg_channel_id,
                    fav,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_iptv_channels(
        &self,
        profile: ProfileId,
        group: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<IptvChannelRow>, DbError> {
        let q_like = query
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, profile_id, stream_id, name, group_title, logo_url, stream_url, epg_channel_id, is_favorite
             FROM iptv_channel
             WHERE profile_id = ?1
               AND (?2 IS NULL OR group_title = ?2)
               AND (?3 IS NULL OR name LIKE ?3 COLLATE NOCASE)
             ORDER BY is_favorite DESC, name ASC",
        )?;
        let rows = stmt
            .query_map(params![profile, group, q_like], |r| {
                Ok(IptvChannelRow {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    profile_id: r.get(2)?,
                    stream_id: r.get(3)?,
                    name: r.get(4)?,
                    group_title: r.get(5)?,
                    logo_url: r.get(6)?,
                    stream_url: r.get(7)?,
                    epg_channel_id: r.get(8)?,
                    is_favorite: r.get::<_, i64>(9)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_iptv_channel_favorite(
        &self,
        profile: ProfileId,
        channel_id: i64,
        favorite: bool,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE iptv_channel SET is_favorite = ?3 WHERE id = ?1 AND profile_id = ?2",
            params![channel_id, profile, i64::from(favorite)],
        )?;
        Ok(())
    }

    pub fn upsert_iptv_epg(
        &self,
        rows: &[IptvEpgRow],
        purge_before_ts: i64,
    ) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM iptv_epg WHERE stop_ts < ?1",
            params![purge_before_ts],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO iptv_epg (channel_id, start_ts, stop_ts, title, description)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(channel_id, start_ts) DO UPDATE SET
                   stop_ts = excluded.stop_ts,
                   title = excluded.title,
                   description = excluded.description",
            )?;
            for r in rows {
                stmt.execute(params![
                    r.channel_id,
                    r.start_ts,
                    r.stop_ts,
                    r.title,
                    r.description
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn current_and_next_epg(
        &self,
        channel_id: &str,
        now_ts: i64,
    ) -> Result<Vec<IptvEpgRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_id, start_ts, stop_ts, title, description
             FROM iptv_epg
             WHERE channel_id = ?1 AND stop_ts >= ?2
             ORDER BY start_ts ASC
             LIMIT 2",
        )?;
        let rows = stmt
            .query_map(params![channel_id, now_ts], |r| {
                Ok(IptvEpgRow {
                    channel_id: r.get(0)?,
                    start_ts: r.get(1)?,
                    stop_ts: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
