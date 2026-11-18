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
    prepare_subtitle_file_with_options(
        raw_bytes,
        cache_dir,
        cache_stem,
        ext_hint,
        encoding_hint,
        lang_hint,
        false,
    )
}

/// Same as [`prepare_subtitle_file`], optionally stripping SDH cues (`[...]`, `(...)`, `♪`)
/// from SRT tracks when `strip_sdh` is enabled.
pub fn prepare_subtitle_file_with_options(
    raw_bytes: &[u8],
    cache_dir: &Path,
    cache_stem: &str,
    ext_hint: Option<&str>,
    encoding_hint: Option<&str>,
    lang_hint: Option<&str>,
    strip_sdh: bool,
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
        .take(72)
        .collect();
    let suffix = if strip_sdh && ext == "srt" {
        "_nosdh"
    } else {
        ""
    };
    let filename = if safe_stem.is_empty() {
        format!("sub{suffix}.{ext}")
    } else {
        format!("{safe_stem}{suffix}.{ext}")
    };
    let path = cache_dir.join(filename);
    let output_text = if strip_sdh && ext == "srt" {
        strip_sdh_cues_srt(&decoded.text)
    } else {
        decoded.text
    };
    // Write UTF-8 cleanly so libmpv / libass render Turkish characters identically;
    // ASS/SSA script sections and style tags stay verbatim (never restyled).
    std::fs::write(&path, output_text.as_bytes())?;
    Ok(PreparedSubtitle {
        local_path: path,
        format: ext.to_owned(),
        detected_encoding: decoded.detected_encoding,
        restyled_ass: false,
    })
}

/// Strips inline and standalone SDH annotations (`[...]`, `(...)`, `♪`, empty all-caps speaker
/// cues) from SRT subtitle text and renumbers remaining cues sequentially.
pub fn strip_sdh_cues_srt(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut next_index = 1usize;

    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .collect();
        let Some(ts_pos) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let ts_line = lines[ts_pos].trim();
        let cleaned_lines: Vec<String> = lines[(ts_pos + 1)..]
            .iter()
            .filter_map(|line| clean_sdh_line(line))
            .collect();
        if cleaned_lines.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!(
            "{next_index}\n{ts_line}\n{}\n",
            cleaned_lines.join("\n")
        ));
        next_index += 1;
    }
    out
}

fn clean_sdh_line(line: &str) -> Option<String> {
    if line.contains('♪') || line.contains('♫') {
        return None;
    }
    let mut stripped = String::with_capacity(line.len());
    let mut square_depth = 0usize;
    let mut paren_depth = 0usize;
    for ch in line.chars() {
        match ch {
            '[' => square_depth += 1,
            ']' if square_depth > 0 => square_depth -= 1,
            '(' if square_depth == 0 => paren_depth += 1,
            ')' if square_depth == 0 && paren_depth > 0 => paren_depth -= 1,
            _ if square_depth == 0 && paren_depth == 0 => stripped.push(ch),
            _ => {}
        }
    }
    let collapsed: String = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    if let Some(before_colon) = trimmed.strip_suffix(':') {
        let label = before_colon.trim_start_matches('-').trim();
        if !label.is_empty() && !label.chars().any(char::is_lowercase) {
            return None;
        }
    }
    Some(trimmed.to_owned())
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

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleCue {
    pub start_s: f64,
    pub end_s: f64,
    pub text: String,
}

/// Parses `.srt`, `.vtt`, `.ass`, or `.ssa` dialogue cues (decoding legacy Windows-1254/UTF-16 encodings automatically).
pub fn parse_subtitle_cues(path: &std::path::Path) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let bytes = std::fs::read(path)?;
    let decoded = encoding::decode_subtitle_bytes(&bytes, None, None);
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if matches!(ext.as_str(), "ass" | "ssa") || decoded.text.contains("[Events]") {
        Ok(parse_ass_cues(&decoded.text))
    } else {
        Ok(parse_srt_vtt_cues(&decoded.text))
    }
}

/// Case-insensitive substring dialogue search across parsed subtitle cues (`limit` capped cleanly).
pub fn search_subtitle_cues(
    path: &std::path::Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SubtitleCue>, SubtitleError> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let cues = parse_subtitle_cues(path)?;
    Ok(cues
        .into_iter()
        .filter(|c| c.text.to_lowercase().contains(&q))
        .take(limit.clamp(1, 200))
        .collect())
}

