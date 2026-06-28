// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Table-of-Contents HTML generation.
// Pure safe Rust; no I/O, no allocator tricks — just string construction.
use crate::tocxsl::TocXslSettings;

/// Escape a string for safe embedding in HTML text content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Parse the numeric factor from a CSS length value such as `"1em"` or `"2.5em"`.
/// Strips any trailing unit suffix (em, rem, px, pt, …) and parses the remainder.
/// Returns 1.0 if parsing fails.
fn parse_indent_factor(css: &str) -> f64 {
    let stripped = css
        .trim()
        .trim_end_matches("rem")
        .trim_end_matches("em")
        .trim_end_matches("px")
        .trim_end_matches("pt")
        .trim_end_matches('%')
        .trim();
    stripped.parse().unwrap_or(1.0)
}

/// Generate a self-contained HTML document representing the Table of Contents.
///
/// Each entry is `(title, page_1based, level)`.  Settings from `s` control the
/// caption text, whether dotted leaders are shown, per-level indentation, and
/// overall font scale.  Titles are HTML-escaped.
///
/// When `s` is `TocXslSettings::default()` the output is equivalent to the
/// previous (single-argument) behaviour: "Table of Contents" heading, dotted
/// leaders, 1.5 em per level of indentation.
pub fn render_toc_html(entries: &[(String, u32, u8)], s: &TocXslSettings) -> String {
    // Parse the indentation CSS value to a numeric factor.
    // The per-level step is `factor * 1.5` em so that the default "1em" produces
    // the same 1.5 em/level as the original implementation.
    let indent_factor = parse_indent_factor(&s.indentation);
    let font_pct = (s.font_scale * 100.0).round() as i64;
    let caption = html_escape(&s.caption_text);

    let mut rows = String::new();
    for (title, page, level) in entries {
        let indent_em = (*level as f64 - 1.0) * indent_factor * 1.5;
        let escaped = html_escape(title);
        let leader_style = if s.use_dotted_lines {
            "border-bottom:1px dotted #888;"
        } else {
            ""
        };
        rows.push_str(&format!(
            "<div style=\"display:flex;align-items:baseline;\
             padding-left:{indent_em:.1}em;margin:0.25em 0\">\
             <span>{escaped}</span>\
             <span style=\"flex:1;{leader_style}\
             margin:0 0.4em\"></span>\
             <span>{page}</span></div>\n"
        ));
    }
    format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <style>\
         body{{font-family:serif;font-size:{font_pct}%;margin:1cm 2cm}}\
         h1{{font-size:16pt;border-bottom:1px solid #333;margin-bottom:0.5em}}\
         </style>\n\
         </head>\n\
         <body>\n\
         <h1>{caption}</h1>\n\
         {rows}</body>\n\
         </html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_toc_html_basic() {
        let entries = vec![
            ("Intro".to_string(), 1u32, 1u8),
            ("Sub".to_string(), 2u32, 2u8),
        ];
        let html = render_toc_html(&entries, &TocXslSettings::default());
        assert!(html.contains("Intro"), "should contain 'Intro'");
        assert!(html.contains("Sub"), "should contain 'Sub'");
        assert!(html.contains('1'), "should contain page '1'");
        assert!(html.contains('2'), "should contain page '2'");
        assert!(
            html.contains("Table of Contents"),
            "should contain 'Table of Contents'"
        );
    }

    #[test]
    fn html_escape_in_titles() {
        let entries = vec![
            ("A & B".to_string(), 1u32, 1u8),
            ("<script>".to_string(), 2u32, 1u8),
        ];
        let html = render_toc_html(&entries, &TocXslSettings::default());
        assert!(html.contains("A &amp; B"), "ampersand must be escaped");
        assert!(
            html.contains("&lt;script&gt;"),
            "angle brackets must be escaped"
        );
    }

    #[test]
    fn level_indentation_increases() {
        let entries = vec![
            ("A".to_string(), 1u32, 1u8),
            ("A.1".to_string(), 2u32, 2u8),
            ("A.1.1".to_string(), 3u32, 3u8),
        ];
        let html = render_toc_html(&entries, &TocXslSettings::default());
        // Level 1 → 0.0 em, level 2 → 1.5 em, level 3 → 3.0 em
        // (default indentation "1em" × 1.5 factor = 1.5 em/level)
        assert!(html.contains("padding-left:0.0em"));
        assert!(html.contains("padding-left:1.5em"));
        assert!(html.contains("padding-left:3.0em"));
    }

    #[test]
    fn settings_caption_and_no_dotted_lines() {
        let s = TocXslSettings {
            caption_text: "Índice".to_string(),
            use_dotted_lines: false,
            ..Default::default()
        };
        let entries = vec![("A".to_string(), 1u32, 1u8)];
        let html = render_toc_html(&entries, &s);
        assert!(html.contains("Índice"), "custom caption must appear");
        assert!(
            !html.contains("dotted"),
            "dotted lines must be absent when use_dotted_lines=false"
        );
    }
}
