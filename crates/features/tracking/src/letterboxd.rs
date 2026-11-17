//! Letterboxd read-only RSS reader and official CSV importer exporter (§5.14; zero scraping).

#[derive(Debug, Clone, PartialEq)]
pub struct LetterboxdEntry {
    pub title: String,
    pub year: Option<u32>,
    pub watched_date: Option<String>,
    pub rating: Option<f32>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetterboxdCsvRow {
    pub title: String,
    pub year: Option<u32>,
    pub watched_date: Option<String>,
    pub rating10: Option<f32>,
    pub review: Option<String>,
}

pub fn parse_rss(xml: &str) -> Vec<LetterboxdEntry> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        let after = &rest[start + 6..];
        let Some(end) = after.find("</item>") else {
            break;
        };
        let block = &after[..end];
        let title = tag(block, "letterboxd:filmTitle")
            .or_else(|| tag(block, "title"))
            .unwrap_or_default();
        if !title.is_empty() {
            out.push(LetterboxdEntry {
                title,
                year: tag(block, "letterboxd:filmYear").and_then(|s| s.parse().ok()),
                watched_date: tag(block, "letterboxd:watchedDate"),
                rating: tag(block, "letterboxd:memberRating").and_then(|s| s.parse().ok()),
                link: tag(block, "link"),
            });
        }
        rest = &after[end + 7..];
    }
    out
}

/// Formats rows into Letterboxd's official CSV import header:
/// `Title,Year,WatchedDate,Rating10,Review`
pub fn export_csv(rows: &[LetterboxdCsvRow]) -> String {
    let mut out = String::from("Title,Year,WatchedDate,Rating10,Review\n");
    for r in rows {
        let title = csv_escape(&r.title);
        let year = r.year.map_or_else(String::new, |y| y.to_string());
        let date = r.watched_date.as_deref().unwrap_or("");
        let rating = r.rating10.map_or_else(String::new, |x| format!("{x:.1}"));
        let review = csv_escape(r.review.as_deref().unwrap_or(""));
        out.push_str(&format!("{title},{year},{date},{rating},{review}\n"));
    }
    out
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)? + s;
    Some(xml[s..e].trim().to_owned())
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}
