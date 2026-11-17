//! M3U / M3U8 playlist parser supporting `tvg-id`, `tvg-logo`, `group-title`.

use cinma_db::NewIptvChannel;

pub fn parse_m3u(content: &str) -> Vec<NewIptvChannel> {
    let mut channels = Vec::new();
    let mut pending_meta: Option<ExtInfMeta> = None;
    let mut fallback_idx = 0usize;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXTINF:") {
            pending_meta = Some(parse_extinf(line));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(meta) = pending_meta.take() {
            fallback_idx += 1;
            let name = if meta.name.is_empty() {
                format!("Channel {fallback_idx}")
            } else {
                meta.name
            };
            let stream_id = meta
                .tvg_id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("m3u:{fallback_idx}:{name}"));
            channels.push(NewIptvChannel {
                stream_id,
                name,
                group_title: meta.group_title.filter(|s| !s.is_empty()),
                logo_url: meta.tvg_logo.filter(|s| !s.is_empty()),
                stream_url: line.to_owned(),
                epg_channel_id: meta.tvg_id.filter(|s| !s.is_empty()),
            });
        } else if line.contains("://") {
            fallback_idx += 1;
            channels.push(NewIptvChannel {
                stream_id: format!("m3u:{fallback_idx}"),
                name: format!("Channel {fallback_idx}"),
                group_title: None,
                logo_url: None,
                stream_url: line.to_owned(),
                epg_channel_id: None,
            });
        }
    }

    channels
}

struct ExtInfMeta {
    tvg_id: Option<String>,
    tvg_logo: Option<String>,
    group_title: Option<String>,
    name: String,
}

fn parse_extinf(line: &str) -> ExtInfMeta {
    // Format: `#EXTINF:-1 tvg-id="..." group-title="..." tvg-logo="...",Channel Name`
    let (attrs, name) = split_extinf_comma(line);
    ExtInfMeta {
        tvg_id: extract_attr(attrs, "tvg-id"),
        tvg_logo: extract_attr(attrs, "tvg-logo"),
        group_title: extract_attr(attrs, "group-title"),
        name: name.trim().to_owned(),
    }
}

fn split_extinf_comma(line: &str) -> (&str, &str) {
    let mut in_quotes = false;
    let bytes = line.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b',' && !in_quotes {
            return (&line[..idx], &line[idx + 1..]);
        }
    }
    (line, "")
}

fn extract_attr(attrs: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=\"");
    let lower = attrs.to_ascii_lowercase();
    let idx = lower.find(&pat)?;
    let start = idx + pat.len();
    let rest = &attrs[start..];
    let end = rest.find('"')?;
    Some(rest[..end].trim().to_owned())
}
