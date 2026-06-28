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
    /// Active level-1 outline section title (empty if none precedes this page).
    pub section: String,
    /// Active level-2 outline subsection title (empty if none precedes this page).
    pub subsection: String,
    /// Document title passed via `AssembleOpts::doc_title`.
    pub title: String,
    /// Current date, UTC, formatted as `YYYY-MM-DD`.
    pub date: String,
    /// Current time, UTC, formatted as `HH:MM:SS`.
    pub time: String,
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
mod tests {
    use super::*;

    fn test_ctx() -> PageCtx {
        PageCtx {
            page: 2,
            topage: 5,
            frompage: 1,
            section: "Introduction".into(),
            subsection: "Overview".into(),
            title: "My Doc".into(),
            date: "2026-06-28".into(),
            time: "12:00:00".into(),
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
}
