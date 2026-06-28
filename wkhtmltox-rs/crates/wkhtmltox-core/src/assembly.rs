// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::{Path, PathBuf};

use crate::error::{Result, WkError};
use crate::outline;
use crate::render::{LoadSettings, PageGeometry, ReadyPolicy, Renderer, Source};

/// Three text cells for one row (header or footer).
///
/// Each field is a template that may contain substitution tokens such as
/// `[page]`, `[topage]`, `[section]`, `[title]`, `[date]`, `[time]`, etc.
/// An empty string means "no content in that cell".
#[derive(Debug, Clone, Default)]
pub struct CellText {
    pub left: String,
    pub center: String,
    pub right: String,
}

/// Options forwarded into [`assemble_pdf`].
///
/// All fields have sensible defaults via [`Default`]; typical usage is
/// `AssembleOpts { number: true, ..Default::default() }`.
#[derive(Debug, Clone)]
pub struct AssembleOpts {
    /// Stamp a centered `"[page] / [topage]"` footer (legacy shorthand).
    /// Ignored when `header` or `footer` is `Some(…)` — use `footer.center`
    /// instead to include a page number in the full cell layout.
    pub number: bool,
    /// Prepend a generated Table of Contents page.
    pub with_toc: bool,
    /// Three-cell header row stamped at the top of every page.
    /// `None` = no header.
    pub header: Option<CellText>,
    /// Three-cell footer row stamped at the bottom of every page.
    /// `None` = no footer.
    pub footer: Option<CellText>,
    /// Helvetica point size used for all header/footer cells (default 9.0).
    pub header_footer_font_size: f64,
    /// Document title substituted for `[title]` in header/footer templates.
    pub doc_title: String,
}

impl Default for AssembleOpts {
    fn default() -> Self {
        Self {
            number: false,
            with_toc: false,
            header: None,
            footer: None,
            header_footer_font_size: 9.0,
            doc_title: String::new(),
        }
    }
}

/// Summary returned by [`assemble_pdf`].
pub struct AssemblyReport {
    pub pages: u32,
    pub objects: usize,
}

