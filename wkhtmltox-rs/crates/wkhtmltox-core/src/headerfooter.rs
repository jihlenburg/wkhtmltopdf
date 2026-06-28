// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Variable header/footer token substitution.
// Pure safe Rust; no I/O, no external date libraries — only `std::time`.

/// Per-page context for header/footer token substitution.
#[derive(Debug, Clone, Default)]
pub struct PageCtx {
    /// 1-based page number of the current page.
    pub page: u32,
    /// 1-based number of the last page.
    pub topage: u32,
    /// 1-based number of the first page (typically 1).
    pub frompage: u32,
    /// URL of the HTML page being converted (empty if not available).
    pub webpage: String,
    /// Active level-1 outline section title (empty if none precedes this page).
    pub section: String,
    /// Active level-2 outline subsection title (empty if none precedes this page).
    pub subsection: String,
    /// Active level-3 outline subsubsection title (empty if none precedes this page).
    pub subsubsection: String,
    /// Current date, UTC, formatted as `YYYY-MM-DD`.
    pub date: String,
    /// Current date in ISO 8601 format (`YYYY-MM-DD`), same as `date`.
    pub isodate: String,
    /// Current time, UTC, formatted as `HH:MM:SS`.
    pub time: String,
    /// Document title passed via `AssembleOpts::doc_title`.
    pub title: String,
    /// Overall document title (same as `title` for single-input documents).
    pub doctitle: String,
    /// 1-based page number within the current input document ("site").
    pub sitepage: u32,
    /// Total number of pages in the current input document ("site").
    pub sitepages: u32,
}

/// Substitute all recognised tokens in `template` with values from `ctx`.
///
/// Recognised tokens:
/// `[page]`, `[topage]`, `[frompage]`, `[section]`, `[subsection]`,
/// `[title]`, `[date]`, `[time]`.
///
/// Unknown tokens are left verbatim.  No escaping is applied here — the
/// PDF content-stream escaping is handled by the C++ shim.
pub fn substitute(template: &str, ctx: &PageCtx) -> String {
    template
        .replace("[page]", &ctx.page.to_string())
        .replace("[topage]", &ctx.topage.to_string())
        .replace("[frompage]", &ctx.frompage.to_string())
        .replace("[section]", &ctx.section)
        .replace("[subsection]", &ctx.subsection)
        .replace("[title]", &ctx.title)
        .replace("[date]", &ctx.date)
        .replace("[time]", &ctx.time)
}

