//! Encoding detection, gzip unpacking, and UTF-8 normalization for subtitle files.
//! Handles UTF-8/UTF-16 BOM, valid UTF-8, explicit encoding hints, and Turkish
//! legacy encodings (`windows-1254` / `ISO-8859-9`).

use std::io::Read;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252, WINDOWS_1254};
use flate2::read::GzDecoder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSubtitle {
    pub text: String,
    pub detected_encoding: String,
}

/// Unpacks gzip payloads (detected by magic bytes `[0x1f, 0x8b]`) and returns
/// the raw subtitle bytes untouched otherwise.
pub fn unpack_if_gzipped(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    if bytes.starts_with(&[0x1F, 0x8B]) {
        let mut decoder = GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

/// Detects the encoding of `raw` (with optional provider hint and language code)
/// and decodes into a UTF-8 string.
pub fn decode_subtitle_bytes(
    raw: &[u8],
    encoding_hint: Option<&str>,
    lang_hint: Option<&str>,
) -> DecodedSubtitle {
    if let Some(stripped) = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (cow, _, _) = UTF_8.decode(stripped);
        return DecodedSubtitle {
            text: cow.into_owned(),
            detected_encoding: "utf-8-bom".to_owned(),
        };
    }
    if let Some(stripped) = raw.strip_prefix(&[0xFF, 0xFE]) {
        let (cow, _, _) = UTF_16LE.decode(stripped);
        return DecodedSubtitle {
            text: cow.into_owned(),
            detected_encoding: "utf-16le".to_owned(),
        };
    }
    if let Some(stripped) = raw.strip_prefix(&[0xFE, 0xFF]) {
        let (cow, _, _) = UTF_16BE.decode(stripped);
        return DecodedSubtitle {
            text: cow.into_owned(),
            detected_encoding: "utf-16be".to_owned(),
        };
    }

    if let Ok(utf8) = std::str::from_utf8(raw) {
        return DecodedSubtitle {
            text: utf8.to_owned(),
            detected_encoding: "utf-8".to_owned(),
        };
    }

    if let Some(hint) = encoding_hint.and_then(|label| Encoding::for_label(label.trim().as_bytes()))
    {
        let (cow, _, _) = hint.decode(raw);
        return DecodedSubtitle {
            text: cow.into_owned(),
            detected_encoding: hint.name().to_ascii_lowercase(),
        };
    }

    if is_likely_turkish(raw, lang_hint) {
        let (cow, _, _) = WINDOWS_1254.decode(raw);
        return DecodedSubtitle {
            text: cow.into_owned(),
            detected_encoding: "windows-1254".to_owned(),
        };
    }

    let (cow, _, _) = WINDOWS_1252.decode(raw);
    DecodedSubtitle {
        text: cow.into_owned(),
        detected_encoding: "windows-1252".to_owned(),
    }
}

fn is_likely_turkish(raw: &[u8], lang_hint: Option<&str>) -> bool {
    if let Some(lang) = lang_hint {
        let l = lang.trim().to_ascii_lowercase();
        if matches!(
            l.as_str(),
            "tr" | "tur" | "trk" | "tr-tr" | "turkish" | "türkçe"
        ) {
            return true;
        }
    }
    // Windows-1254 / ISO-8859-9 Turkish-specific bytes:
    // 0xD0 (Ğ), 0xDD (İ), 0xDE (Ş), 0xF0 (ğ), 0xFD (ı), 0xFE (ş).
    let tr_hits = raw
        .iter()
        .filter(|&&b| matches!(b, 0xD0 | 0xDD | 0xDE | 0xF0 | 0xFD | 0xFE))
        .count();
    // In Latin-1 (windows-1252) those bytes are rare Icelandic letters (Ð, Ý, Þ, ð, ý, þ).
    tr_hits >= 2
}