/// Render each [`Source`] in `objects`, merge them into a single PDF at `out`,
/// attach a combined `/Outlines` bookmark tree derived from the per-object heading
/// probes, and apply header/footer cells and/or page-number footers as configured
/// in `opts`.
///
/// When `opts.with_toc` is `true` a Table of Contents page is prepended.  The TOC
/// is rendered as a leading object and its page count is stabilised via a
/// fixed-point loop (cap 3 iterations) so that the page numbers printed in the
/// TOC match the final document layout.
///
/// # Per-heading page accuracy
/// For each part PDF that contains an `/Outlines` tree (emitted by Chromium via
/// `generateDocumentOutline:true`), [`crate::pdfread::extract_outline`] is used to
/// read the **exact** local page number for every heading.  When a part has no
/// `/Outlines`, the function falls back to the JS-probe result where every heading
/// in that object is mapped to the object's first page.
pub fn assemble_pdf(
    r: &mut dyn Renderer,
    objects: &[Source],
    geom: &PageGeometry,
    out: &Path,
    opts: &AssembleOpts,
) -> Result<AssemblyReport> {
    // Fix 2: reject empty input early.
    if objects.is_empty() {
        return Err(WkError::BadArg("no input objects".into()));
    }

    // Fix 1: use a randomised, mode-0700, fail-if-exists temp directory.
    // TempDir auto-removes on drop — cleanup is now unconditional (leak-on-error fixed).
    let work = tempfile::TempDir::new().map_err(|e| WkError::Io(e.to_string()))?;

    // ── TOC path ──────────────────────────────────────────────────────────────
    if opts.with_toc {
        return assemble_with_toc(r, objects, geom, out, opts, work.path());
        // `work` drops here after assemble_with_toc returns.
    }

    // ── Non-TOC path ──────────────────────────────────────────────────────────
    let mut outline_items: Vec<(String, u32, u8)> = Vec::new();
    let mut offset: u32 = 0;
    let mut parts: Vec<std::path::PathBuf> = Vec::with_capacity(objects.len());

    for (i, src) in objects.iter().enumerate() {
        let p = r.open(src, &LoadSettings::default())?;
        r.wait_ready(p, &ReadyPolicy::default())?;
        let probe = r.eval_json(p, outline::PROBE_JS)?;
        let bytes = r.print_pdf(p, geom)?;

        let part = work.path().join(format!("part{i}.pdf"));
        std::fs::write(&part, &bytes).map_err(|e| WkError::Io(e.to_string()))?;

        let n = wkhtmltox_pdf_sys::page_count(&part).map_err(WkError::Pdf)?;

        // Prefer exact per-heading page numbers from the engine's embedded /Outlines.
        // Fall back to the JS-probe (every heading → object's first page) when the
        // part PDF has no /Outlines (e.g. engine outline was disabled or the document
        // has no headings recognised by Chromium).
        let engine_entries = crate::pdfread::extract_outline(&part);
        if !engine_entries.is_empty() {
            for (title, local_page, level) in engine_entries {
                outline_items.push((title, local_page + offset, level));
            }
        } else {
            for h in outline::parse_probe(&probe) {
                outline_items.push((h.text, offset, h.level));
            }
        }

        offset += n;
        parts.push(part);
    }

    // Merge all part PDFs into a single document.
    let merged = work.path().join("merged.pdf");
    wkhtmltox_pdf_sys::merge(&parts, &merged).map_err(WkError::Pdf)?;

    // Optionally embed the combined outline (bookmarks).
    let after_outline = if !outline_items.is_empty() {
        let outlined = work.path().join("outlined.pdf");
        wkhtmltox_pdf_sys::set_outline(&merged, &outlined, &outline_items)
            .map_err(WkError::Pdf)?;
        outlined
    } else {
        merged
    };

    // Optionally stamp simple page-number footers (legacy `number` path).
    // Skipped when header/footer cells are configured — the user should put
    // `[page]` in their template instead.
    let has_cells = opts.header.is_some() || opts.footer.is_some();
    let before_cells = if opts.number && !has_cells {
        let numbered = work.path().join("numbered.pdf");
        wkhtmltox_pdf_sys::stamp_footer(&after_outline, &numbered, "[page] / [topage]", 1)
            .map_err(WkError::Pdf)?;
        numbered
    } else {
        after_outline
    };

    // Optionally stamp variable header/footer cells.
    let final_path = if has_cells {
        let stamped = work.path().join("cells.pdf");
        let cell_strings = build_cell_strings(opts, &outline_items, offset);
        wkhtmltox_pdf_sys::stamp_cells(
            &before_cells,
            &stamped,
            &cell_strings,
            offset,
            opts.header_footer_font_size,
        )
        .map_err(WkError::Pdf)?;
        stamped
    } else {
        before_cells
    };

    // Copy result to the caller-supplied destination before `work` is dropped.
    std::fs::copy(&final_path, out).map_err(|e| WkError::Io(e.to_string()))?;

    // `work` drops here → TempDir removes the directory unconditionally.
    Ok(AssemblyReport {
        pages: offset,
        objects: objects.len(),
    })
}