/// Percent-encode a string value for inclusion in a URL query component.
///
/// Encodes: space → `%20`, `%` → `%25`, `&` → `%26`, `+` → `%2B`,
/// `=` → `%3D`, `?` → `%3F`, `#` → `%23`, and any non-ASCII byte → `%XX`.
/// Other printable ASCII characters are left as-is.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for byte in s.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'&' => out.push_str("%26"),
            b'+' => out.push_str("%2B"),
            b'=' => out.push_str("%3D"),
            b'?' => out.push_str("%3F"),
            b'#' => out.push_str("%23"),
            b' ' => out.push_str("%20"),
            0x21..=0x7E => out.push(byte as char),
            b => {
                let hi = b >> 4;
                let lo = b & 0xF;
                out.push('%');
                out.push(char::from_digit(u32::from(hi), 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit(u32::from(lo), 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

/// Build the URL that will be loaded for an HTML header or footer, with all
/// wkhtmltopdf per-page variables appended as URL-encoded query parameters.
///
/// The 13 standard parameters are appended in upstream order:
/// `page`, `frompage`, `topage`, `webpage`, `section`, `subsection`,
/// `subsubsection`, `date`, `isodate`, `time`, `title`, `doctitle`,
/// `sitepage`, `sitepages`.
///
/// Any `replacements` (`--replace name value` pairs) are appended after the
/// standard parameters.
///
/// If `base_url` already contains a `?`, the parameters are appended with
/// `&`; otherwise a `?` separator is used.
pub fn header_footer_query(
    base_url: &str,
    ctx: &PageCtx,
    replacements: &[(String, String)],
) -> String {
    let sep = if base_url.contains('?') { '&' } else { '?' };

    let pairs: [(&str, String); 14] = [
        ("page", ctx.page.to_string()),
        ("frompage", ctx.frompage.to_string()),
        ("topage", ctx.topage.to_string()),
        ("webpage", percent_encode(&ctx.webpage)),
        ("section", percent_encode(&ctx.section)),
        ("subsection", percent_encode(&ctx.subsection)),
        ("subsubsection", percent_encode(&ctx.subsubsection)),
        ("date", percent_encode(&ctx.date)),
        ("isodate", percent_encode(&ctx.isodate)),
        ("time", percent_encode(&ctx.time)),
        ("title", percent_encode(&ctx.title)),
        ("doctitle", percent_encode(&ctx.doctitle)),
        ("sitepage", ctx.sitepage.to_string()),
        ("sitepages", ctx.sitepages.to_string()),
    ];

    let mut url = String::with_capacity(base_url.len() + 256);
    url.push_str(base_url);
    url.push(sep);
    let mut first = true;
    for (k, v) in &pairs {
        if !first {
            url.push('&');
        }
        first = false;
        url.push_str(k);
        url.push('=');
        url.push_str(v);
    }
    for (k, v) in replacements {
        url.push('&');
        url.push_str(&percent_encode(k));
        url.push('=');
        url.push_str(&percent_encode(v));
    }
    url
}

/// Find the active section/subsection title for a given 0-based page index.
///
/// Scans `outline` — each entry `(title, page_0based, level)` — in order and
/// returns the last level-1 and level-2 entries whose `page_0based <= page`.
/// Encountering a new level-1 entry resets the tracked subsection.
pub fn active_section_subsection(
    outline: &[(String, u32, u8)],
    page_0based: u32,
) -> (String, String) {
    let mut section = String::new();
    let mut subsection = String::new();
    for (title, page, level) in outline {
        if *page <= page_0based {
            match level {
                1 => {
                    section = title.clone();
                    subsection = String::new(); // new section resets subsection
                }
                2 => {
                    subsection = title.clone();
                }
                _ => {}
            }
        }
    }
    (section, subsection)
}

/// Return a `(date_str, time_str)` pair for the current UTC instant.
///
/// Formats: `YYYY-MM-DD` and `HH:MM:SS`.  Uses only `std::time`; no
/// external crate required.
pub fn now_date_time() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, m, s) = epoch_to_datetime(secs);
    (
        format!("{y:04}-{mo:02}-{d:02}"),
        format!("{h:02}:{m:02}:{s:02}"),
    )
}

/// Convert a Unix timestamp (seconds since epoch) to a naive UTC broken-down
/// time `(year, month, day, hour, minute, second)`.
///
/// Does not account for leap seconds; accurate for dates from 1970 onwards.
fn epoch_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    let min = ((secs / 60) % 60) as u32;
    let hour = ((secs / 3600) % 24) as u32;
    let mut days = secs / 86400;

    let mut year = 1970u32;
    loop {
        let days_in_year: u64 = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths: [u64; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &ml in &month_lengths {
        if days < ml {
            break;
        }
        days -= ml;
        month += 1;
    }

    (year, month, days as u32 + 1, hour, min, sec)
}

fn is_leap_year(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg(test)]
impl PageCtx {
    /// Construct a fully-populated `PageCtx` suitable for use in tests.
    pub fn sample() -> Self {
        PageCtx {
            page: 1,
            topage: 5,
            frompage: 1,
            webpage: "http://example.com/".into(),
            section: "Section".into(),
            subsection: "Sub".into(),
            subsubsection: "".into(),
            date: "2026-06-28".into(),
            isodate: "2026-06-28".into(),
            time: "12:00:00".into(),
            title: "Sample".into(),
            doctitle: "Sample Doc".into(),
            sitepage: 1,
            sitepages: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> PageCtx {
        PageCtx {
            page: 2,
            topage: 5,
            frompage: 1,
            webpage: "".into(),
            section: "Introduction".into(),
            subsection: "Overview".into(),
            subsubsection: "".into(),
            date: "2026-06-28".into(),
            isodate: "2026-06-28".into(),
            time: "12:00:00".into(),
            title: "My Doc".into(),
            doctitle: "My Doc".into(),
            sitepage: 2,
            sitepages: 5,
        }
    }

    #[test]
    fn page_and_topage() {
        let c = test_ctx();
        assert_eq!(substitute("[page] of [topage]", &c), "2 of 5");
    }

    #[test]
    fn section_and_title() {
        let c = test_ctx();
        assert_eq!(
            substitute("[section] \u{2014} [title]", &c),
            "Introduction \u{2014} My Doc"
        );
    }

    #[test]
    fn no_tokens_pass_through() {
        let c = test_ctx();
        assert_eq!(substitute("static text", &c), "static text");
    }

    #[test]
    fn frompage_and_subsection() {
        let c = test_ctx();
        assert_eq!(substitute("[frompage] [subsection]", &c), "1 Overview");
    }

    #[test]
    fn date_and_time() {
        let c = test_ctx();
        assert_eq!(substitute("[date] [time]", &c), "2026-06-28 12:00:00");
    }

    #[test]
    fn active_section_finds_last_before_page() {
        let outline = vec![
            ("Intro".to_string(), 0u32, 1u8),
            ("Sub1".to_string(), 1u32, 2u8),
            ("Chapter 2".to_string(), 3u32, 1u8),
        ];
        // page 0: only "Intro" active (level 1), no subsection yet
        assert_eq!(
            active_section_subsection(&outline, 0),
            ("Intro".to_string(), "".to_string())
        );
        // page 1: "Intro" + "Sub1"
        assert_eq!(
            active_section_subsection(&outline, 1),
            ("Intro".to_string(), "Sub1".to_string())
        );
        // page 2: still "Intro" + "Sub1" (nothing new on page 2)
        assert_eq!(
            active_section_subsection(&outline, 2),
            ("Intro".to_string(), "Sub1".to_string())
        );
        // page 3: "Chapter 2" is a new level-1; subsection resets
        assert_eq!(
            active_section_subsection(&outline, 3),
            ("Chapter 2".to_string(), "".to_string())
        );
    }

    #[test]
    fn active_section_empty_outline() {
        assert_eq!(
            active_section_subsection(&[], 0),
            ("".to_string(), "".to_string())
        );
    }

    #[test]
    fn epoch_to_datetime_epoch_zero() {
        // Unix epoch: 1970-01-01 00:00:00
        let (y, mo, d, h, m, s) = epoch_to_datetime(0);
        assert_eq!((y, mo, d, h, m, s), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn epoch_to_datetime_known_date() {
        // 2000-01-01 00:00:00 = 946684800
        let (y, mo, d, h, m, s) = epoch_to_datetime(946_684_800);
        assert_eq!((y, mo, d, h, m, s), (2000, 1, 1, 0, 0, 0));
    }

    #[test]
    fn builds_query_string_with_all_tokens() {
        let ctx = PageCtx {
            page: 3,
            frompage: 1,
            topage: 10,
            webpage: "http://x/".into(),
            section: "S".into(),
            subsection: "Sub".into(),
            subsubsection: "".into(),
            date: "2026-06-28".into(),
            isodate: "2026-06-28".into(),
            time: "12:00".into(),
            title: "T".into(),
            doctitle: "Doc".into(),
            sitepage: 3,
            sitepages: 10,
        };
        let url = header_footer_query("header.html", &ctx, &[]);
        assert!(url.starts_with("header.html?"));
        assert!(url.contains("page=3"));
        assert!(url.contains("topage=10"));
        assert!(url.contains("doctitle=Doc"));
        assert!(url.contains("frompage=1"));
    }

    #[test]
    fn url_encodes_values_and_appends_replacements() {
        let ctx = PageCtx { title: "a&b c".into(), ..PageCtx::sample() };
        let url = header_footer_query("h.html", &ctx, &[("co".into(), "A & B".into())]);
        assert!(url.contains("title=a%26b%20c") || url.contains("title=a%26b+c"));
        assert!(url.contains("co=A%20%26%20B") || url.contains("co=A+%26+B"));
    }

    #[test]
    fn preserves_existing_query_in_base_url() {
        let url = header_footer_query("h.html?x=1", &PageCtx::sample(), &[]);
        assert!(url.contains("h.html?x=1&") && url.contains("page="));
    }
}
