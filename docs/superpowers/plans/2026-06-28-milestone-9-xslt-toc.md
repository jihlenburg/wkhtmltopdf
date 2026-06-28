# wkhtmltox-rs Milestone 9 — Custom-XSLT Table of Contents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Support `--xsl-style-sheet` (and the `--toc-*` settings + `--dump-outline`/`--dump-default-toc-xsl`) so a user can control the Table of Contents via XSLT, matching wkhtmltopdf's behavior — applied through Chromium's built-in `XSLTProcessor` (no new native dependency).

**Architecture:** Serialize the document outline to wkhtmltopdf's exact `<outline>` XML schema (pure Rust). When the user supplies an XSL stylesheet, transform that XML → TOC HTML in-browser via `XSLTProcessor` (driven over CDP `Runtime.evaluate`), then feed the result into the existing M2b TOC fixed-point page-number loop. Without `--xsl-style-sheet`, the existing built-in `render_toc_html` default is used (now honoring the `--toc-*` settings). A built-in default XSLT (pure Rust generator) reproduces upstream's stylesheet for `--dump-default-toc-xsl`.

**Tech Stack:** Rust 2021 (`wkhtmltox-core` stays `#![forbid(unsafe_code)]`), Chromium `XSLTProcessor`/`DOMParser`/`XMLSerializer` via CDP `Runtime.evaluate`. No libxslt, no new FFI.