/// Inner implementation of the TOC + fixed-point loop path.
///
/// Called only when `opts.with_toc = true`; separated so the `TempDir` borrow in
/// the caller stays in scope across the copy.
fn assemble_with_toc(
    r: &mut dyn Renderer,
    objects: &[Source],
    geom: &PageGeometry,
    out: &Path,
    opts: &AssembleOpts,
    work: &Path,
) -> Result<AssemblyReport> {
    // ── Phase 1: render all content objects ───────────────────────────────────
    struct ContentPart {
        path: PathBuf,
        page_count: u32,
        /// Flat outline entries extracted from this object's PDF.
        /// Each entry is `(title, local_page_0based, level)`.
        local_outline: Vec<(String, u32, u8)>,
    }

    let mut content_parts: Vec<ContentPart> = Vec::with_capacity(objects.len());

    for (i, src) in objects.iter().enumerate() {
        let ph = r.open(src, &LoadSettings::default())?;
        r.wait_ready(ph, &ReadyPolicy::default())?;
        let probe = r.eval_json(ph, outline::PROBE_JS)?;
        let bytes = r.print_pdf(ph, geom)?;

        let part = work.join(format!("part{i}.pdf"));
        std::fs::write(&part, &bytes).map_err(|e| WkError::Io(e.to_string()))?;

        let page_count = wkhtmltox_pdf_sys::page_count(&part).map_err(WkError::Pdf)?;

        // Prefer engine-embedded /Outlines (exact pages); fall back to JS probe.
        let engine = crate::pdfread::extract_outline(&part);
        let local_outline: Vec<(String, u32, u8)> = if !engine.is_empty() {
            engine
        } else {
            outline::parse_probe(&probe)
                .into_iter()
                .map(|h| (h.text, h.page, h.level))
                .collect()
        };

        content_parts.push(ContentPart { path: part, page_count, local_outline });
    }

    // ── Phase 2: fixed-point TOC page-count stabilisation ─────────────────────
    //
    // Inserting a TOC shifts content page numbers, which changes the TOC
    // content, which may change the TOC page count.  We iterate until the TOC
    // page count stabilises (or cap at MAX_ITERS and log a warning).
    //
    // cover_pages = 0 here; Task 4 will wire the cover offset.

    let cover_pages: u32 = 0;
    let mut toc_pages: u32 = 1; // initial estimate: assume 1 TOC page
    const MAX_ITERS: u32 = 3;

    let mut final_toc_path = work.join("toc0.pdf");
    let mut outline_items: Vec<(String, u32, u8)> = Vec::new();
    let mut converged = false;

    for _iter in 0..MAX_ITERS {
        // Recompute global (0-based) page offsets for every heading, assuming
        // the TOC occupies `toc_pages` pages.
        outline_items.clear();
        let mut prior: u32 = 0;
        for cp in &content_parts {
            for (title, local_0, level) in &cp.local_outline {
                let global_0 = cover_pages + toc_pages + prior + local_0;
                outline_items.push((title.clone(), global_0, *level));
            }
            prior += cp.page_count;
        }

        // Build TOC HTML with 1-based display page numbers.
        let toc_display: Vec<(String, u32, u8)> = outline_items
            .iter()
            .map(|(t, g, l)| (t.clone(), g + 1, *l))
            .collect();
        let toc_html = crate::toc::render_toc_html(&toc_display);

        // TODO(M3/M4): ChromiumRenderer must accept Source::Html for TOC.
        // Until then, TOC rendering is exercised via MockRenderer in tests; the
        // real-Chrome path (T6) writes the HTML to a temp file and uses file://.
        let tp = r.open(&Source::Html(toc_html), &LoadSettings::default())?;
        r.wait_ready(tp, &ReadyPolicy::default())?;
        let toc_bytes = r.print_pdf(tp, geom)?;
        let tpath = work.join(format!("toc{_iter}.pdf"));
        std::fs::write(&tpath, &toc_bytes).map_err(|e| WkError::Io(e.to_string()))?;
        let new_pages = wkhtmltox_pdf_sys::page_count(&tpath).map_err(WkError::Pdf)?;

        final_toc_path = tpath;

        if new_pages == toc_pages {
            converged = true;
            break; // page count stable — outline_items are consistent with TOC
        }
        toc_pages = new_pages;
    }

    if !converged {
        eprintln!(
            "wkhtmltox: TOC page count did not converge in {} iterations; \
             using last result (page numbers may be off by ≤1 page)",
            MAX_ITERS
        );
    }

    // ── Phase 3: assemble [toc, content…] and set outline ─────────────────────

    // Prepend a top-level "Table of Contents" bookmark pointing at the TOC itself.
    let mut all_outline: Vec<(String, u32, u8)> = Vec::with_capacity(1 + outline_items.len());
    all_outline.push(("Table of Contents".to_string(), cover_pages, 1u8));
    all_outline.extend_from_slice(&outline_items);

    let mut parts: Vec<PathBuf> = Vec::with_capacity(1 + content_parts.len());
    parts.push(final_toc_path);
    for cp in &content_parts {
        parts.push(cp.path.clone());
    }

    let content_pages: u32 = content_parts.iter().map(|cp| cp.page_count).sum();
    let total_pages = toc_pages + content_pages;

    let merged = work.join("merged.pdf");
    wkhtmltox_pdf_sys::merge(&parts, &merged).map_err(WkError::Pdf)?;

    let after_outline = if !all_outline.is_empty() {
        let outlined = work.join("outlined.pdf");
        wkhtmltox_pdf_sys::set_outline(&merged, &outlined, &all_outline)
            .map_err(WkError::Pdf)?;
        outlined
    } else {
        merged
    };

    // Optionally stamp simple page-number footers (legacy path).
    let has_cells = opts.header.is_some() || opts.footer.is_some();
    let before_cells = if opts.number && !has_cells {
        let numbered = work.join("numbered.pdf");
        wkhtmltox_pdf_sys::stamp_footer(&after_outline, &numbered, "[page] / [topage]", 1)
            .map_err(WkError::Pdf)?;
        numbered
    } else {
        after_outline
    };

    // Optionally stamp variable header/footer cells.
    let final_path = if has_cells {
        let stamped = work.join("cells.pdf");
        let cell_strings = build_cell_strings(opts, &all_outline, total_pages);
        wkhtmltox_pdf_sys::stamp_cells(
            &before_cells,
            &stamped,
            &cell_strings,
            total_pages,
            opts.header_footer_font_size,
        )
        .map_err(WkError::Pdf)?;
        stamped
    } else {
        before_cells
    };

    std::fs::copy(&final_path, out).map_err(|e| WkError::Io(e.to_string()))?;

    Ok(AssemblyReport {
        pages: total_pages,
        objects: objects.len(),
    })
}

