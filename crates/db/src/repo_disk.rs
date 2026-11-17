use rusqlite::{OptionalExtension, params};

use crate::{Database, DbError, ProfileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKind {
    Blob,
    Torrent,
}

impl DiskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Torrent => "torrent",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DbError> {
        match s {
            "blob" => Ok(Self::Blob),
            "torrent" => Ok(Self::Torrent),
            other => Err(DbError::Invalid(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskEntryRow {
    pub entry_key: String,
    pub kind: DiskKind,
    pub meta_id: Option<String>,
    pub media_type: Option<String>,
    pub path: String,
    pub bytes: u64,
    pub last_accessed_at: i64,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepRuleRow {
    pub profile_id: ProfileId,
    pub meta_id: String,
    pub media_type: String,
    pub keep_watched: bool,
}

impl Database {
    pub fn upsert_disk_entry(&self, entry: &DiskEntryRow) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO disk_entry (entry_key, kind, meta_id, media_type, path, bytes, last_accessed_at, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), ?7)
             ON CONFLICT(entry_key) DO UPDATE SET
               path = excluded.path,
               bytes = excluded.bytes,
               meta_id = COALESCE(excluded.meta_id, disk_entry.meta_id),
               media_type = COALESCE(excluded.media_type, disk_entry.media_type),
               last_accessed_at = unixepoch(),
               pinned = excluded.pinned",
            params![
                entry.entry_key,
                entry.kind.as_str(),
                entry.meta_id,
                entry.media_type,
                entry.path,
                entry.bytes as i64,
                i64::from(entry.pinned),
            ],
        )?;
        Ok(())
    }

    pub fn touch_disk_entry(&self, entry_key: &str) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE disk_entry SET last_accessed_at = unixepoch() WHERE entry_key = ?1",
            [entry_key],
        )?;
        Ok(())
    }

    pub fn delete_disk_entry(&self, entry_key: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM disk_entry WHERE entry_key = ?1", [entry_key])?;
        Ok(())
    }

    pub fn disk_entries_lru(&self) -> Result<Vec<DiskEntryRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT entry_key, kind, meta_id, media_type, path, bytes, last_accessed_at, pinned
             FROM disk_entry
             ORDER BY pinned ASC, last_accessed_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let kind_str: String = r.get(1)?;
            Ok((
                DiskEntryRow {
                    entry_key: r.get(0)?,
                    kind: DiskKind::Blob,
                    meta_id: r.get(2)?,
                    media_type: r.get(3)?,
                    path: r.get(4)?,
                    bytes: r.get::<_, i64>(5)?.max(0) as u64,
                    last_accessed_at: r.get(6)?,
                    pinned: r.get::<_, i64>(7)? != 0,
                },
                kind_str,
            ))
        })?;
        rows.map(|r| {
            let (mut row, k) = r?;
            row.kind = DiskKind::parse(&k)?;
            Ok(row)
        })
        .collect()
    }

    pub fn set_keep_rule(
        &self,
        profile: ProfileId,
        meta_id: &str,
        media_type: &str,
        keep_watched: bool,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO keep_rule (profile_id, meta_id, media_type, keep_watched, updated_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())
             ON CONFLICT(profile_id, meta_id, media_type)
             DO UPDATE SET keep_watched = excluded.keep_watched, updated_at = unixepoch()",
            params![profile, meta_id, media_type, i64::from(keep_watched)],
        )?;
        Ok(())
    }

    pub fn is_item_kept(
        &self,
        profile: ProfileId,
        meta_id: &str,
        media_type: &str,
    ) -> Result<Option<bool>, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT keep_watched FROM keep_rule WHERE profile_id = ?1 AND meta_id = ?2 AND media_type = ?3",
                params![profile, meta_id, media_type],
                |r| Ok(r.get::<_, i64>(0)? != 0),
            )
            .optional()?)
    }

    pub fn any_profile_keeps_item(&self, meta_id: &str, media_type: &str) -> Result<bool, DbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM keep_rule WHERE meta_id = ?1 AND media_type = ?2 AND keep_watched = 1 LIMIT 1",
                params![meta_id, media_type],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
