//! macOS Keychain wrapper for user secrets (TMDB API key in Phase 3).

use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password,
};

use crate::CoreError;

const SERVICE: &str = "app.cinma.mac";
const TMDB_ACCOUNT: &str = "tmdb.api_key";

pub fn get_tmdb_api_key() -> Result<Option<String>, CoreError> {
    if let Ok(env_key) = std::env::var("CINMA_TMDB_API_KEY") {
        let trimmed = env_key.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_owned()));
        }
    }
    match generic_password(
        security_framework::passwords::PasswordOptions::new_generic_password(SERVICE, TMDB_ACCOUNT),
    ) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes)
                .map_err(|e| CoreError::Invalid(format!("invalid UTF-8 in Keychain: {e}")))?;
            let trimmed = s.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_owned()))
        }
        Err(err) if err.code() == -25300 /* errSecItemNotFound */ => Ok(None),
        Err(err) => Err(CoreError::Invalid(format!("keychain read failed: {err}"))),
    }
}

pub fn set_tmdb_api_key(key: Option<&str>) -> Result<(), CoreError> {
    let trimmed = key.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(value) => set_generic_password(SERVICE, TMDB_ACCOUNT, value.as_bytes())
            .map_err(|e| CoreError::Invalid(format!("keychain write failed: {e}"))),
        None => match delete_generic_password(SERVICE, TMDB_ACCOUNT) {
            Ok(()) => Ok(()),
            Err(err) if err.code() == -25300 => Ok(()),
            Err(err) => Err(CoreError::Invalid(format!("keychain delete failed: {err}"))),
        },
    }
}
