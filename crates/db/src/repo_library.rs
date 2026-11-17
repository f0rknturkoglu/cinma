//! Profile-scoped library and watch progress repositories.

use rusqlite::{OptionalExtension, params};

use crate::{Database, DbError, ProfileId};

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryItemRow {
    pub profile_id: ProfileId,
    pub meta_id: String,
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WatchProgressRow {
    pub profile_id: ProfileId,
    pub video_id: String,
    pub meta_id: String,
    pub media_type: String,
    pub title: String,
    pub artwork: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub position_s: f64,
    pub duration_s: f64,
    pub watched: bool,
    pub updated_at: i64,
}

impl Database {
    // ---- library ----

    pub fn add_library_item(
        &self,
        profile: ProfileId,
        meta_id: &str,
        media_type: &str,
        name: &str,
        poster: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO library_item (profile_id, meta_id, media_type, name, poster)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, meta_id, media_type) DO UPDATE SET
                 name = excluded.name,
                 poster = COALESCE(excluded.poster, library_item.poster)",
            params![profile, meta_id, media_type, name, poster],
        )?;
        Ok(())
    }

    pub fn remove_library_item(
        &self,
        profile: ProfileId,
        meta_id: &str,
        media_type: &str,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM library_item WHERE profile_id = ?1 AND meta_id = ?2 AND media_type = ?3",
            params![profile, meta_id, media_type],
        )?;
        Ok(())
    }

    pub fn is_in_library(
        &self,
        profile: ProfileId,
        meta_id: &str,
        media_type: &str,
    ) -> Result<bool, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM library_item WHERE profile_id = ?1 AND meta_id = ?2 AND media_type = ?3",
                params![profile, meta_id, media_type],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn library_items(&self, profile: ProfileId) -> Result<Vec<LibraryItemRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT profile_id, meta_id, media_type, name, poster, added_at
             FROM library_item WHERE profile_id = ?1 ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([profile], |r| {
            Ok(LibraryItemRow {
                profile_id: r.get(0)?,
                meta_id: r.get(1)?,
                media_type: r.get(2)?,
                name: r.get(3)?,
                poster: r.get(4)?,
                added_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---- watch progress ----

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_watch_progress(
        &self,
        profile: ProfileId,
        video_id: &str,
        meta_id: &str,
        media_type: &str,
        title: &str,
        artwork: Option<&str>,
        season: Option<i64>,
        episode: Option<i64>,
        position_s: f64,
        duration_s: f64,
        watched: bool,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO watch_progress
                (profile_id, video_id, meta_id, media_type, title, artwork, season, episode,
                 position_s, duration_s, watched, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, unixepoch())
             ON CONFLICT(profile_id, video_id) DO UPDATE SET
                 meta_id = excluded.meta_id,
                 media_type = excluded.media_type,
                 title = CASE WHEN excluded.title != '' THEN excluded.title ELSE watch_progress.title END,
                 artwork = COALESCE(excluded.artwork, watch_progress.artwork),
                 season = COALESCE(excluded.season, watch_progress.season),
                 episode = COALESCE(excluded.episode, watch_progress.episode),
                 position_s = excluded.position_s,
                 duration_s = CASE WHEN excluded.duration_s > 0 THEN excluded.duration_s ELSE watch_progress.duration_s END,
                 watched = excluded.watched,
                 updated_at = unixepoch()",
            params![
                profile,
                video_id,
                meta_id,
                media_type,
                title,
                artwork,
                season,
                episode,
                position_s,
                duration_s,
                watched as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_watch_progress(
        &self,
        profile: ProfileId,
        video_id: &str,
    ) -> Result<Option<WatchProgressRow>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT profile_id, video_id, meta_id, media_type, title, artwork, season, episode,
                        position_s, duration_s, watched, updated_at
                 FROM watch_progress WHERE profile_id = ?1 AND video_id = ?2",
                params![profile, video_id],
                progress_from_row,
            )
            .optional()?)
    }

    /// Most recent in-progress items (one row per `meta_id`) for the home rail.
    pub fn continue_watching(
        &self,
        profile: ProfileId,
        limit: usize,
    ) -> Result<Vec<WatchProgressRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT wp.profile_id, wp.video_id, wp.meta_id, wp.media_type, wp.title, wp.artwork,
                    wp.season, wp.episode, wp.position_s, wp.duration_s, wp.watched, wp.updated_at
             FROM watch_progress wp
             WHERE wp.profile_id = ?1
               AND wp.watched = 0
               AND wp.position_s > 0
               AND wp.duration_s > 0
               AND wp.updated_at = (
                   SELECT MAX(wp2.updated_at) FROM watch_progress wp2
                   WHERE wp2.profile_id = ?1 AND wp2.meta_id = wp.meta_id AND wp2.watched = 0
               )
             ORDER BY wp.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![profile, limit as i64], progress_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn progress_for_meta(
        &self,
        profile: ProfileId,
        meta_id: &str,
    ) -> Result<Vec<WatchProgressRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT profile_id, video_id, meta_id, media_type, title, artwork, season, episode,
                    position_s, duration_s, watched, updated_at
             FROM watch_progress WHERE profile_id = ?1 AND meta_id = ?2",
        )?;
        let rows = stmt.query_map(params![profile, meta_id], progress_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn remove_watch_progress(&self, profile: ProfileId, video_id: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM watch_progress WHERE profile_id = ?1 AND video_id = ?2",
            params![profile, video_id],
        )?;
        Ok(())
    }
}

fn progress_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WatchProgressRow> {
    Ok(WatchProgressRow {
        profile_id: r.get(0)?,
        video_id: r.get(1)?,
        meta_id: r.get(2)?,
        media_type: r.get(3)?,
        title: r.get(4)?,
        artwork: r.get(5)?,
        season: r.get(6)?,
        episode: r.get(7)?,
        position_s: r.get(8)?,
        duration_s: r.get(9)?,
        watched: r.get::<_, i64>(10)? != 0,
        updated_at: r.get(11)?,
    })
}
