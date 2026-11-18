//! Recursive local-folder media library scanner (#N23).
//! Strictly in-process and on-demand (never spawns background daemons).

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::events::CoreEvent;
use crate::{Core, CoreError};

const KEY_LOCAL_LIBRARY: &str = "local_library_entries";
const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg",
];
const NOISE_TOKENS: &[&str] = &[
    "1080p", "2160p", "720p", "480p", "4k", "uhd", "hdr", "hdr10", "dv", "dovi", "web", "web-dl",
    "webdl", "webrip", "bluray", "bdrip", "brrip", "remux", "x264", "x265", "h264", "h265", "hevc",
    "av1", "aac", "ac3", "dts", "atmos", "truehd", "ddp", "ddp5", "proper", "repack",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalMediaEntry {
    pub path: String,
    pub filename: String,
    pub title: String,
    pub year: Option<String>,
    pub media_type: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub size_bytes: u64,
}

/// Parses a clean title, optional year, and `(season, episode)` from a media filename.
pub fn parse_local_media_filename(path: &Path, size_bytes: u64) -> LocalMediaEntry {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_owned();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&filename);
    let (season, episode) = cinma_torrent::parse_episode_from_filename(stem);
    let media_type = if season.is_some() || episode.is_some() {
        "series".to_owned()
    } else {
        "movie".to_owned()
    };

    let normalized = stem.replace(['.', '_', '(', ')', '[', ']'], " ");
    let mut title_words: Vec<&str> = Vec::new();
    let mut year: Option<String> = None;

    for token in normalized.split_whitespace() {
        if token == "-" || token == "–" {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        let (ep_s, _) = cinma_torrent::parse_episode_from_filename(&lower);
        if ep_s.is_some() {
            break;
        }
        if NOISE_TOKENS.iter().any(|n| *n == lower) {
            break;
        }
        title_words.push(token);
    }
    if title_words.len() >= 2 {
        if let Some(last) = title_words.last().copied() {
            if last.len() == 4
                && last.bytes().all(|b| b.is_ascii_digit())
                && (last.starts_with("19") || last.starts_with("20"))
            {
                year = Some(last.to_owned());
                title_words.pop();
            }
        }
    }

    let title = if title_words.is_empty() {
        stem.to_owned()
    } else {
        title_words.join(" ")
    };

    LocalMediaEntry {
        path: path.to_string_lossy().into_owned(),
        filename,
        title,
        year,
        media_type,
        season,
        episode,
        size_bytes,
    }
}

impl Core {
    pub fn local_media_entries(&self) -> Result<Vec<LocalMediaEntry>, CoreError> {
        self.load_profile_json(KEY_LOCAL_LIBRARY)
    }

    /// Recursively scans configured `local_media_folders` (depth capped at 6) and
    /// persists the discovered files into SQLite profile storage.
    pub fn scan_local_folders(&self) -> Result<Vec<LocalMediaEntry>, CoreError> {
        let folders = self.player_preferences()?.local_media_folders;
        let mut seen_paths = HashSet::new();
        let mut entries = Vec::new();
        for folder in folders {
            let trimmed = folder.trim();
            if trimmed.is_empty() {
                continue;
            }
            let root = Path::new(trimmed);
            if root.is_dir() {
                walk_media_dir(root, 0, &mut seen_paths, &mut entries);
            }
        }
        entries.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.season.cmp(&b.season))
                .then(a.episode.cmp(&b.episode))
        });
        self.save_profile_json(KEY_LOCAL_LIBRARY, &entries)?;
        self.emit(CoreEvent::LibraryChanged);
        Ok(entries)
    }
}

fn walk_media_dir(
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<LocalMediaEntry>,
) {
    if depth > 6 || out.len() >= 5_000 {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|s| s.to_str())
            && name.starts_with('.')
        {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            walk_media_dir(&path, depth + 1, seen, out);
        } else if meta.is_file() && meta.len() >= 512 * 1024 {
            let is_video = path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| VIDEO_EXTS.contains(&ext.to_ascii_lowercase().as_str()));
            if is_video {
                let canonical = path.to_string_lossy().into_owned();
                if seen.insert(canonical) {
                    out.push(parse_local_media_filename(&path, meta.len()));
                }
            }
        }
    }
}
