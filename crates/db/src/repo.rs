//! Typed accessors. Every user-data query is scoped by `profile_id`.

use rusqlite::{OptionalExtension, Row, params};

use crate::{Database, DbError};

pub type ProfileId = i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: ProfileId,
    pub name: String,
    pub avatar: Option<String>,
    pub has_pin: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AddonRow {
    pub id: String,
    pub transport_url: String,
    pub manifest_json: String,
    pub position: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadKind {
    Torrent,
    Http,
    Ytdlp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Active,
    Paused,
    Completed,
    Failed,
}

macro_rules! text_enum {
    ($ty:ty { $($variant:ident => $text:literal),* $(,)? }) => {
        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),* }
            }
            pub fn parse(s: &str) -> Result<Self, DbError> {
                match s { $($text => Ok(Self::$variant),)* other => Err(DbError::Invalid(other.to_owned())) }
            }
        }
    };
}
text_enum!(DownloadKind { Torrent => "torrent", Http => "http", Ytdlp => "ytdlp" });
text_enum!(DownloadState {
    Queued => "queued", Active => "active", Paused => "paused",
    Completed => "completed", Failed => "failed",
});

#[derive(Debug, Clone, PartialEq)]
pub struct NewDownload {
    pub profile_id: ProfileId,
    pub kind: DownloadKind,
    pub title: String,
    pub source_json: String,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRow {
    pub id: i64,
    pub profile_id: ProfileId,
    pub kind: DownloadKind,
    pub title: String,
    pub source_json: String,
    pub state: DownloadState,
    pub priority: i64,
    pub blob_id: Option<String>,
    pub bytes_total: Option<u64>,
    pub bytes_done: u64,
    pub error: Option<String>,
}

const DOWNLOAD_COLUMNS: &str = "id, profile_id, kind, title, source_json, state, priority, \
                                blob_id, bytes_total, bytes_done, error";

fn download_from_row(row: &Row<'_>) -> rusqlite::Result<(DownloadRow, String, String)> {
    Ok((
        DownloadRow {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            kind: DownloadKind::Torrent, // replaced below
            title: row.get(3)?,
            source_json: row.get(4)?,
            state: DownloadState::Queued, // replaced below
            priority: row.get(6)?,
            blob_id: row.get(7)?,
            bytes_total: row.get::<_, Option<i64>>(8)?.map(|v| v.max(0) as u64),
            bytes_done: row.get::<_, i64>(9)?.max(0) as u64,
            error: row.get(10)?,
        },
        row.get(2)?,
        row.get(5)?,
    ))
}

fn finish(raw: (DownloadRow, String, String)) -> Result<DownloadRow, DbError> {
    let (mut row, kind, state) = raw;
    row.kind = DownloadKind::parse(&kind)?;
    row.state = DownloadState::parse(&state)?;
    Ok(row)
}

impl Database {
    // ---- app settings (global) ----

    pub fn get_app_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM app_setting WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO app_setting (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn delete_app_setting(&self, key: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM app_setting WHERE key = ?1", [key])?;
        Ok(())
    }

    /// All global settings whose key starts with `prefix`.
    pub fn app_settings_with_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, value FROM app_setting WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key",
        )?;
        let rows = stmt.query_map([prefix], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---- profile settings ----

    pub fn get_profile_setting(
        &self,
        profile: ProfileId,
        key: &str,
    ) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM profile_setting WHERE profile_id = ?1 AND key = ?2",
                params![profile, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_profile_setting(
        &self,
        profile: ProfileId,
        key: &str,
        value: &str,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO profile_setting (profile_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id, key) DO UPDATE SET value = excluded.value",
            params![profile, key, value],
        )?;
        Ok(())
    }

    // ---- profiles ----

    pub fn profiles(&self) -> Result<Vec<ProfileRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, avatar, pin_hash IS NOT NULL FROM profile ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ProfileRow {
                id: r.get(0)?,
                name: r.get(1)?,
                avatar: r.get(2)?,
                has_pin: r.get::<_, i64>(3)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn profile_exists(&self, profile: ProfileId) -> Result<bool, DbError> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM profile WHERE id = ?1", [profile], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn create_profile(&self, name: &str) -> Result<ProfileId, DbError> {
        self.create_profile_with_avatar(name, None)
    }

    pub fn create_profile_with_avatar(
        &self,
        name: &str,
        avatar: Option<&str>,
    ) -> Result<ProfileId, DbError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(DbError::Invalid("profile name cannot be empty".into()));
        }
        self.conn.execute(
            "INSERT INTO profile (name, avatar) VALUES (?1, ?2)",
            params![trimmed, avatar],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_profile(&self, id: ProfileId, name: &str) -> Result<(), DbError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(DbError::Invalid("profile name cannot be empty".into()));
        }
        self.conn.execute(
            "UPDATE profile SET name = ?2 WHERE id = ?1",
            params![id, trimmed],
        )?;
        Ok(())
    }

