//! Profile management, onboarding flag, library items, and watch progress.

use std::collections::HashSet;

use cinma_db::{LibraryItemRow, ProfileId, ProfileRow, WatchProgressRow};

use crate::events::CoreEvent;
use crate::{ACTIVE_PROFILE_KEY, Core, CoreError};

const ONBOARDING_KEY: &str = "onboarding.completed";
const WATCHED_THRESHOLD_RATIO: f64 = 0.90;

#[derive(Debug, Clone)]
pub struct WatchProgressInput {
    pub video_id: String,
    pub meta_id: String,
    pub media_type: String,
    pub title: String,
    pub artwork: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub position_s: f64,
    pub duration_s: f64,
}

impl Core {
    // ---- onboarding ----

    pub fn onboarding_completed(&self) -> Result<bool, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.get_app_setting(ONBOARDING_KEY)?.as_deref() == Some("1"))
    }

    pub fn set_onboarding_completed(&self, completed: bool) -> Result<(), CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        db.set_app_setting(ONBOARDING_KEY, if completed { "1" } else { "0" })?;
        Ok(())
    }

    // ---- profiles ----

    pub fn profiles(&self) -> Result<Vec<ProfileRow>, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.profiles()?)
    }

    pub fn set_active_profile(&self, id: ProfileId) -> Result<(), CoreError> {
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            if !db.profile_exists(id)? {
                return Err(CoreError::Invalid(format!("profile {id} does not exist")));
            }
            db.set_app_setting(ACTIVE_PROFILE_KEY, &id.to_string())?;
        }
        self.emit(CoreEvent::ProfileChanged);
        self.emit(CoreEvent::AddonsChanged);
        self.emit(CoreEvent::LibraryChanged);
        Ok(())
    }

    pub fn create_profile(
        &self,
        name: String,
        avatar: Option<String>,
    ) -> Result<ProfileId, CoreError> {
        let id = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.create_profile_with_avatar(&name, avatar.as_deref())?
        };
        self.emit(CoreEvent::ProfileChanged);
        Ok(id)
    }

    pub fn rename_profile(&self, id: ProfileId, name: String) -> Result<(), CoreError> {
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.rename_profile(id, &name)?;
        }
        self.emit(CoreEvent::ProfileChanged);
        Ok(())
    }

    pub fn delete_profile(&self, id: ProfileId) -> Result<(), CoreError> {
        let switched = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let active = self.active_profile_locked(&db)?;
            db.delete_profile(id)?;
            if active == id {
                let first = db
                    .profiles()?
                    .first()
                    .map(|p| p.id)
                    .ok_or_else(|| CoreError::Invalid("no profiles remaining".into()))?;
                db.set_app_setting(ACTIVE_PROFILE_KEY, &first.to_string())?;
                true
            } else {
                false
            }
        };
        self.emit(CoreEvent::ProfileChanged);
        if switched {
            self.emit(CoreEvent::AddonsChanged);
            self.emit(CoreEvent::LibraryChanged);
        }
        Ok(())
    }

    fn active_profile_locked(&self, db: &cinma_db::Database) -> Result<ProfileId, CoreError> {
        let stored = db
            .get_app_setting(ACTIVE_PROFILE_KEY)?
            .and_then(|v| v.parse().ok());
        match stored {
            Some(id) if db.profile_exists(id)? => Ok(id),
            _ => Ok(1),
        }
    }

    // ---- library ----

    pub fn library_items(&self) -> Result<Vec<LibraryItemRow>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.library_items(profile)?)
    }

    pub fn is_in_library(&self, meta_id: &str, media_type: &str) -> Result<bool, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.is_in_library(profile, meta_id, media_type)?)
    }

    pub fn add_to_library(
        &self,
        meta_id: String,
        media_type: String,
        name: String,
        poster: Option<String>,
    ) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .add_library_item(profile, &meta_id, &media_type, &name, poster.as_deref())?;
        self.emit(CoreEvent::LibraryChanged);
        Ok(())
    }

    pub fn remove_from_library(&self, meta_id: &str, media_type: &str) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .remove_library_item(profile, meta_id, media_type)?;
        self.emit(CoreEvent::LibraryChanged);
        Ok(())
    }

    // ---- watch progress ----

    /// Pure SQLite read — called on launch before any network call (§5.16).
    pub fn continue_watching(&self, limit: usize) -> Result<Vec<WatchProgressRow>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.continue_watching(profile, limit)?)
    }

    pub fn get_watch_progress(
        &self,
        video_id: &str,
    ) -> Result<Option<WatchProgressRow>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.get_watch_progress(profile, video_id)?)
    }

    pub fn progress_for_meta(&self, meta_id: &str) -> Result<Vec<WatchProgressRow>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.progress_for_meta(profile, meta_id)?)
    }

    /// Meta IDs that have watched entries and are not currently in-progress in Continue Watching.
    pub fn watched_meta_ids(&self) -> Result<Vec<String>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let watched = db.watched_items(profile, 2000)?;
        let active = db.continue_watching(profile, 500)?;
        let in_progress: HashSet<String> = active.into_iter().map(|r| r.meta_id).collect();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for row in watched {
            if !in_progress.contains(&row.meta_id) && seen.insert(row.meta_id.clone()) {
                out.push(row.meta_id);
            }
        }
        Ok(out)
    }

    pub fn save_watch_progress(&self, input: WatchProgressInput) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        let watched = input.duration_s > 0.0
            && (input.position_s / input.duration_s) >= WATCHED_THRESHOLD_RATIO;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_watch_progress(
                profile,
                &input.video_id,
                &input.meta_id,
                &input.media_type,
                &input.title,
                input.artwork.as_deref(),
                input.season,
                input.episode,
                input.position_s.max(0.0),
                input.duration_s.max(0.0),
                watched,
            )?;
        if watched {
            let _ = self.enqueue_watch_sync(
                profile,
                &input.meta_id,
                &input.media_type,
                &input.title,
                input.season.and_then(|s| u32::try_from(s).ok()),
                input.episode.and_then(|e| u32::try_from(e).ok()),
                100.0,
            );
        }
        self.emit(CoreEvent::LibraryChanged);
        Ok(())
    }

    pub fn set_watched(
        &self,
        video_id: String,
        meta_id: String,
        media_type: String,
        title: String,
        watched: bool,
    ) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let existing = db.get_watch_progress(profile, &video_id)?;
        let (position_s, duration_s, artwork, season, episode) = match existing {
            Some(row) => (
                if watched { row.duration_s } else { 0.0 },
                row.duration_s,
                row.artwork,
                row.season,
                row.episode,
            ),
            None => (0.0, 0.0, None, None, None),
        };
        db.upsert_watch_progress(
            profile,
            &video_id,
            &meta_id,
            &media_type,
            &title,
            artwork.as_deref(),
            season,
            episode,
            position_s,
            duration_s,
            watched,
        )?;
        drop(db);
        if watched {
            let _ = self.enqueue_watch_sync(
                profile,
                &meta_id,
                &media_type,
                &title,
                season.and_then(|s| u32::try_from(s).ok()),
                episode.and_then(|e| u32::try_from(e).ok()),
                100.0,
            );
        }
        self.emit(CoreEvent::LibraryChanged);
        Ok(())
    }
}
