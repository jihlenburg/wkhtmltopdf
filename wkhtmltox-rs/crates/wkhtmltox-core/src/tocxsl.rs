// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Outline → XML and the default TOC XSLT, plus a browser-driven XSLT transform.
//! Matches wkhtmltopdf's `<outline>` schema + default stylesheet so `--xsl-style-sheet`
//! is drop-in compatible. XSLT is applied via the rendering engine's `XSLTProcessor`
//! (no native libxslt dependency).

/// XML-escape text for an attribute/element value.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Settings controlling the built-in default TOC (mirror upstream `TableOfContent`).
#[derive(Debug, Clone)]
pub struct TocXslSettings {
    pub caption_text: String,
    pub use_dotted_lines: bool,
    pub forward_links: bool,
    pub back_links: bool,
    pub indentation: String,
    pub font_scale: f64,
}
impl Default for TocXslSettings {
    fn default() -> Self {
        Self {
            caption_text: "Table of Contents".to_string(),
            use_dotted_lines: true,
            forward_links: true,
            back_links: false,
            indentation: "1em".to_string(),
            font_scale: 0.8,
        }
    }
}

/// Serialize the (title, 1-based page, level) entries to wkhtmltopdf's outline XML.
/// Nesting is reconstructed from `level` with a stack: an entry deeper than the
/// previous opens children; shallower/equal closes back to its level.
pub fn outline_to_xml(entries: &[(String, u32, u8)]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<outline xmlns=\"http://wkhtmltopdf.org/outline\">\n");
    // open_levels holds the level of each currently-open (non-self-closed) <item>.
    let mut open_levels: Vec<u8> = Vec::new();
    let indent = |n: usize| "  ".repeat(n + 1);
    for (i, (title, page, level)) in entries.iter().enumerate() {
        // Close any open items at >= this level.
        while let Some(&top) = open_levels.last() {
            if top >= *level {
                open_levels.pop();
                out.push_str(&indent(open_levels.len()));
                out.push_str("</item>\n");
            } else {
                break;
            }
        }
        let next_deeper = entries.get(i + 1).map(|(_, _, l)| *l > *level).unwrap_or(false);
        out.push_str(&indent(open_levels.len()));
        out.push_str(&format!(
            "<item title=\"{}\" page=\"{}\"",
            xml_escape(title),
            page
        ));
        if next_deeper {
            out.push_str(">\n");
            open_levels.push(*level);
        } else {
            out.push_str("/>\n");
        }
    }
    while open_levels.pop().is_some() {
        out.push_str(&indent(open_levels.len()));
        out.push_str("</item>\n");
    }
    out.push_str("</outline>\n");
    out
}

/// Generate the default TOC stylesheet (1.0-compatible; XSLTProcessor runs XSLT 1.0).
/// Reproduces upstream's structure: an `outline:outline` template (page + CSS) and a
/// recursive `outline:item` template (li/div/a/span with floating page number).
pub fn default_toc_xsl(s: &TocXslSettings) -> String {
    let dotted = if s.use_dotted_lines {
        "div { border-bottom: 1px dashed rgb(200,200,200); }\n"
    } else {
        ""
    };
    let font_pct = (s.font_scale * 100.0).round() as i64;
    let href_attr = if s.forward_links {
        "<xsl:if test=\"@link\"><xsl:attribute name=\"href\"><xsl:value-of select=\"@link\"/></xsl:attribute></xsl:if>"
    } else {
        ""
    };
    let name_attr = if s.back_links {
        "<xsl:if test=\"@backLink\"><xsl:attribute name=\"name\"><xsl:value-of select=\"@backLink\"/></xsl:attribute></xsl:if>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0"
    xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:outline="http://wkhtmltopdf.org/outline"
    xmlns="http://www.w3.org/1999/xhtml">
  <xsl:output method="html" indent="yes"/>
  <xsl:template match="outline:outline">
    <html><head><style>
      h1 {{ text-align: center; font-size: 20px; font-family: arial; }}
      {dotted}      span {{ float: right; }}
      li {{ list-style: none; }}
      ul {{ font-size: 20px; font-family: arial; padding-left: 0em; }}
      ul ul {{ font-size: {font_pct}%; padding-left: {indent}; }}
      a {{ text-decoration: none; color: black; }}
    </style></head><body>
      <h1>{caption}</h1>
      <ul><xsl:apply-templates select="outline:item/outline:item"/></ul>
    </body></html>
  </xsl:template>
  <xsl:template match="outline:item">
    <li>
      <xsl:if test="@title!=''">
        <div>
          <a>{href}{name}<xsl:value-of select="@title"/></a>
          <span><xsl:value-of select="@page"/></span>
        </div>
      </xsl:if>
      <ul><xsl:apply-templates select="outline:item"/></ul>
    </li>
  </xsl:template>
</xsl:stylesheet>
"#,
        dotted = dotted,
        font_pct = font_pct,
        indent = xml_escape(&s.indentation),
        caption = xml_escape(&s.caption_text),
        href = href_attr,
        name = name_attr,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escapes_special_chars() {
        assert_eq!(xml_escape(r#"a & b <c> "d" 'e'"#), "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;");
    }

    #[test]
    fn outline_xml_schema_and_nesting() {
        let entries = vec![
            ("Intro".to_string(), 1u32, 1u8),
            ("Background".to_string(), 2, 2),
            ("Method".to_string(), 3, 1),
        ];
        let xml = outline_to_xml(&entries);
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<outline xmlns=\"http://wkhtmltopdf.org/outline\">"));
        // Intro is a parent of Background (deeper level) → open/close form
        assert!(xml.contains("<item title=\"Intro\" page=\"1\">"));
        assert!(xml.contains("<item title=\"Background\" page=\"2\"/>"));
        // Method is a leaf at level 1 → self-closing
        assert!(xml.contains("<item title=\"Method\" page=\"3\"/>"));
        assert!(xml.trim_end().ends_with("</outline>"));
    }

    #[test]
    fn outline_xml_escapes_titles() {
        let xml = outline_to_xml(&[("A & <B>".to_string(), 1, 1)]);
        assert!(xml.contains("title=\"A &amp; &lt;B&gt;\""));
    }

    #[test]
    fn default_xsl_has_namespace_templates_and_settings() {
        let s = TocXslSettings::default();
        let xsl = default_toc_xsl(&s);
        assert!(xsl.contains("xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\""));
        assert!(xsl.contains("xmlns:outline=\"http://wkhtmltopdf.org/outline\""));
        assert!(xsl.contains("match=\"outline:outline\""));
        assert!(xsl.contains("match=\"outline:item\""));
        assert!(xsl.contains("Table of Contents")); // default caption
        assert!(xsl.contains("dashed"));            // dotted lines on by default
        assert!(xsl.contains("padding-left: 1em")); // default indentation
    }

    #[test]
    fn default_xsl_omits_dotted_when_disabled() {
        let s = TocXslSettings { use_dotted_lines: false, ..Default::default() };
        assert!(!default_toc_xsl(&s).contains("dashed"));
    }
}
