// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Table-of-Contents HTML generation.
// Pure safe Rust; no I/O, no allocator tricks — just string construction.

/// Escape a string for safe embedding in HTML text content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Generate a self-contained HTML document representing the Table of Contents.
///
/// Each entry is `(title, page_1based, level)`.  Level 1 entries are at the
/// root indent; each additional level adds 1.5 em of left padding.  A dotted
/// leader (CSS `border-bottom`) separates the title from the page number.
/// Titles are HTML-escaped so arbitrary UTF-8 heading text is safe.
pub fn render_toc_html(entries: &[(String, u32, u8)]) -> String {
    let mut rows = String::new();
    for (title, page, level) in entries {
        let indent_em = (*level as f64 - 1.0) * 1.5;
        let escaped = html_escape(title);
        rows.push_str(&format!(
            "<div style=\"display:flex;align-items:baseline;\
             padding-left:{indent_em:.1}em;margin:0.25em 0\">\
             <span>{escaped}</span>\
             <span style=\"flex:1;border-bottom:1px dotted #888;\
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
         body{{font-family:serif;font-size:12pt;margin:1cm 2cm}}\
         h1{{font-size:16pt;border-bottom:1px solid #333;margin-bottom:0.5em}}\
         </style>\n\
         </head>\n\
         <body>\n\
         <h1>Table of Contents</h1>\n\
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
        let html = render_toc_html(&entries);
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
        let html = render_toc_html(&entries);
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
        let html = render_toc_html(&entries);
        // Level 1 → 0.0 em, level 2 → 1.5 em, level 3 → 3.0 em
        assert!(html.contains("padding-left:0.0em"));
        assert!(html.contains("padding-left:1.5em"));
        assert!(html.contains("padding-left:3.0em"));
    }
}
