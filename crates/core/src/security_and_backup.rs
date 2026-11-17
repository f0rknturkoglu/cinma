//! Phase 7 Security & Backup (`crates/core/src/security_and_backup.rs`):
//! - Profile PIN hashing & constant-time verification (`PBKDF2-HMAC-SHA256` via `ring`)
//! - SQLite online backup (`VACUUM INTO`) and validated live restore

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use cinma_db::{Database, ProfileId};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};

use crate::{Core, CoreError, CoreEvent, DB_FILE_NAME};

const PIN_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

impl Core {
    /// Sets (or clears when `pin` is `None` / empty) a profile's PIN using salted PBKDF2-HMAC-SHA256.
    pub fn set_profile_pin(
        &self,
        profile_id: ProfileId,
        pin: Option<&str>,
    ) -> Result<(), CoreError> {
        let cleaned = pin.map(str::trim).filter(|s| !s.is_empty());
        let encoded = match cleaned {
            Some(raw) => {
                if raw.chars().count() < 4 {
                    return Err(CoreError::Invalid(
                        "PIN must be at least 4 characters".into(),
                    ));
                }
                Some(hash_pin(raw)?)
            }
            None => None,
        };
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_profile_pin_hash(profile_id, encoded.as_deref())?;
        self.emit(CoreEvent::ProfileChanged);
        Ok(())
    }

    /// Returns `true` if the profile has no PIN or if `pin` matches the stored hash.
    pub fn verify_profile_pin(&self, profile_id: ProfileId, pin: &str) -> Result<bool, CoreError> {
        let stored = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .profile_pin_hash(profile_id)?;
        match stored {
            None => Ok(true),
            Some(encoded) => Ok(verify_hashed_pin(pin.trim(), &encoded)),
        }
    }

    /// Writes a consistent SQLite snapshot using `VACUUM INTO`.
    pub fn export_backup(&self, dest: &Path) -> Result<(), CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        db.backup_to(dest)?;
        Ok(())
    }

    /// Validates `src` integrity/schema and replaces the live SQLite database atomically.
    pub fn import_backup(self: &Arc<Self>, src: &Path) -> Result<u32, CoreError> {
        let version = Database::validate_backup_file(src)?;
        let db_path = self.data_dir.join(DB_FILE_NAME);
        {
            let mut db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let placeholder = Database::open_in_memory()?;
            let old = std::mem::replace(&mut *db, placeholder);
            drop(old);

            for suffix in ["-wal", "-shm"] {
                let sidecar = Path::new(&format!("{}{}", db_path.display(), suffix)).to_path_buf();
                if sidecar.exists() {
                    let _ = std::fs::remove_file(sidecar);
                }
            }
            std::fs::copy(src, &db_path)?;
            *db = Database::open(&db_path)?;
        }
        self.emit(CoreEvent::ProfileChanged);
        self.emit(CoreEvent::AddonsChanged);
        self.emit(CoreEvent::LibraryChanged);
        self.emit(CoreEvent::DownloadsChanged);
        Ok(version)
    }
}

fn hash_pin(pin: &str) -> Result<String, CoreError> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    rng.fill(&mut salt)
        .map_err(|_| CoreError::Invalid("failed to generate PIN salt".into()))?;
    let mut out = [0u8; HASH_LEN];
    let iters =
        NonZeroU32::new(PIN_ITERATIONS).ok_or_else(|| CoreError::Invalid("iters".into()))?;
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iters,
        &salt,
        pin.as_bytes(),
        &mut out,
    );
    Ok(format!("v1${}${}", hex_encode(&salt), hex_encode(&out)))
}

fn verify_hashed_pin(pin: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 3 || parts[0] != "v1" {
        return false;
    }
    let (Some(salt), Some(expected)) = (hex_decode(parts[1]), hex_decode(parts[2])) else {
        return false;
    };
    let Some(iters) = NonZeroU32::new(PIN_ITERATIONS) else {
        return false;
    };
    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iters,
        &salt,
        pin.as_bytes(),
        &expected,
    )
    .is_ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks_exact(2) {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}
