//! Subtitle provider aggregation, ranking, Turkish/legacy encoding detection,
//! and UTF-8 preparation (`CLAUDE.md` §5.8).

#![forbid(unsafe_code)]

pub mod encoding;
pub mod opensubtitles;

use std::path::{Path, PathBuf};

use cinma_stremio::AddonSubtitle;

pub use encoding::{DecodedSubtitle, decode_subtitle_bytes, unpack_if_gzipped};
pub use opensubtitles::{OpenSubtitlesClient, OsAccountSession, OsUserQuota};

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("subtitle provider returned HTTP {0}")]
    HttpStatus(u16),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleQueryParams {
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCandidate {
    pub id: String,
    pub provider: String,
    pub lang: String,
    pub title: Option<String>,
    pub download_url: String,
    pub opensubtitles_file_id: Option<i64>,
    pub encoding_hint: Option<String>,
    pub is_sdh: bool,
    pub is_forced: bool,
    pub hash_matched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSubtitle {
    pub local_path: PathBuf,
    pub format: String,
    pub detected_encoding: String,
    pub restyled_ass: bool,
}

/// Normalizes ISO 639-1/2/BCP-47 language codes to 3-letter canonical tags where
/// common (`tr`/`tur`/`turkish` → `tur`, `en`/`eng` → `eng`, etc.).
pub fn normalize_lang_code(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    let base = trimmed.split(['-', '_']).next().unwrap_or(&trimmed);
    match base {
        "tr" | "tur" | "trk" | "turkish" | "türkçe" | "turkce" => "tur".to_owned(),
        "en" | "eng" | "english" => "eng".to_owned(),
        "de" | "deu" | "ger" | "german" => "deu".to_owned(),
        "fr" | "fra" | "fre" | "french" => "fra".to_owned(),
        "es" | "spa" | "spanish" => "spa".to_owned(),
        "it" | "ita" | "italian" => "ita".to_owned(),
        "pt" | "por" | "portuguese" => "por".to_owned(),
        "ru" | "rus" | "russian" => "rus".to_owned(),
        "ja" | "jpn" | "japanese" => "jpn".to_owned(),
        "ko" | "kor" | "korean" => "kor".to_owned(),
        "ar" | "ara" | "arabic" => "ara".to_owned(),
        other => other.to_owned(),
    }
}

pub fn candidate_from_stremio(
    addon_name: &str,
    sub: AddonSubtitle,
    had_hash: bool,
) -> SubtitleCandidate {
    let label_lower = sub.title.as_deref().unwrap_or(&sub.id).to_ascii_lowercase();
    let is_sdh = sub.hearing_impaired.unwrap_or(false)
        || label_lower.contains("sdh")
        || label_lower.contains("[cc]")
        || label_lower.contains("hearing");
    let is_forced = sub.forced.unwrap_or(false) || label_lower.contains("forced");
    SubtitleCandidate {
        id: sub.id,
        provider: addon_name.to_owned(),
        lang: normalize_lang_code(&sub.lang),
        title: sub.title,
        download_url: sub.url,
        opensubtitles_file_id: None,
        encoding_hint: sub.sub_encoding,
        is_sdh,
        is_forced,
        hash_matched: had_hash,
    }
}

/// Orders candidates by:
/// 1. Preferred language rank (lower index = higher priority)
/// 2. OpenSubtitles hash match
/// 3. Non-forced / non-SDH unless preferred
pub fn rank_candidates(candidates: &mut [SubtitleCandidate], preferred_langs: &[String]) {
    let norm_prefs: Vec<String> = preferred_langs
        .iter()
        .map(|l| normalize_lang_code(l))
        .collect();
    candidates.sort_by_key(|c| {
        let lang_rank = norm_prefs
            .iter()
            .position(|p| p == &c.lang)
            .unwrap_or(usize::MAX);
        (
            lang_rank,
            !c.hash_matched,
            c.is_forced,
            c.is_sdh,
            c.provider.clone(),
        )
    });
}

/// Prepares downloaded or local subtitle bytes onto `cache_dir` as UTF-8,
/// unpacking gzip if needed and converting legacy encodings (such as Turkish
/// `windows-1254` / `ISO-8859-9`). Never modifies ASS/SSA script styling rules.
pub fn prepare_subtitle_file(
    raw_bytes: &[u8],
    cache_dir: &Path,
    cache_stem: &str,
    ext_hint: Option<&str>,
    encoding_hint: Option<&str>,
    lang_hint: Option<&str>,
) -> Result<PreparedSubtitle, SubtitleError> {
    std::fs::create_dir_all(cache_dir)?;
    let unpacked = unpack_if_gzipped(raw_bytes)?;
    let decoded = decode_subtitle_bytes(&unpacked, encoding_hint, lang_hint);

    let ext = detect_format_ext(&decoded.text, ext_hint);
    let safe_stem: String = cache_stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let filename = if safe_stem.is_empty() {
        format!("sub.{ext}")
    } else {
        format!("{safe_stem}.{ext}")
    };
    let path = cache_dir.join(filename);
    // Write UTF-8 cleanly so libmpv / libass render Turkish characters identically;
    // ASS/SSA script sections and style tags stay verbatim (never restyled).
    std::fs::write(&path, decoded.text.as_bytes())?;
    Ok(PreparedSubtitle {
        local_path: path,
        format: ext.to_owned(),
        detected_encoding: decoded.detected_encoding,
        restyled_ass: false,
    })
}

fn detect_format_ext<'a>(text: &str, hint: Option<&'a str>) -> &'a str {
    if let Some(ext) = hint.map(|s| s.trim().trim_start_matches('.')) {
        match ext.to_ascii_lowercase().as_str() {
            "ass" => return "ass",
            "ssa" => return "ssa",
            "vtt" => return "vtt",
            "srt" => return "srt",
            "sub" => return "sub",
            _ => {}
        }
    }
    let trimmed = text.trim_start();
    if trimmed.contains("[Script Info]") || trimmed.contains("[V4+ Styles]") {
        "ass"
    } else if trimmed.starts_with("WEBVTT") {
        "vtt"
    } else {
        "srt"
    }
}