fn parse_srt_vtt_cues(text: &str) -> Vec<SubtitleCue> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let Some(ts_pos) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start_s, end_s)) = parse_srt_range(lines[ts_pos]) else {
            continue;
        };
        let body: Vec<String> = lines[(ts_pos + 1)..]
            .iter()
            .map(|l| strip_subtitle_markup(l))
            .filter(|l| !l.is_empty())
            .collect();
        if body.is_empty() {
            continue;
        }
        cues.push(SubtitleCue {
            start_s,
            end_s,
            text: body.join(" "),
        });
    }
    cues
}

fn parse_ass_cues(text: &str) -> Vec<SubtitleCue> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    let mut start_idx = 1usize;
    let mut end_idx = 2usize;
    let mut text_idx = 9usize;
    let mut in_events = false;

    for raw_line in normalized.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(fmt) = line.strip_prefix("Format:") {
            for (i, col) in fmt.split(',').enumerate() {
                match col.trim().to_ascii_lowercase().as_str() {
                    "start" => start_idx = i,
                    "end" => end_idx = i,
                    "text" => text_idx = i,
                    _ => {}
                }
            }
            continue;
        }
        let Some(payload) = line.strip_prefix("Dialogue:") else {
            continue;
        };
        let max_fields = text_idx + 1;
        let parts: Vec<&str> = payload.splitn(max_fields, ',').collect();
        if parts.len() <= text_idx {
            continue;
        }
        let Some(start_s) = parse_timestamp_seconds(parts[start_idx].trim()) else {
            continue;
        };
        let Some(end_s) = parse_timestamp_seconds(parts[end_idx].trim()) else {
            continue;
        };
        let clean = strip_subtitle_markup(parts[text_idx]);
        if !clean.is_empty() {
            cues.push(SubtitleCue {
                start_s,
                end_s,
                text: clean,
            });
        }
    }
    cues
}

fn parse_srt_range(line: &str) -> Option<(f64, f64)> {
    let (lhs, rhs) = line.split_once("-->")?;
    let end_token = rhs.split_whitespace().next()?;
    Some((
        parse_timestamp_seconds(lhs.trim())?,
        parse_timestamp_seconds(end_token.trim())?,
    ))
}

fn parse_timestamp_seconds(raw: &str) -> Option<f64> {
    let normalized = raw.trim().replace(',', ".");
    let mut parts = normalized.split(':');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next();
    match c {
        Some(sec) => {
            let h: f64 = a.parse().ok()?;
            let m: f64 = b.parse().ok()?;
            let s: f64 = sec.parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + s)
        }
        None => {
            let m: f64 = a.parse().ok()?;
            let s: f64 = b.parse().ok()?;
            Some(m * 60.0 + s)
        }
    }
}

fn strip_subtitle_markup(raw: &str) -> String {
    let replaced = raw
        .replace("\\N", " ")
        .replace("\\n", " ")
        .replace("\\h", " ");
    let mut out = String::with_capacity(replaced.len());
    let mut in_curly = false;
    let mut in_angle = false;
    for ch in replaced.chars() {
        match ch {
            '{' => in_curly = true,
            '}' => in_curly = false,
            '<' if !in_curly => in_angle = true,
            '>' if in_angle => in_angle = false,
            _ if !in_curly && !in_angle => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{parse_ass_cues, parse_srt_vtt_cues, strip_sdh_cues_srt};

    #[test]
    fn parses_srt_and_ass_dialogue_cues() {
        let srt = "1\n00:01:02,500 --> 00:01:05,000\n<i>Merhaba</i> dünya!\n\n2\n00:02:00.000 --> 00:02:02.500\nİkinci satır\n";
        let cues = parse_srt_vtt_cues(srt);
        assert_eq!(cues.len(), 2);
        assert!((cues[0].start_s - 62.5).abs() < 0.01);
        assert_eq!(cues[0].text, "Merhaba dünya!");

        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:10.50,0:00:13.00,Default,,0,0,0,,{\\an8}Selam\\NTest\n";
        let ass_cues = parse_ass_cues(ass);
        assert_eq!(ass_cues.len(), 1);
        assert!((ass_cues[0].start_s - 10.5).abs() < 0.01);
        assert_eq!(ass_cues[0].text, "Selam Test");
    }

    #[test]
    fn strips_sdh_cues_and_renumbers_srt() {
        let src = "\
1
00:00:01,000 --> 00:00:02,000
[door slams]

2
00:00:03,000 --> 00:00:05,000
MAN: (sighs)
Hello there! [music playing]

3
00:00:06,000 --> 00:00:08,000
♪ Upbeat jazz ♪

4
00:00:09,000 --> 00:00:11,000
Wait for me.
";
        let cleaned = strip_sdh_cues_srt(src);
        let expected = "\
1
00:00:03,000 --> 00:00:05,000
Hello there!

2
00:00:09,000 --> 00:00:11,000
Wait for me.
";
        assert_eq!(cleaned, expected);
    }
}