    pub fn delete_profile(&self, id: ProfileId) -> Result<(), DbError> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM profile", [], |r| r.get(0))?;
        if count <= 1 {
            return Err(DbError::Invalid("cannot delete the last profile".into()));
        }
        self.conn
            .execute("DELETE FROM profile WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_profile_pin_hash(
        &self,
        id: ProfileId,
        pin_hash: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE profile SET pin_hash = ?2 WHERE id = ?1",
            params![id, pin_hash],
        )?;
        Ok(())
    }

    pub fn profile_pin_hash(&self, id: ProfileId) -> Result<Option<String>, DbError> {
        Ok(self
            .conn
            .query_row("SELECT pin_hash FROM profile WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten())
    }

    // ---- addons ----

    /// Installed addons in user order, with this profile's enabled flag.
    pub fn addons_for_profile(&self, profile: ProfileId) -> Result<Vec<AddonRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT a.id, a.transport_url, a.manifest_json, a.position,
                    COALESCE(pa.enabled, 0)
             FROM addon a
             LEFT JOIN profile_addon pa ON pa.addon_id = a.id AND pa.profile_id = ?1
             ORDER BY a.position, a.installed_at",
        )?;
        let rows = stmt.query_map([profile], |r| {
            Ok(AddonRow {
                id: r.get(0)?,
                transport_url: r.get(1)?,
                manifest_json: r.get(2)?,
                position: r.get(3)?,
                enabled: r.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Installs or refreshes an addon (keeps its position) and enables it for
    /// `profile`.
    pub fn upsert_addon(
        &mut self,
        profile: ProfileId,
        id: &str,
        transport_url: &str,
        manifest_json: &str,
    ) -> Result<(), DbError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO addon (id, transport_url, manifest_json, position)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position) + 1, 0) FROM addon))
             ON CONFLICT(id) DO UPDATE SET transport_url = excluded.transport_url,
                                           manifest_json = excluded.manifest_json",
            params![id, transport_url, manifest_json],
        )?;
        tx.execute(
            "INSERT INTO profile_addon (profile_id, addon_id, enabled) VALUES (?1, ?2, 1)
             ON CONFLICT(profile_id, addon_id) DO UPDATE SET enabled = 1",
            params![profile, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_addon_enabled(
        &self,
        profile: ProfileId,
        id: &str,
        enabled: bool,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO profile_addon (profile_id, addon_id, enabled) VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id, addon_id) DO UPDATE SET enabled = excluded.enabled",
            params![profile, id, enabled as i64],
        )?;
        Ok(())
    }

    /// Uninstalls globally (all profiles).
    pub fn remove_addon(&self, id: &str) -> Result<(), DbError> {
        self.conn.execute("DELETE FROM addon WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Updates addon `position` in user order.
    pub fn reorder_addons(&mut self, ordered_ids: &[String]) -> Result<(), DbError> {
        let tx = self.conn.transaction()?;
        for (pos, id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE addon SET position = ?2 WHERE id = ?1",
                params![id, pos as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---- downloads ----

    pub fn insert_download(&self, d: &NewDownload) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO download (profile_id, kind, title, source_json, priority)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                d.profile_id,
                d.kind.as_str(),
                d.title,
                d.source_json,
                d.priority
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn download(&self, profile: ProfileId, id: i64) -> Result<Option<DownloadRow>, DbError> {
        let sql =
            format!("SELECT {DOWNLOAD_COLUMNS} FROM download WHERE profile_id = ?1 AND id = ?2");
        self.conn
            .query_row(&sql, params![profile, id], download_from_row)
            .optional()?
            .map(finish)
            .transpose()
    }

    pub fn downloads(&self, profile: ProfileId) -> Result<Vec<DownloadRow>, DbError> {
        let sql = format!(
            "SELECT {DOWNLOAD_COLUMNS} FROM download WHERE profile_id = ?1 ORDER BY priority DESC, id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([profile], download_from_row)?;
        rows.map(|r| finish(r?)).collect()
    }

    /// Queued items across profiles, best first (the queue is global).
    pub fn next_queued_downloads(&self, limit: usize) -> Result<Vec<DownloadRow>, DbError> {
        let sql = format!(
            "SELECT {DOWNLOAD_COLUMNS} FROM download WHERE state = 'queued'
             ORDER BY priority DESC, id LIMIT ?1"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([limit as i64], download_from_row)?;
        rows.map(|r| finish(r?)).collect()
    }

    pub fn count_downloads_in_state(&self, state: DownloadState) -> Result<usize, DbError> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM download WHERE state = ?1",
            [state.as_str()],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    pub fn set_download_state(
        &self,
        id: i64,
        state: DownloadState,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE download SET state = ?2, error = ?3, updated_at = unixepoch() WHERE id = ?1",
            params![id, state.as_str(), error],
        )?;
        Ok(())
    }

    pub fn set_download_blob(
        &self,
        id: i64,
        blob_id: &str,
        bytes_total: Option<u64>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE download SET blob_id = ?2, bytes_total = ?3, updated_at = unixepoch() WHERE id = ?1",
            params![id, blob_id, bytes_total.map(|v| v as i64)],
        )?;
        Ok(())
    }

    /// Records durable progress. Callers must checkpoint the blob first so the
    /// row never claims bytes the blob can't vouch for after a crash.
    pub fn set_download_progress(&self, id: i64, bytes_done: u64) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE download SET bytes_done = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, bytes_done as i64],
        )?;
        Ok(())
    }

    /// After a crash or restart nothing is running: active → queued.
    pub fn requeue_active_downloads(&self) -> Result<usize, DbError> {
        Ok(self.conn.execute(
            "UPDATE download SET state = 'queued', updated_at = unixepoch() WHERE state = 'active'",
            [],
        )?)
    }

    pub fn has_pending_downloads(&self) -> Result<bool, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM download WHERE state IN ('queued', 'active') LIMIT 1",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn all_active_or_queued_blob_ids(&self) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT blob_id FROM download WHERE blob_id IS NOT NULL AND state IN ('queued', 'active', 'paused')",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete_download(&self, profile: ProfileId, id: i64) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM download WHERE profile_id = ?1 AND id = ?2",
            params![profile, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn new_dl(profile: ProfileId, title: &str, priority: i64) -> NewDownload {
        NewDownload {
            profile_id: profile,
            kind: DownloadKind::Http,
            title: title.into(),
            source_json: "{}".into(),
            priority,
        }
    }

    #[test]
    fn addons_are_global_but_enabled_per_profile() {
        let mut db = Database::open_in_memory().unwrap();
        let second = db.create_profile("Second").unwrap();
        db.upsert_addon(1, "a", "https://a/manifest.json", "{}")
            .unwrap();
        db.upsert_addon(1, "b", "https://b/manifest.json", "{}")
            .unwrap();
        let p1 = db.addons_for_profile(1).unwrap();
        assert_eq!(
            p1.iter()
                .map(|a| (a.id.as_str(), a.position, a.enabled))
                .collect::<Vec<_>>(),
            vec![("a", 0, true), ("b", 1, true)]
        );
        // Visible to the other profile, but not enabled there.
        assert!(
            db.addons_for_profile(second)
                .unwrap()
                .iter()
                .all(|a| !a.enabled)
        );
        db.set_addon_enabled(second, "b", true).unwrap();
        db.set_addon_enabled(1, "a", false).unwrap();
        assert!(!db.addons_for_profile(1).unwrap()[0].enabled);
        assert!(db.addons_for_profile(second).unwrap()[1].enabled);
        // Refresh keeps position; remove cascades.
        db.upsert_addon(1, "a", "https://a2/manifest.json", "{\"v\":2}")
            .unwrap();
        let a = &db.addons_for_profile(1).unwrap()[0];
        assert_eq!(
            (a.position, a.transport_url.as_str()),
            (0, "https://a2/manifest.json")
        );
        db.remove_addon("a").unwrap();
        assert_eq!(db.addons_for_profile(second).unwrap().len(), 1);
    }

    #[test]
    fn profile_rows_are_isolated() {
        let db = Database::open_in_memory().unwrap();
        let other = db.create_profile("Other").unwrap();
        let mine = db.insert_download(&new_dl(1, "mine", 0)).unwrap();
        let theirs = db.insert_download(&new_dl(other, "theirs", 0)).unwrap();
        assert_eq!(
            db.downloads(1)
                .unwrap()
                .iter()
                .map(|d| d.id)
                .collect::<Vec<_>>(),
            vec![mine]
        );
        assert!(db.download(1, theirs).unwrap().is_none());
        db.delete_download(1, theirs).unwrap();
        assert!(
            db.download(other, theirs).unwrap().is_some(),
            "can't delete across profiles"
        );
        db.set_profile_setting(1, "lang", "tr").unwrap();
        assert_eq!(db.get_profile_setting(other, "lang").unwrap(), None);
    }

    #[test]
    fn download_lifecycle_and_queue_order() {
        let db = Database::open_in_memory().unwrap();
        let low = db.insert_download(&new_dl(1, "low", 0)).unwrap();
        let high = db.insert_download(&new_dl(1, "high", 5)).unwrap();
        let next: Vec<i64> = db
            .next_queued_downloads(10)
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(next, vec![high, low]);
        db.set_download_state(high, DownloadState::Active, None)
            .unwrap();
        db.set_download_blob(high, "blob1", Some(100)).unwrap();
        db.set_download_progress(high, 40).unwrap();
        let row = db.download(1, high).unwrap().unwrap();
        assert_eq!(
            (row.state, row.bytes_done, row.bytes_total),
            (DownloadState::Active, 40, Some(100))
        );
        assert_eq!(db.requeue_active_downloads().unwrap(), 1);
        assert_eq!(
            db.download(1, high).unwrap().unwrap().state,
            DownloadState::Queued
        );
    }

    #[test]
    fn settings_prefix_listing() {
        let db = Database::open_in_memory().unwrap();
        db.set_app_setting("keybinding.a", "x").unwrap();
        db.set_app_setting("keybinding.b", "y").unwrap();
        db.set_app_setting("feature.torrent", "0").unwrap();
        assert_eq!(db.app_settings_with_prefix("keybinding.").unwrap().len(), 2);
        db.delete_app_setting("keybinding.a").unwrap();
        assert_eq!(db.app_settings_with_prefix("keybinding.").unwrap().len(), 1);
    }
}
