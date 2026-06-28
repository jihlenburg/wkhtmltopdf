// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Outline → XML and the default TOC XSLT, plus a browser-driven XSLT transform.
//! Matches wkhtmltopdf's `<outline>` schema + default stylesheet so `--xsl-style-sheet`
//! is drop-in compatible. XSLT is applied via the rendering engine's `XSLTProcessor`
//! (no native libxslt dependency).

use crate::render::{LoadSettings, ReadyPolicy, Renderer, Source};

/// Transform `xml` with `xsl` using the rendering engine's XSLTProcessor.
///
/// Opens a blank page, evaluates a JS snippet that runs `DOMParser` + `XSLTProcessor`,
/// and returns the serialized output HTML.  No network is performed.
///
/// Fails closed: any parse error, transform failure, or empty result returns `Err`.
pub fn transform_toc(r: &mut dyn Renderer, xml: &str, xsl: &str) -> crate::error::Result<String> {
    let page = r.open(
        &Source::Html("<!doctype html><html><body></body></html>".into()),
        &LoadSettings::default(),
    )?;
    r.wait_ready(page, &ReadyPolicy::default())?;
    let xml_lit = serde_json::to_string(xml).unwrap_or_else(|_| "\"\"".into());
    let xsl_lit = serde_json::to_string(xsl).unwrap_or_else(|_| "\"\"".into());
    let script = format!(
        r#"(() => {{
  try {{
    const xml = {xml_lit};
    const xsl = {xsl_lit};
    const p = new DOMParser();
    const xmlDoc = p.parseFromString(xml, "application/xml");
    const xslDoc = p.parseFromString(xsl, "application/xml");
    if (xmlDoc.querySelector("parsererror") || xslDoc.querySelector("parsererror"))
      return {{ ok: false, err: "parse error in outline XML or stylesheet" }};
    const proc = new XSLTProcessor();
    proc.importStylesheet(xslDoc);
    const out = proc.transformToDocument(xmlDoc);
    if (!out) return {{ ok: false, err: "XSLT transform returned null" }};
    const html = new XMLSerializer().serializeToString(out);
    if (!html) return {{ ok: false, err: "XSLT serializer returned empty string" }};
    return {{ ok: true, html }};
  }} catch (e) {{ return {{ ok: false, err: String(e) }}; }}
}})()"#,
        xml_lit = xml_lit,
        xsl_lit = xsl_lit
    );
    let v = r.eval_json(page, &script)?;
    if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        let html = v.get("html").and_then(|h| h.as_str()).unwrap_or("").to_string();
        if html.is_empty() {
            return Err(crate::error::WkError::Xslt(
                "XSLT transform produced empty output".into(),
            ));
        }
        Ok(html)
    } else {
        let err = v
            .get("err")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown XSLT error");
        Err(crate::error::WkError::Xslt(format!(
            "XSLT TOC transform failed: {err}"
        )))
    }
}

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
    /// When `true`, emit `<a href="…">` links in the TOC pointing from each entry to its
    /// heading anchor.
    ///
    /// **Note (deferred):** `outline_to_xml` does not yet emit `link` attributes on
    /// `<item>` elements because the `__WKANCHOR` anchor infrastructure required for
    /// TOC clickable-link wiring was deferred in M2b.  The generated outline XML
    /// therefore never includes `link="…"`, so the default XSL's `@link` conditional
    /// never fires and this toggle is currently accepted but does not change output.
    /// Enabling this flag will take effect once anchor synthesis is wired in.
    pub forward_links: bool,
    /// When `true`, each heading in the document links back to its TOC entry.
    ///
    /// **Note (deferred):** same as `forward_links` — `backLink` attribute emission
    /// is deferred pending TOC anchor infrastructure.  This toggle is accepted but
    /// currently does not change output.
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
///
/// # Upstream compatibility: synthetic document-root `<item>`
///
/// The real wkhtmltopdf 0.12.6 binary ALWAYS wraps the real heading items inside a
/// synthetic document-root `<item title="" page="0">…</item>` directly under
/// `<outline>`.  The default TOC stylesheet relies on this via its selector
/// `select="outline:item/outline:item"` — without the wrapper every top-level heading
/// is a direct child of `<outline>` and is silently dropped.  This function therefore
/// always emits that wrapper, matching upstream's schema exactly:
///
/// ```xml
/// <outline xmlns="http://wkhtmltopdf.org/outline">
///   <item title="" page="0">      <!-- synthetic root — always present -->
///     <item title="Alpha" page="1"/>
///     …
///   </item>
/// </outline>
/// ```
///
/// When `entries` is empty the synthetic root is self-closed:
/// `<item title="" page="0"/>`.
///
/// # Deferred: `link` / `backLink` attributes
///
/// This function does **not** emit `link` or `backLink` attributes on `<item>`
/// elements.  The `__WKANCHOR` anchor infrastructure required for TOC clickable-link
/// wiring was deferred in M2b.  Emitting empty `link=""`/`backLink=""` would create
/// broken `href=""` links, so these attributes are omitted until anchor synthesis
/// is implemented.  As a result the `forward_links` / `back_links` toggles on
/// [`TocXslSettings`] are currently no-ops.
pub fn outline_to_xml(entries: &[(String, u32, u8)]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<outline xmlns=\"http://wkhtmltopdf.org/outline\">\n");

    if entries.is_empty() {
        // Upstream always emits the synthetic root even for empty outlines.
        out.push_str("  <item title=\"\" page=\"0\"/>\n");
        out.push_str("</outline>\n");
        return out;
    }

    // Synthetic document-root required by upstream's default XSL selector
    // `select="outline:item/outline:item"`.
    out.push_str("  <item title=\"\" page=\"0\">\n");

    // open_levels holds the level of each currently-open (non-self-closed) <item>.
    let mut open_levels: Vec<u8> = Vec::new();
    // Base indent is 2 (inside the synthetic root) plus one per open level.
    let indent = |n: usize| "  ".repeat(n + 2);
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

    out.push_str("  </item>\n");
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
        // Upstream-compatible: synthetic root is always present.
        assert!(xml.contains("<item title=\"\" page=\"0\">"));
        // Intro is a parent of Background (deeper level) → open/close form;
        // real items are now nested inside the synthetic root.
        assert!(xml.contains("<item title=\"Intro\" page=\"1\">"));
        assert!(xml.contains("<item title=\"Background\" page=\"2\"/>"));
        // Method is a leaf at level 1 → self-closing (inside synthetic root)
        assert!(xml.contains("<item title=\"Method\" page=\"3\"/>"));
        // Intro and Background must appear AFTER the synthetic-root opening tag.
        let root_pos = xml.find("<item title=\"\" page=\"0\">").unwrap();
        let intro_pos = xml.find("<item title=\"Intro\"").unwrap();
        assert!(
            intro_pos > root_pos,
            "Intro must be nested inside the synthetic root"
        );
        assert!(xml.trim_end().ends_with("</outline>"));
    }

    #[test]
    fn outline_xml_escapes_titles() {
        let xml = outline_to_xml(&[("A & <B>".to_string(), 1, 1)]);
        assert!(xml.contains("title=\"A &amp; &lt;B&gt;\""));
        // Escaped title must appear inside the synthetic root.
        let root_pos = xml.find("<item title=\"\" page=\"0\">").unwrap();
        let title_pos = xml.find("title=\"A &amp;").unwrap();
        assert!(title_pos > root_pos, "escaped title must be inside the synthetic root");
    }

    #[test]
    fn outline_xml_has_synthetic_root() {
        // 1. Always contains the synthetic root.
        let xml_multi = outline_to_xml(&[
            ("Alpha".to_string(), 1u32, 1u8),
            ("Alpha-Sub".to_string(), 2, 2),
            ("Beta".to_string(), 3, 1),
        ]);
        assert!(
            xml_multi.contains("<item title=\"\" page=\"0\">"),
            "multi-entry: synthetic root must be present"
        );
        // Real items must appear after the synthetic root opening tag.
        let root_pos = xml_multi.find("<item title=\"\" page=\"0\">").unwrap();
        let alpha_pos = xml_multi.find("<item title=\"Alpha\"").unwrap();
        assert!(
            alpha_pos > root_pos,
            "Alpha must be nested inside the synthetic root, not a direct child of <outline>"
        );

        // 2. Single-heading input: heading nested inside root, not direct child of <outline>.
        let xml_single = outline_to_xml(&[("OnlyOne".to_string(), 1u32, 1u8)]);
        assert!(
            xml_single.contains("<item title=\"\" page=\"0\">"),
            "single-entry: synthetic root must be present (open form)"
        );
        let root_pos_s = xml_single.find("<item title=\"\" page=\"0\">").unwrap();
        let heading_pos_s = xml_single.find("<item title=\"OnlyOne\"").unwrap();
        assert!(
            heading_pos_s > root_pos_s,
            "single heading must be nested inside the synthetic root"
        );

        // 3. Empty input: synthetic root is self-closed.
        let xml_empty = outline_to_xml(&[]);
        assert!(
            xml_empty.contains("<item title=\"\" page=\"0\"/>"),
            "empty input: synthetic root must be self-closed"
        );
        assert!(
            !xml_empty.contains("<item title=\"\" page=\"0\">"),
            "empty input: synthetic root must NOT be open-form"
        );
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
