use cinma_stremio::Stream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resolution {
    Unknown,
    Sd480,
    Hd720,
    Fhd1080,
    Uhd2160,
}

impl Resolution {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Sd480 => "480p",
            Self::Hd720 => "720p",
            Self::Fhd1080 => "1080p",
            Self::Uhd2160 => "4K",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HdrInfo {
    pub dolby_vision: bool,
    pub hdr10_plus: bool,
    pub hdr10: bool,
    pub hlg: bool,
}

impl HdrInfo {
    pub fn any(self) -> bool {
        self.dolby_vision || self.hdr10_plus || self.hdr10 || self.hlg
    }

    pub fn has_hdr10_compatible(self) -> bool {
        self.hdr10 || self.hdr10_plus
    }

    pub fn badges(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.dolby_vision {
            out.push("DV");
        }
        if self.hdr10_plus {
            out.push("HDR10+");
        } else if self.hdr10 {
            out.push("HDR10");
        }
        if self.hlg {
            out.push("HLG");
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    Unknown,
    Av1,
    Hevc,
    H264,
    Mpeg2,
    Xvid,
}

impl VideoCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Av1 => "AV1",
            Self::Hevc => "HEVC",
            Self::H264 => "H.264",
            Self::Mpeg2 => "MPEG-2",
            Self::Xvid => "XviD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioCodec {
    Unknown,
    Atmos,
    TrueHd,
    DtsHdMa,
    Dts,
    Eac3,
    Ac3,
    Flac,
    Aac,
    Opus,
}

impl AudioCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Atmos => "Atmos",
            Self::TrueHd => "TrueHD",
            Self::DtsHdMa => "DTS-HD MA",
            Self::Dts => "DTS",
            Self::Eac3 => "DD+",
            Self::Ac3 => "AC3",
            Self::Flac => "FLAC",
            Self::Aac => "AAC",
            Self::Opus => "Opus",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceQuality {
    Unknown,
    Remux,
    BluRay,
    WebDl,
    WebRip,
    Hdtv,
    DvdRip,
    CamTs,
}

impl SourceQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Remux => "REMUX",
            Self::BluRay => "BluRay",
            Self::WebDl => "WEB-DL",
            Self::WebRip => "WEBRip",
            Self::Hdtv => "HDTV",
            Self::DvdRip => "DVDRip",
            Self::CamTs => "CAM/TS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedStream {
    pub resolution: Resolution,
    pub hdr: HdrInfo,
    pub codec: VideoCodec,
    pub audio: AudioCodec,
    pub source: SourceQuality,
    pub seeders: Option<u32>,
    pub size_bytes: Option<u64>,
    pub release_group: Option<String>,
    pub languages: Vec<String>,
    pub is_cam_or_ts: bool,
    pub is_sample_or_fake: bool,
    pub is_torrent: bool,
    pub cached_debrid: bool,
}

pub fn parse_stream(stream: &Stream) -> ParsedStream {
    let name = stream.name.as_deref().unwrap_or("");
    let title = stream.display_title().unwrap_or("");
    let hints = stream.behavior_hints.as_ref();
    let filename = hints.and_then(|h| h.filename.as_deref()).unwrap_or("");
    let binge = hints.and_then(|h| h.binge_group.as_deref()).unwrap_or("");

    let combined = format!("{name}\n{title}\n{filename}\n{binge}");
    let tokens = tokenize(&combined);

    let source = parse_source(&tokens, &combined);
    let is_cam_or_ts = source == SourceQuality::CamTs;
    let size_bytes = hints
        .and_then(|h| h.video_size)
        .or_else(|| parse_size_bytes(&combined));
    let is_sample_or_fake = detect_fake(filename, &combined, &tokens, size_bytes);
    let resolution = parse_resolution(&tokens);
    let hdr = parse_hdr(&tokens, &combined);
    let codec = parse_video_codec(&tokens);
    let audio = parse_audio_codec(&tokens, &combined);
    let seeders = parse_seeders(&combined);
    let release_group = parse_release_group(filename).or_else(|| {
        title
            .lines()
            .next()
            .and_then(parse_release_group)
    });
    let languages = parse_languages(&tokens, &combined);

    let is_torrent = stream.info_hash.is_some();
    let cached_debrid = !is_torrent
        && stream.url.is_some()
        && (name.contains('+')
            || name.contains('⚡')
            || name.to_ascii_lowercase().contains("cached"));

    ParsedStream {
        resolution,
        hdr,
        codec,
        audio,
        source,
        seeders,
        size_bytes,
        release_group,
        languages,
        is_cam_or_ts,
        is_sample_or_fake,
        is_torrent,
        cached_debrid,
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| {
        c.is_whitespace() || matches!(c, '.' | '_' | '-' | '[' | ']' | '(' | ')' | '|' | '/' | '+')
    })
    .filter(|s| !s.is_empty())
    .map(|s| s.to_ascii_lowercase())
    .collect()
}

fn has_token(tokens: &[String], candidates: &[&str]) -> bool {
    tokens.iter().any(|t| candidates.contains(&t.as_str()))
}

fn parse_resolution(tokens: &[String]) -> Resolution {
    if has_token(tokens, &["2160p", "4k", "uhd"]) {
        Resolution::Uhd2160
    } else if has_token(tokens, &["1080p", "1080i", "fhd"]) {
        Resolution::Fhd1080
    } else if has_token(tokens, &["720p", "hd"]) {
        Resolution::Hd720
    } else if has_token(tokens, &["480p", "576p", "sd", "480i"]) {
        Resolution::Sd480
    } else {
        Resolution::Unknown
    }
}

fn parse_hdr(tokens: &[String], raw: &str) -> HdrInfo {
    let lower = raw.to_ascii_lowercase();
    let dolby_vision = has_token(tokens, &["dv", "dovi"])
        || lower.contains("dolby vision")
        || lower.contains("dolby.vision")
        || lower.contains("dv+hdr")
        || lower.contains("dv|hdr");
    let hdr10_plus = lower.contains("hdr10+")
        || lower.contains("hdr10plus")
        || has_token(tokens, &["hdr10plus"]);
    let hdr10 = hdr10_plus
        || has_token(tokens, &["hdr10", "hdr"])
        || lower.contains("hdr10");
    let hlg = has_token(tokens, &["hlg"]);
    HdrInfo { dolby_vision, hdr10_plus, hdr10, hlg }
}

fn parse_video_codec(tokens: &[String]) -> VideoCodec {
    if has_token(tokens, &["av1"]) {
        VideoCodec::Av1
    } else if has_token(tokens, &["hevc", "x265", "h265"]) || contains_pair(tokens, "h", "265") {
        VideoCodec::Hevc
    } else if has_token(tokens, &["x264", "h264", "avc"]) || contains_pair(tokens, "h", "264") {
        VideoCodec::H264
    } else if has_token(tokens, &["mpeg2"]) {
        VideoCodec::Mpeg2
    } else if has_token(tokens, &["xvid", "divx"]) {
        VideoCodec::Xvid
    } else {
        VideoCodec::Unknown
    }
}

fn parse_audio_codec(tokens: &[String], raw: &str) -> AudioCodec {
    let lower = raw.to_ascii_lowercase();
    if has_token(tokens, &["atmos"]) {
        AudioCodec::Atmos
    } else if has_token(tokens, &["truehd"]) {
        AudioCodec::TrueHd
    } else if lower.contains("dts-hd") || lower.contains("dts.hd") || has_token(tokens, &["dtshd"]) {
        AudioCodec::DtsHdMa
    } else if has_token(tokens, &["dts"]) {
        AudioCodec::Dts
    } else if has_token(tokens, &["eac3", "ddp", "ddp5", "ddp2"]) || lower.contains("dd+") {
        AudioCodec::Eac3
    } else if has_token(tokens, &["ac3", "dd5", "dd"]) {
        AudioCodec::Ac3
    } else if has_token(tokens, &["flac"]) {
        AudioCodec::Flac
    } else if has_token(tokens, &["aac"]) {
        AudioCodec::Aac
    } else if has_token(tokens, &["opus"]) {
        AudioCodec::Opus
    } else {
        AudioCodec::Unknown
    }
}

fn parse_source(tokens: &[String], raw: &str) -> SourceQuality {
    let lower = raw.to_ascii_lowercase();
    if has_token(
        tokens,
        &["cam", "hdcam", "camrip", "ts", "hdts", "telesync", "tc", "telecine", "hdripcam", "wp", "workprint"],
    ) || lower.contains("cam-rip")
    {
        SourceQuality::CamTs
    } else if has_token(tokens, &["remux"]) {
        SourceQuality::Remux
    } else if has_token(tokens, &["bluray", "bdrip", "brrip", "bdremux", "uhdbd"])
        || lower.contains("blu-ray")
    {
        SourceQuality::BluRay
    } else if lower.contains("web-dl") || lower.contains("web.dl") || has_token(tokens, &["webdl"]) {
        SourceQuality::WebDl
    } else if has_token(tokens, &["webrip", "web"]) {
        SourceQuality::WebRip
    } else if has_token(tokens, &["hdtv", "pdtv"]) {
        SourceQuality::Hdtv
    } else if has_token(tokens, &["dvdrip", "dvdscr", "scr", "dvd"]) {
        SourceQuality::DvdRip
    } else {
        SourceQuality::Unknown
    }
}

fn contains_pair(tokens: &[String], a: &str, b: &str) -> bool {
    tokens.windows(2).any(|w| w[0] == a && w[1] == b)
}

fn detect_fake(filename: &str, raw: &str, tokens: &[String], size_bytes: Option<u64>) -> bool {
    let lower_file = filename.trim().to_ascii_lowercase();
    const BAD_EXTENSIONS: [&str; 6] = [".exe", ".bat", ".cmd", ".scr", ".msi", ".apk"];
    if BAD_EXTENSIONS.iter().any(|ext| lower_file.ends_with(ext)) {
        return true;
    }
    let lower_raw = raw.to_ascii_lowercase();
    if lower_raw.contains("password.txt") || lower_raw.contains("codec.exe") {
        return true;
    }
    if has_token(tokens, &["sample"]) && size_bytes.is_some_and(|s| s < 150 * 1024 * 1024) {
        return true;
    }
    false
}

/// Parses seeders from Torrentio/Comet/MediaFusion formats like `👤 152` or `Seeds: 42`.
fn parse_seeders(raw: &str) -> Option<u32> {
    for marker in ["👤", "👥", "Seeds:", "seeds:", "Seeders:", "seeders:"] {
        if let Some(after) = raw.split(marker).nth(1) {
            let digits: String = after
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

/// Parses sizes from Torrentio/Comet descriptions like `💾 14.5 GB` or `💾 850 MB`.
fn parse_size_bytes(raw: &str) -> Option<u64> {
    let after = raw.split('💾').nth(1)?;
    let mut parts = after.split_whitespace();
    let num: f64 = parts.next()?.replace(',', ".").parse().ok()?;
    let unit = parts.next()?.to_ascii_uppercase();
    let mult: f64 = match unit.as_str() {
        "TB" | "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "MB" | "MIB" => 1024.0 * 1024.0,
        "KB" | "KIB" => 1024.0,
        _ => return None,
    };
    if num.is_sign_negative() || !num.is_finite() {
        return None;
    }
    Some((num * mult) as u64)
}

/// Extracts trailing release group (`-FLUX`, `-FraMeSToR`) from a release name/filename.
fn parse_release_group(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stem = trimmed
        .rsplit_once('.')
        .filter(|(_, ext)| matches!(ext.to_ascii_lowercase().as_str(), "mkv" | "mp4" | "avi" | "mov" | "ts" | "m2ts"))
        .map_or(trimmed, |(base, _)| base);

    let (_, tail) = stem.rsplit_once('-')?;
    let group: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if group.len() >= 2 && group.len() <= 20 && !group.chars().all(|c| c.is_ascii_digit()) {
        // Ignore common hyphenated tags like WEB-DL or DTS-HD.
        if matches!(
            group.to_ascii_lowercase().as_str(),
            "dl" | "hd" | "ma" | "dts" | "rip" | "ray" | "10" | "264" | "265"
        ) {
            return None;
        }
        return Some(group);
    }
    None
}

fn parse_languages(tokens: &[String], raw: &str) -> Vec<String> {
    let mut langs = Vec::new();
    let mut add = |code: &str| {
        if !langs.iter().any(|l| l == code) {
            langs.push(code.to_owned());
        }
    };
    const FLAG_MAP: [(&str, &str); 9] = [
        ("🇹🇷", "tr"),
        ("🇬🇧", "en"),
        ("🇺🇸", "en"),
        ("🇩🇪", "de"),
        ("🇫🇷", "fr"),
        ("🇪🇸", "es"),
        ("🇮🇹", "it"),
        ("🇷🇺", "ru"),
        ("🇯🇵", "ja"),
    ];
    for (flag, code) in FLAG_MAP {
        if raw.contains(flag) {
            add(code);
        }
    }
    const TOKEN_MAP: [(&str, &str); 18] = [
        ("turkish", "tr"),
        ("turkce", "tr"),
        ("tr", "tr"),
        ("tur", "tr"),
        ("english", "en"),
        ("eng", "en"),
        ("german", "de"),
        ("ger", "de"),
        ("french", "fr"),
        ("fre", "fr"),
        ("spanish", "es"),
        ("spa", "es"),
        ("italian", "it"),
        ("ita", "it"),
        ("russian", "ru"),
        ("rus", "ru"),
        ("japanese", "ja"),
        ("jpn", "ja"),
    ];
    for (tok, code) in TOKEN_MAP {
        if tokens.iter().any(|t| t == tok) {
            add(code);
        }
    }
    if has_token(tokens, &["multi"]) {
        add("multi");
    }
    if has_token(tokens, &["dual"]) {
        add("dual");
    }
    langs
}
