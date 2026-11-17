//! Stream parsing, filtering, scoring, tiering, and explanation (§5.6).

#![forbid(unsafe_code)]

mod caps;
mod parse;

use cinma_stremio::Stream;
use serde::{Deserialize, Serialize};

pub use caps::{MachineCapabilities, parse_hardware_av1};
pub use parse::{
    AudioCodec, HdrInfo, ParsedStream, Resolution, SourceQuality, VideoCodec, parse_stream,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyLevel {
    /// Reject fakes, CAM/TS, and torrents with 0 known seeders.
    Strict,
    /// Reject fakes and CAM/TS.
    Standard,
    /// Reject only clear fakes/malicious files; keep CAM/TS.
    Permissive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankerPreferences {
    pub max_resolution: Resolution,
    pub prefer_hdr: bool,
    pub prefer_languages: Vec<String>,
    pub safety_level: SafetyLevel,
    pub min_seeders: u32,
}

impl Default for RankerPreferences {
    fn default() -> Self {
        Self {
            max_resolution: Resolution::Uhd2160,
            prefer_hdr: true,
            prefer_languages: vec!["en".to_owned()],
            safety_level: SafetyLevel::Standard,
            min_seeders: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamTier {
    Best,
    Recommended,
    Good,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreReason {
    /// Machine-readable key for UI localization (e.g. `"resolution.2160p"`).
    pub code: String,
    pub detail: Option<String>,
    pub points: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedStream {
    pub addon_name: String,
    pub stream: Stream,
    pub parsed: ParsedStream,
    pub score: i32,
    pub tier: StreamTier,
    pub reasons: Vec<ScoreReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterReason {
    FakeOrMalicious,
    CamOrTelesync,
    BelowMinSeeders,
    ExceedsMaxResolution,
    NoPlayableSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilteredOutStream {
    pub addon_name: String,
    pub stream: Stream,
    pub reason: FilterReason,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankResult {
    pub ranked: Vec<RankedStream>,
    pub filtered: Vec<FilteredOutStream>,
}

/// Runs the parse → filter → score → tier pipeline on stream candidates.
pub fn rank_streams(
    candidates: Vec<(String, Stream)>,
    prefs: &RankerPreferences,
    caps: &MachineCapabilities,
) -> RankResult {
    let mut scored = Vec::new();
    let mut filtered = Vec::new();

    for (addon_name, stream) in candidates {
        let parsed = parse_stream(&stream);
        if let Some(reason) = filter_reason(&stream, &parsed, prefs) {
            filtered.push(FilteredOutStream { addon_name, stream, reason });
            continue;
        }
        let (score, reasons) = score_stream(&stream, &parsed, prefs, caps);
        scored.push((addon_name, stream, parsed, score, reasons));
    }

    scored.sort_by(|a, b| {
        b.3.cmp(&a.3)
            .then_with(|| b.2.seeders.unwrap_or(0).cmp(&a.2.seeders.unwrap_or(0)))
    });

    let top_score = scored.first().map_or(0, |s| s.3);
    let ranked = scored
        .into_iter()
        .enumerate()
        .map(|(index, (addon_name, stream, parsed, score, reasons))| {
            let tier = assign_tier(index, score, top_score);
            RankedStream { addon_name, stream, parsed, score, tier, reasons }
        })
        .collect();

    RankResult { ranked, filtered }
}

fn filter_reason(
    stream: &Stream,
    parsed: &ParsedStream,
    prefs: &RankerPreferences,
) -> Option<FilterReason> {
    if stream.info_hash.is_none() && stream.url.is_none() {
        return Some(FilterReason::NoPlayableSource);
    }
    if parsed.is_sample_or_fake {
        return Some(FilterReason::FakeOrMalicious);
    }
    if parsed.is_cam_or_ts && prefs.safety_level != SafetyLevel::Permissive {
        return Some(FilterReason::CamOrTelesync);
    }
    if parsed.resolution != Resolution::Unknown && parsed.resolution > prefs.max_resolution {
        return Some(FilterReason::ExceedsMaxResolution);
    }
    if parsed.is_torrent {
        let seeders = parsed.seeders;
        if prefs.safety_level == SafetyLevel::Strict && seeders == Some(0) {
            return Some(FilterReason::BelowMinSeeders);
        }
        if prefs.min_seeders > 0 && seeders.is_some_and(|s| s < prefs.min_seeders) {
            return Some(FilterReason::BelowMinSeeders);
        }
    }
    None
}

fn score_stream(
    stream: &Stream,
    parsed: &ParsedStream,
    prefs: &RankerPreferences,
    caps: &MachineCapabilities,
) -> (i32, Vec<ScoreReason>) {
    let mut reasons = Vec::new();
    let mut push = |code: &str, detail: Option<String>, points: i32| {
        if points != 0 {
            reasons.push(ScoreReason { code: code.to_owned(), detail, points });
        }
    };

    let res_pts = match parsed.resolution {
        Resolution::Uhd2160 => 400,
        Resolution::Fhd1080 => 300,
        Resolution::Hd720 => 180,
        Resolution::Sd480 => 60,
        Resolution::Unknown => 100,
    };
    push("resolution", Some(parsed.resolution.label().to_owned()), res_pts);

    let src_pts = match parsed.source {
        SourceQuality::Remux => 140,
        SourceQuality::BluRay => 110,
        SourceQuality::WebDl => 90,
        SourceQuality::WebRip => 65,
        SourceQuality::Hdtv => 40,
        SourceQuality::DvdRip => 10,
        SourceQuality::CamTs => -500,
        SourceQuality::Unknown => 0,
    };
    if parsed.source != SourceQuality::Unknown {
        push("source", Some(parsed.source.label().to_owned()), src_pts);
    }

    if prefs.prefer_hdr && parsed.hdr.any() {
        let hdr_pts = if parsed.hdr.dolby_vision && parsed.hdr.has_hdr10_compatible() {
            90
        } else if parsed.hdr.has_hdr10_compatible() {
            80
        } else if parsed.hdr.dolby_vision {
            // Pure DV (P5) has limited support on the legacy OpenGL render path (ADR 0004).
            45
        } else {
            40
        };
        push("hdr", Some(parsed.hdr.badges().join(" · ")), hdr_pts);
    }

    let codec_pts = match parsed.codec {
        VideoCodec::Hevc => 45,
        VideoCodec::Av1 if caps.hardware_av1 => 55,
        VideoCodec::Av1 => -40,
        VideoCodec::H264 => 25,
        VideoCodec::Mpeg2 | VideoCodec::Xvid => -15,
        VideoCodec::Unknown => 0,
    };
    if parsed.codec != VideoCodec::Unknown {
        let code = if parsed.codec == VideoCodec::Av1 && !caps.hardware_av1 {
            "codec.av1_sw"
        } else {
            "codec"
        };
        push(code, Some(parsed.codec.label().to_owned()), codec_pts);
    }

    let audio_pts = match parsed.audio {
        AudioCodec::Atmos | AudioCodec::TrueHd => 45,
        AudioCodec::DtsHdMa => 40,
        AudioCodec::Eac3 => 25,
        AudioCodec::Dts | AudioCodec::Flac => 20,
        AudioCodec::Ac3 => 12,
        AudioCodec::Aac | AudioCodec::Opus => 8,
        AudioCodec::Unknown => 0,
    };
    if parsed.audio != AudioCodec::Unknown {
        push("audio", Some(parsed.audio.label().to_owned()), audio_pts);
    }

    if parsed.is_torrent {
        if let Some(seeders) = parsed.seeders {
            let seed_pts = match seeders {
                0 => -250,
                1..=4 => 10,
                5..=19 => 40,
                20..=99 => 80,
                _ => 120,
            };
            push("seeders", Some(seeders.to_string()), seed_pts);
        }
    } else if stream.url.is_some() {
        let not_web_ready = stream
            .behavior_hints
            .as_ref()
            .and_then(|h| h.not_web_ready)
            .unwrap_or(false);
        let direct_pts = if parsed.cached_debrid {
            135
        } else if not_web_ready {
            60
        } else {
            110
        };
        push("direct", None, direct_pts);
    }

    for (idx, pref_lang) in prefs.prefer_languages.iter().enumerate() {
        let pref_lower = pref_lang.to_ascii_lowercase();
        if parsed.languages.iter().any(|l| l == &pref_lower) {
            let lang_pts = if idx == 0 { 90 } else { 55 };
            push("language", Some(pref_lang.clone()), lang_pts);
            break;
        }
    }
    if parsed.languages.iter().any(|l| l == "multi" || l == "dual") {
        push("language.multi", None, 25);
    }

    if let Some(ref group) = parsed.release_group {
        if is_reputable_group(group) {
            push("release_group", Some(group.clone()), 25);
        }
    }

    let total = reasons.iter().map(|r| r.points).sum();
    (total, reasons)
}

fn assign_tier(index: usize, score: i32, top_score: i32) -> StreamTier {
    if index == 0 && score >= 450 {
        StreamTier::Best
    } else if score >= 480 && top_score - score <= 60 {
        StreamTier::Recommended
    } else if score >= 280 {
        StreamTier::Good
    } else {
        StreamTier::Fallback
    }
}

fn is_reputable_group(group: &str) -> bool {
    const KNOWN: [&str; 13] = [
        "framestor", "flux", "ntb", "playbd", "ctrlhd", "don", "hone", "tepes", "sparks", "qxr",
        "tigole", "deflate", "cmrg",
    ];
    KNOWN.contains(&group.to_ascii_lowercase().as_str())
}