/// Build the flat `cells` array consumed by [`wkhtmltox_pdf_sys::stamp_cells`].
///
/// Returns a `Vec<String>` of length `total_pages * 6`.  For each 0-based page
/// `p` the six entries at `p*6+0..5` are:
/// `[top-left, top-center, top-right, bottom-left, bottom-center, bottom-right]`.
///
/// Token substitution is performed here in Rust; the shim is a dumb stamper.
fn build_cell_strings(
    opts: &AssembleOpts,
    outline_items: &[(String, u32, u8)],
    total_pages: u32,
) -> Vec<String> {
    let (date, time) = crate::headerfooter::now_date_time();
    let mut cells: Vec<String> = Vec::with_capacity(total_pages as usize * 6);

    for page_0based in 0..total_pages {
        let page_1based = page_0based + 1;
        let (section, subsection) =
            crate::headerfooter::active_section_subsection(outline_items, page_0based);

        let ctx = crate::headerfooter::PageCtx {
            page: page_1based,
            topage: total_pages,
            frompage: 1,
            section,
            subsection,
            title: opts.doc_title.clone(),
            date: date.clone(),
            time: time.clone(),
        };

        // Header row: top-left, top-center, top-right
        let (hl, hc, hr) = if let Some(h) = &opts.header {
            (
                crate::headerfooter::substitute(&h.left, &ctx),
                crate::headerfooter::substitute(&h.center, &ctx),
                crate::headerfooter::substitute(&h.right, &ctx),
            )
        } else {
            (String::new(), String::new(), String::new())
        };

        // Footer row: bottom-left, bottom-center, bottom-right
        let (fl, fc, fr) = if let Some(f) = &opts.footer {
            (
                crate::headerfooter::substitute(&f.left, &ctx),
                crate::headerfooter::substitute(&f.center, &ctx),
                crate::headerfooter::substitute(&f.right, &ctx),
            )
        } else {
            (String::new(), String::new(), String::new())
        };

        cells.push(hl);
        cells.push(hc);
        cells.push(hr);
        cells.push(fl);
        cells.push(fc);
        cells.push(fr);
    }

    cells
}