## Global Constraints
- `wkhtmltox-core` + `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`. No new native/FFI dependency (XSLT runs in the browser).
- **Match upstream exactly** where specified: the outline XML schema (`<outline xmlns="http://wkhtmltopdf.org/outline">`, `<item title page link backLink/>`, 2-space indent, XML-escaped) and the default-stylesheet structure + `TableOfContent` defaults (`useDottedLines=true`, `captionText="Table of Contents"`, `forwardLinks=true`, `backLinks=false`, `indentation="1em"`, `fontScale=0.8`).
- **Outline titles are attacker-controlled** (from rendered HTML headings) → MUST be XML-escaped in `outline_to_xml` (`&` `<` `>` `"` `'`). The user-supplied XSL is trusted (a local file the operator chose).
- Preserve the existing default TOC output when no `--xsl-style-sheet` is given (don't regress M2b). LGPL header on new files; `cargo clippy --lib -- -D warnings` clean on touched crates. Commit per task + trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Working dir for commands: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs`.

## File Structure
- `crates/wkhtmltox-core/src/tocxsl.rs` — CREATE: `outline_to_xml`, `TocXslSettings`, `default_toc_xsl`, `transform_toc` (browser), `xml_escape`.
- `crates/wkhtmltox-core/src/lib.rs` — MODIFY: `pub mod tocxsl;`.
- `crates/wkhtmltox-core/src/assembly.rs` — MODIFY: `AssembleOpts` gains `toc_xsl: Option<String>` (XSL file path) + `toc_settings: TocXslSettings`; the fixed-point loop uses the XSLT path when `toc_xsl` is set.
- `crates/wkhtmltox-core/src/toc.rs` — MODIFY: `render_toc_html` honors `TocXslSettings` (caption/dotted/indentation/fontScale).
- `crates/wkhtmltox-core/src/settings.rs` + `registry.rs` — MODIFY: implement `tocXsl` + `toc.*` (move out of warn arms).
- `crates/wkhtmltox-cli/src/lib.rs` — MODIFY: add `--xsl-style-sheet`, `--toc-header-text`, `--disable-toc-links`, `--disable-dotted-lines`, `--toc-text-size-shrink`, `--toc-level-indentation`, `--enable-toc-back-links`, `--dump-outline`, `--dump-default-toc-xsl` to `FLAGS`.
- `crates/wkhtmltopdf-cli/src/main.rs` — MODIFY: handle `--dump-default-toc-xsl` (print + exit 0) and `--dump-outline <file>`.

---

### Task 1 — Outline XML + default TOC XSLT (pure core)

**Files:** Create `crates/wkhtmltox-core/src/tocxsl.rs`; modify `crates/wkhtmltox-core/src/lib.rs`.
**Interfaces:**
- Produces: `pub fn outline_to_xml(entries: &[(String, u32, u8)]) -> String`; `pub struct TocXslSettings { pub caption_text: String, pub use_dotted_lines: bool, pub forward_links: bool, pub back_links: bool, pub indentation: String, pub font_scale: f64 }` + `Default`; `pub fn default_toc_xsl(s: &TocXslSettings) -> String`; `pub fn xml_escape(s: &str) -> String`.
- `entries` are `(title, 1-based-global-page, level)` exactly as `assemble_with_toc` already builds them (`level` ≥ 1).

- [ ] **Step 1: Write failing tests** in `tocxsl.rs`:

```rust
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
```

- [ ] **Step 2: Run, verify failure:** `cargo test -p wkhtmltox-core tocxsl::` → fails to compile (module absent).

- [ ] **Step 3: Implement `tocxsl.rs`** (the pure parts). Add `pub mod tocxsl;` to `lib.rs`. Implement:

```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Outline → XML and the default TOC XSLT, plus a browser-driven XSLT transform.
//! Matches wkhtmltopdf's `<outline>` schema + default stylesheet so `--xsl-style-sheet`
//! is drop-in compatible. XSLT is applied via the rendering engine's `XSLTProcessor`
//! (no native libxslt dependency).
use crate::error::Result;

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
    while let Some(_) = open_levels.pop() {
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
```

(`transform_toc` is added in Task 2 — Task 1 is the pure, no-browser layer.)

- [ ] **Step 4: Run, verify pass:** `cargo test -p wkhtmltox-core tocxsl:: && cargo clippy -p wkhtmltox-core --lib -- -D warnings`.

- [ ] **Step 5: Commit** `feat(toc): outline XML serialization + default TOC XSLT generator (pure)`.

---

### Task 2 — Browser XSLT transform + assembly integration

**Files:** Modify `crates/wkhtmltox-core/src/tocxsl.rs`, `assembly.rs`, `toc.rs`; gated e2e in `crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`.
**Interfaces:**
- Produces: `pub fn transform_toc(r: &mut dyn crate::render::Renderer, xml: &str, xsl: &str) -> Result<String>`; `AssembleOpts.toc_xsl: Option<String>` (XSL file path), `AssembleOpts.toc_settings: TocXslSettings`.
- Consumes: Task 1 `outline_to_xml`/`default_toc_xsl`; the `Renderer` trait (`open`/`eval_json`); the existing `assemble_with_toc` fixed-point loop + `toc::render_toc_html`.

- [ ] **Step 1: Implement `transform_toc`** in `tocxsl.rs` (browser XSLTProcessor via a generic JS eval). Use `serde_json::to_string` to safely embed the XML/XSL as JS string literals:

```rust
use crate::render::{LoadSettings, ReadyPolicy, Renderer, Source};

/// Transform `xml` with `xsl` using the rendering engine's XSLTProcessor.
/// Returns the serialized result HTML. No network is performed by the transform.
pub fn transform_toc(r: &mut dyn Renderer, xml: &str, xsl: &str) -> Result<String> {
    let page = r.open(&Source::Html("<!doctype html><html><body></body></html>".into()), &LoadSettings::default())?;
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
    return {{ ok: true, html: new XMLSerializer().serializeToString(out) }};
  }} catch (e) {{ return {{ ok: false, err: String(e) }}; }}
}})()"#,
        xml_lit = xml_lit,
        xsl_lit = xsl_lit
    );
    let v = r.eval_json(page, &script)?;
    if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(v.get("html").and_then(|h| h.as_str()).unwrap_or("").to_string())
    } else {
        let err = v.get("err").and_then(|e| e.as_str()).unwrap_or("unknown XSLT error");
        Err(crate::error::WkError::Render(format!("XSLT TOC transform failed: {err}")))
    }
}
```

(Confirm the exact `WkError` variant — use whatever the crate uses for render-time failures, e.g. `WkError::Render`.)

- [ ] **Step 2: Extend `render_toc_html` to honor settings.** Change `toc::render_toc_html` to take `(entries: &[(String, u32, u8)], s: &TocXslSettings)` and use `s.caption_text` for the heading, `s.use_dotted_lines` to toggle the leader, `s.indentation`/`s.font_scale` for nested styling. Keep the default output equivalent to today when `s` is `TocXslSettings::default()` (so M2b output doesn't regress — verify the existing TOC test still passes, updating its call site to pass `&TocXslSettings::default()`).

- [ ] **Step 3: Wire into `assemble_with_toc`.** Add `toc_xsl: Option<String>` + `toc_settings: TocXslSettings` to `AssembleOpts` (defaults: `None`, `TocXslSettings::default()`). In the fixed-point loop where `toc_html` is built (currently `crate::toc::render_toc_html(&toc_display)`), branch:

```rust
let toc_html = if let Some(xsl_path) = &opts.toc_xsl {
    let xsl = std::fs::read_to_string(xsl_path)
        .map_err(|e| WkError::Config(format!("cannot read --xsl-style-sheet {xsl_path:?}: {e}")))?;
    let xml = crate::tocxsl::outline_to_xml(&toc_display);
    crate::tocxsl::transform_toc(r, &xml, &xsl)?
} else {
    crate::toc::render_toc_html(&toc_display, &opts.toc_settings)
};
```

(Use the actual `WkError` config/IO variant the crate has.) Everything else in the loop (page counting, convergence, merge order) is unchanged.

- [ ] **Step 4: Gated e2e** in `chromium_e2e.rs` (real Chrome) — a custom XSL produces a TOC containing the heading titles:

```rust
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn custom_xsl_toc_contains_headings() {
    use wkhtmltox_core::{assembly::*, render::*, tocxsl::*};
    // minimal custom XSL: list every item's title in a <p class="tocitem">
    let xsl = r#"<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:o="http://wkhtmltopdf.org/outline" xmlns="http://www.w3.org/1999/xhtml">
  <xsl:template match="o:outline"><html><body><h1>TOC</h1>
    <xsl:for-each select="//o:item"><p class="tocitem"><xsl:value-of select="@title"/></p></xsl:for-each>
  </body></html></xsl:template>
</xsl:stylesheet>"#;
    let dir = tempfile::tempdir().unwrap();
    let xsl_path = dir.path().join("toc.xsl");
    std::fs::write(&xsl_path, xsl).unwrap();
    let mut r = ChromiumRenderer::spawn(&Default::default()).unwrap();
    let html = "<h1>Alpha</h1><p>x</p><h1 style='page-break-before:always'>Beta</h1><p>y</p>";
    let out = dir.path().join("out.pdf");
    let opts = AssembleOpts {
        with_toc: true,
        toc_xsl: Some(xsl_path.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let rep = assemble_pdf(&mut r, &[Source::Html(html.into())], &PageGeometry::default(), &out, &opts).unwrap();
    // The TOC page text should contain both headings (extracted via pdf text).
    let bytes = std::fs::read(&out).unwrap();
    let doc = lopdf::Document::load_mem(&bytes).unwrap();
    let text = doc.extract_text(&[1]).unwrap_or_default(); // page 1 = TOC
    assert!(text.contains("Alpha") && text.contains("Beta"), "custom-XSL TOC missing headings: {text:?}");
    let _ = rep;
}
```

Match the real `assemble_pdf` argument order (recon: `assemble_pdf(r, objects, geom, out, opts)`) and `ChromiumRenderer::spawn`. If `lopdf`'s `extract_text` isn't available, assert page count ≥ 2 (TOC + content) instead and note it.

- [ ] **Step 5: Verify:** `cargo test -p wkhtmltox-core && cargo build -p wkhtmltox-render-chromium --tests && cargo clippy -p wkhtmltox-core -p wkhtmltox-render-chromium --lib -- -D warnings`. Chrome IS available — run `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1 custom_xsl_toc_contains_headings` and report.

- [ ] **Step 6: Commit** `feat(toc): custom --xsl-style-sheet TOC via browser XSLTProcessor + settings-aware default`.

---

### Task 3 — Registry + CLI wiring + dump utilities

**Files:** Modify `settings.rs`, `registry.rs`, `crates/wkhtmltox-cli/src/lib.rs`, `crates/wkhtmltopdf-cli/src/main.rs`.
**Interfaces:** Consumes Task 1/2 (`TocXslSettings`, `outline_to_xml`, `default_toc_xsl`, `AssembleOpts.toc_xsl/toc_settings`).

- [ ] **Step 1: Settings + registry.** Add fields to the PDF settings struct that flows into `AssembleOpts`: `toc_xsl: Option<String>` + a `toc_settings: TocXslSettings` (or the six individual fields). In `registry.rs`, move these OUT of the warn arms and store them: `"tocXsl"`→`toc_xsl`; `"toc.captionText"`→`caption_text`; `"toc.useDottedLines"`→`use_dotted_lines` (bool); `"toc.forwardLinks"`→`forward_links`; `"toc.backLinks"`→`back_links`; `"toc.indentation"`→`indentation`; `"toc.fontScale"`→`font_scale` (f64). Thread them into `AssembleOpts` where it's constructed from settings.

- [ ] **Step 2: CLI flags.** Add to the `FLAGS` table in `crates/wkhtmltox-cli/src/lib.rs` (mapping to the registry setting names): `--xsl-style-sheet <file>`→`tocXsl`; `--toc-header-text <text>`→`toc.captionText`; `--disable-toc-links`→`toc.forwardLinks=false`; `--disable-dotted-lines`→`toc.useDottedLines=false`; `--toc-text-size-shrink <real>`→`toc.fontScale`; `--toc-level-indentation <width>`→`toc.indentation`; `--enable-toc-back-links`→`toc.backLinks=true`. Match the existing `FLAGS` entry shape (value-taking vs boolean) used by neighboring flags.

- [ ] **Step 3: `--dump-default-toc-xsl`** in `wkhtmltopdf-cli/src/main.rs`: a mode flag (like `--version`) that prints `wkhtmltox_core::tocxsl::default_toc_xsl(&TocXslSettings::default())` to stdout and returns 0, before any rendering. **`--dump-outline <file>`**: after the document is rendered and the outline is known, write `outline_to_xml(&entries)` to the file (reuse the outline the assembly path already extracts; if that requires a render, do it on the convert path). For v1, `--dump-outline` may write the outline of the rendered document at the end of a normal convert.

- [ ] **Step 4: Tests.**
  - CLI integration (no Chrome) in `wkhtmltopdf-cli/tests/`: `wkhtmltopdf --dump-default-toc-xsl` exits 0 and stdout contains `xsl:stylesheet` + `http://wkhtmltopdf.org/outline`.
  - Registry unit test: `set_*("toc.fontScale", "0.5")` then the built `AssembleOpts.toc_settings.font_scale == 0.5`; `set_*("tocXsl", "/x.xsl")` → `toc_xsl == Some("/x.xsl")`.
  - `--xsl-style-sheet` is now an accepted flag (no "unknown option"): a parse test.

- [ ] **Step 5: Verify:** `cargo test -p wkhtmltox-core -p wkhtmltox-cli -p wkhtmltopdf-cli && cargo clippy -p wkhtmltox-core -p wkhtmltox-cli -p wkhtmltopdf-cli --lib -- -D warnings`.

- [ ] **Step 6: Commit** `feat(cli): --xsl-style-sheet + --toc-* settings + --dump-outline/--dump-default-toc-xsl`.

---

## Milestone Gate (controller-run)
1. **Opus review** over the M9 diff: focus on (a) **XML-injection safety** — attacker-controlled heading titles are XML-escaped by `outline_to_xml` so they cannot break out of the `title="..."` attribute or inject `<item>`/markup into the outline XML (the most important security check); (b) the XSLT transform runs in the sandboxed browser, makes no network requests, and `transform_toc` fails closed (returns `Err`, never silently empties the TOC) on parse/transform error; (c) the user XSL is read from a local path with a clear error if unreadable; (d) the default TOC output didn't regress (M2b); (e) no new native dependency; `forbid(unsafe)` intact.
2. **Static audit:** `bash scripts/security-audit.sh --full` (expect HIGH=0).
3. **Fix wave** for Critical/Important.
4. **Push**; update `TODO.md` (mark custom-XSLT-TOC done; HTML headers/footers → M10) + `logbook.md`.

## Self-Review
- **Coverage:** custom XSLT TOC → Task 2 (browser transform) + Task 3 (`--xsl-style-sheet`); outline XML + default stylesheet → Task 1; `--toc-*` settings → Tasks 1–3; `--dump-outline`/`--dump-default-toc-xsl` → Task 3. HTML headers/footers are explicitly OUT (→ M10, needs a new PDF-overlay shim).
- **No new dependency:** XSLT via Chromium `XSLTProcessor` (recon confirmed `eval_json` runs arbitrary JS; XSLT makes no network calls so the raw `cdp.call` path is fine).
- **Security:** the one real surface is XML injection via heading titles → `xml_escape` (tested); the transform is sandboxed + fails closed.
- **No placeholder:** real code for the pure layer + the transform JS + the e2e; Task 3 references real registry/FLAGS shapes the implementer must match (existing symbols).
- **Type consistency:** `TocXslSettings` (six fields), `outline_to_xml(&[(String,u32,u8)])`, `default_toc_xsl(&TocXslSettings)`, `transform_toc(&mut dyn Renderer, &str, &str)`, `AssembleOpts.toc_xsl/toc_settings` used identically across tasks. `render_toc_html` gains a `&TocXslSettings` param (call sites updated).
- **Testability:** pure XML/XSL generation fully unit-tested; the browser transform is a gated e2e; the default path (no `--xsl-style-sheet`) keeps MockRenderer-based assembly tests working (the XSLT branch only triggers when `toc_xsl` is set).
