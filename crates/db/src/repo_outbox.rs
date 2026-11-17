use rusqlite::params;

use crate::{Database, DbError, ProfileId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: i64,
    pub profile_id: ProfileId,
    pub adapter: String,
    pub action: String,
    pub payload_json: String,
    pub attempts: i64,
    pub next_retry_at: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterStateRow {
    pub profile_id: ProfileId,
    pub adapter: String,
    pub enabled: bool,
    pub last_sync_at: Option<i64>,
    pub cursor_json: Option<String>,
}

impl Database {
    pub fn enqueue_outbox(
        &self,
        profile: ProfileId,
        adapter: &str,
        action: &str,
        payload_json: &str,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO sync_outbox (profile_id, adapter, action, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![profile, adapter, action, payload_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn due_outbox_rows(&self, now_unix: i64, limit: usize) -> Result<Vec<OutboxRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, profile_id, adapter, action, payload_json, attempts, next_retry_at, last_error
             FROM sync_outbox
             WHERE next_retry_at <= ?1
             ORDER BY next_retry_at ASC, id ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now_unix, limit as i64], |r| {
            Ok(OutboxRow {
                id: r.get(0)?,
                profile_id: r.get(1)?,
                adapter: r.get(2)?,
                action: r.get(3)?,
                payload_json: r.get(4)?,
                attempts: r.get(5)?,
                next_retry_at: r.get(6)?,
                last_error: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn outbox_count(&self, profile: ProfileId) -> Result<i64, DbError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE profile_id = ?1",
            [profile],
            |r| r.get(0),
        )?)
    }

    /// Earliest `next_retry_at` across all profiles, or `None` when empty.
    pub fn next_outbox_due(&self) -> Result<Option<i64>, DbError> {
        Ok(self
            .conn
            .query_row("SELECT MIN(next_retry_at) FROM sync_outbox", [], |r| {
                r.get(0)
            })?)
    }

    pub fn delete_outbox_row(&self, id: i64) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM sync_outbox WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn schedule_outbox_retry(
        &self,
        id: i64,
        attempts: i64,
        next_retry_at: i64,
        last_error: &str,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE sync_outbox
             SET attempts = ?2, next_retry_at = ?3, last_error = ?4
             WHERE id = ?1",
            params![id, attempts, next_retry_at, last_error],
        )?;
        Ok(())
    }

    pub fn set_adapter_enabled(
        &self,
        profile: ProfileId,
        adapter: &str,
        enabled: bool,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO sync_adapter_state (profile_id, adapter, enabled)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id, adapter) DO UPDATE SET enabled = excluded.enabled",
            params![profile, adapter, i64::from(enabled)],
        )?;
        Ok(())
    }

    pub fn update_adapter_sync(
        &self,
        profile: ProfileId,
        adapter: &str,
        cursor_json: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO sync_adapter_state (profile_id, adapter, enabled, last_sync_at, cursor_json)
             VALUES (?1, ?2, 1, unixepoch(), ?3)
             ON CONFLICT(profile_id, adapter) DO UPDATE SET
               last_sync_at = unixepoch(),
               cursor_json = COALESCE(excluded.cursor_json, sync_adapter_state.cursor_json)",
            params![profile, adapter, cursor_json],
        )?;
        Ok(())
    }

    pub fn adapter_states(&self, profile: ProfileId) -> Result<Vec<AdapterStateRow>, DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT profile_id, adapter, enabled, last_sync_at, cursor_json
             FROM sync_adapter_state
             WHERE profile_id = ?1
             ORDER BY adapter",
        )?;
        let rows = stmt.query_map([profile], |r| {
            Ok(AdapterStateRow {
                profile_id: r.get(0)?,
                adapter: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                last_sync_at: r.get(3)?,
                cursor_json: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
