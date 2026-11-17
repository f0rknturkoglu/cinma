//! Minimal zero-dependency XMLTV parser for `<programme>` EPG entries.

use cinma_db::IptvEpgRow;

pub fn parse_xmltv(xml: &str) -> Vec<IptvEpgRow> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(open_idx) = rest.find("<programme") {
        let after_open = &rest[open_idx..];
        let Some(tag_end) = after_open.find('>') else {
            break;
        };
        let attrs = &after_open[..tag_end];
        let Some(close_idx) = after_open[tag_end + 1..].find("</programme>") else {
            break;
        };
        let body = &after_open[tag_end + 1..tag_end + 1 + close_idx];

        let channel_id = attr_value(attrs, "channel").unwrap_or_default();
        let start_ts = attr_value(attrs, "start")
            .as_deref()
            .and_then(parse_xmltv_timestamp);
        let stop_ts = attr_value(attrs, "stop")
            .as_deref()
            .and_then(parse_xmltv_timestamp);

        if !channel_id.is_empty()
            && let (Some(start), Some(stop)) = (start_ts, stop_ts)
            && let Some(title) = tag_text(body, "title")
        {
            out.push(IptvEpgRow {
                channel_id,
                start_ts: start,
                stop_ts: stop,
                title,
                description: tag_text(body, "desc"),
            });
        }

        rest = &after_open[tag_end + 1 + close_idx + "</programme>".len()..];
    }
    out
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pat = format!("{name}={quote}");
        if let Some(pos) = attrs.find(&pat) {
            let after = &attrs[pos + pat.len()..];
            if let Some(end) = after.find(quote) {
                return Some(unescape_xml(after[..end].trim()));
            }
        }
    }
    None
}

fn tag_text(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let idx = body.find(&open)?;
    let gt = body[idx..].find('>')? + idx;
    let close = format!("</{tag}>");
    let end = body[gt + 1..].find(&close)? + gt + 1;
    let text = unescape_xml(body[gt + 1..end].trim());
    (!text.is_empty()).then_some(text)
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Parses XMLTV timestamps like `20261117220000 +0300` or `20261117220000` into Unix epoch seconds.
pub fn parse_xmltv_timestamp(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    let digits = trimmed.get(..14)?;
    let year: i64 = digits[0..4].parse().ok()?;
    let month: u32 = digits[4..6].parse().ok()?;
    let day: u32 = digits[6..8].parse().ok()?;
    let hour: i64 = digits[8..10].parse().ok()?;
    let min: i64 = digits[10..12].parse().ok()?;
    let sec: i64 = digits[12..14].parse().ok()?;

    let days = days_from_civil(year, month, day)?;
    let mut epoch = days * 86_400 + hour * 3600 + min * 60 + sec;

    if let Some(tz_part) = trimmed.get(14..).map(str::trim).filter(|s| s.len() >= 5) {
        let sign: i64 = match tz_part.as_bytes()[0] {
            b'+' => 1,
            b'-' => -1,
            _ => 0,
        };
        if sign != 0
            && let (Ok(th), Ok(tm)) = (tz_part[1..3].parse::<i64>(), tz_part[3..5].parse::<i64>())
        {
            epoch -= sign * (th * 3600 + tm * 60);
        }
    }
    Some(epoch)
}

// Howard Hinnant's civil-from-days inverse (UTC).
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
