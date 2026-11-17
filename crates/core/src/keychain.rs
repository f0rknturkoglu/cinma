//! macOS Keychain wrapper for user secrets (TMDB & OpenSubtitles credentials).

use security_framework::passwords::{
    PasswordOptions, delete_generic_password, generic_password, set_generic_password,
};

use crate::CoreError;

const SERVICE: &str = "app.cinma.mac";
const TMDB_ACCOUNT: &str = "tmdb.api_key";
const OPENSUBTITLES_KEY_ACCOUNT: &str = "opensubtitles.api_key";
const OPENSUBTITLES_TOKEN_ACCOUNT: &str = "opensubtitles.token";

fn read_secret(account: &str, env_override: Option<&str>) -> Result<Option<String>, CoreError> {
    if let Some(var) = env_override
        && let Ok(env_val) = std::env::var(var)
    {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_owned()));
        }
    }
    match generic_password(PasswordOptions::new_generic_password(SERVICE, account)) {
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

fn write_secret(account: &str, value: Option<&str>) -> Result<(), CoreError> {
    let trimmed = value.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(v) => set_generic_password(SERVICE, account, v.as_bytes())
            .map_err(|e| CoreError::Invalid(format!("keychain write failed: {e}"))),
        None => match delete_generic_password(SERVICE, account) {
            Ok(()) => Ok(()),
            Err(err) if err.code() == -25300 => Ok(()),
            Err(err) => Err(CoreError::Invalid(format!("keychain delete failed: {err}"))),
        },
    }
}

pub fn get_tmdb_api_key() -> Result<Option<String>, CoreError> {
    read_secret(TMDB_ACCOUNT, Some("CINMA_TMDB_API_KEY"))
}

pub fn set_tmdb_api_key(key: Option<&str>) -> Result<(), CoreError> {
    write_secret(TMDB_ACCOUNT, key)
}

pub fn get_opensubtitles_api_key() -> Result<Option<String>, CoreError> {
    read_secret(
        OPENSUBTITLES_KEY_ACCOUNT,
        Some("CINMA_OPENSUBTITLES_API_KEY"),
    )
}

pub fn set_opensubtitles_api_key(key: Option<&str>) -> Result<(), CoreError> {
    write_secret(OPENSUBTITLES_KEY_ACCOUNT, key)
}

pub fn get_opensubtitles_token() -> Result<Option<String>, CoreError> {
    read_secret(
        OPENSUBTITLES_TOKEN_ACCOUNT,
        Some("CINMA_OPENSUBTITLES_TOKEN"),
    )
}

pub fn set_opensubtitles_token(token: Option<&str>) -> Result<(), CoreError> {
    write_secret(OPENSUBTITLES_TOKEN_ACCOUNT, token)
}

pub fn get_tracking_token(profile_id: i64, adapter: &str) -> Result<Option<String>, CoreError> {
    let account = format!("tracking.p{profile_id}.{adapter}");
    let env_name = format!("CINMA_TRACKING_{}_TOKEN", adapter.to_ascii_uppercase());
    read_secret(&account, Some(&env_name))
}

/// App credentials (client secret) for a tracking adapter; shared by profiles.
pub fn set_tracking_client_secret(adapter: &str, secret: Option<&str>) -> Result<(), CoreError> {
    write_secret(&format!("tracking.{adapter}.client_secret"), secret)
}

pub fn set_tracking_token(
    profile_id: i64,
    adapter: &str,
    token: Option<&str>,
) -> Result<(), CoreError> {
    let account = format!("tracking.p{profile_id}.{adapter}");
    write_secret(&account, token)
}
