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
    /// An optional cover page rendered as the very first object.
    ///
    /// The cover is **excluded** from:
    /// - the outline/bookmark tree (no headings are extracted from it),
    /// - the Table of Contents (TOC entries come only from content objects),
    /// - header/footer stamping: the first `cover_pages` entries in the
    ///   per-page cells array are all-empty so the cover receives no stamp,
    /// - page numbering: `[page]` = 1 on the first non-cover page;
    ///   `[topage]` = total pages minus cover pages.
    ///
    /// `None` = no cover page.
    pub cover: Option<Source>,
    /// `LoadSettings` applied to every `r.open(…)` call: cookies, headers,
    /// auth, certificate policy, JavaScript flag.  Defaults to
    /// `LoadSettings::default()` (no cookies, JS disabled, local-file allowed
    /// per the permissive default).
    pub load: LoadSettings,
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
            cover: None,
            load: LoadSettings::default(),
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
/// When `opts.cover` is `Some(src)` the cover is rendered first and placed before
/// the TOC (if any) and all content pages.  The cover is excluded from the bookmark
/// tree, TOC, and all header/footer/numbering stamps; `[page]` starts at 1 on the
/// first non-cover page and `[topage]` equals the total non-cover page count.
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

    // Render cover page first (before the TOC/non-TOC dispatch) so that both
    // paths share the same rendered cover artefact and cover_pages value.
    let (cover_path_opt, cover_pages) =
        render_cover(r, geom, work.path(), opts)?;

    // ── TOC path ──────────────────────────────────────────────────────────────
    if opts.with_toc {
        // NOTE(T5): internal link annotation synthesis is not yet wired into the
        // TOC path.  The named-dest extraction and coordinate mapping work
        // correctly for the non-TOC path; the TOC path is deferred to T6/M2c.
        return assemble_with_toc(
            r,
            objects,
            geom,
            out,
            opts,
            work.path(),
            (cover_path_opt.as_deref(), cover_pages),
        );
        // `work` drops here after assemble_with_toc returns.
    }

    // ── Non-TOC path ──────────────────────────────────────────────────────────
    let mut outline_items: Vec<(String, u32, u8)> = Vec::new();
    // Content page offset: begins after the cover (0 if no cover).
    let mut content_offset: u32 = 0;
    let mut content_parts: Vec<PathBuf> = Vec::with_capacity(objects.len());

    // Accumulated /Link annotation specs: (src_page_global, rect_pt, dest_page_global).
    // Populated from probe `links` + named-dest extraction; see coordinate note below.
    let mut link_annots: Vec<wkhtmltox_pdf_sys::LinkSpec> = Vec::new();

    // APPROXIMATE: page height in PDF points estimated from `geom`, not the actual
    // rendered MediaBox.  Accurate when the renderer uses the given geometry (the
    // common case).  CSS px → PDF pt = ×0.75 (96 → 72 dpi).  PDF y is bottom-up.
    // Source-page determination (`top / page_height_px`) is a further approximation
    // validated loosely in T6.
    let page_height_pt = geom.height_mm * 72.0 / 25.4;
    let page_height_px = page_height_pt / 0.75; // CSS 96 dpi → PDF 72 dpi

    for (i, src) in objects.iter().enumerate() {
        let p = r.open(src, &opts.load)?;
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
                outline_items.push((title, cover_pages + content_offset + local_page, level));
            }
        } else {
            for h in outline::parse_probe(&probe) {
                outline_items.push((h.text, cover_pages + content_offset, h.level));
            }
        }

        // ── Synthesise internal /Link annotations (mitigates Chromium bug 347674894).
        //
        // Extract named destinations from the part PDF (before merging, while obj IDs
        // are still original).  Then for each internal probe link whose anchor appears
        // in the named-dest map, compute global page indices and approximate PDF-space
        // coordinates, accumulating into `link_annots`.
        let named_dests = crate::pdfread::extract_named_dests(&part);
        for link in crate::outline::parse_probe_links(&probe) {
            if !link.internal {
                continue;
            }
            let anchor = link.href.trim_start_matches('#');
            let local_dest_page = match named_dests.get(anchor) {
                Some(&p) => p,
                None => continue, // anchor not found in this object's named dests
            };
            let global_dest_page = cover_pages + content_offset + local_dest_page;

            // APPROXIMATE: derive which page the link element sits on from its
            // document-absolute CSS top coordinate.
            let local_src_page = (link.top / page_height_px).floor() as u32;
            if local_src_page >= n {
                continue; // top coordinate exceeds this object's page count
            }
            let global_src_page = cover_pages + content_offset + local_src_page;

            // APPROXIMATE: convert bounding rect from CSS px (top-down, document-abs)
            // to PDF user-space points (bottom-up, page-relative).
            let page_y_offset_px = local_src_page as f64 * page_height_px;
            let x0 = link.rect[0] * 0.75;
            let x1 = link.rect[2] * 0.75;
            let css_y_top = (link.rect[1] - page_y_offset_px) * 0.75;
            let css_y_bot = (link.rect[3] - page_y_offset_px) * 0.75;
            // PDF rect: [x_left, y_bottom, x_right, y_top] with y from page bottom.
            let pdf_y0 = (page_height_pt - css_y_bot).max(0.0);
            let pdf_y1 = (page_height_pt - css_y_top).max(0.0);

            link_annots.push((global_src_page, [x0, pdf_y0, x1, pdf_y1], global_dest_page));
        }

        content_offset += n;
        content_parts.push(part);
    }

    let total_pages = cover_pages + content_offset;

    // Merge: cover (if present) followed by all content parts.
    let mut parts: Vec<PathBuf> = Vec::with_capacity(1 + content_parts.len());
    if let Some(cp) = cover_path_opt {
        parts.push(cp);
    }
    parts.extend(content_parts);

    let merged = work.path().join("merged.pdf");
    wkhtmltox_pdf_sys::merge(&parts, &merged).map_err(WkError::Pdf)?;

    // Optionally embed the combined outline (bookmarks).
    // Note: the cover contributes no bookmark entries — its pages are skipped.
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
    // Note: this path is not cover-aware (all pages including the cover are
    // stamped). For cover-page-aware numbering, use `footer.center = "[page]/[topage]"`.
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
    // Cover pages receive all-empty cells (no stamp); non-cover pages are
    // numbered from 1 with [topage] = total_pages - cover_pages.
    let final_path = if has_cells {
        let stamped = work.path().join("cells.pdf");
        let cell_strings =
            build_cell_strings(opts, &outline_items, total_pages, cover_pages);
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

    // ── Synthesise clickable /Link annotations (mitigates Chromium bug 347674894).
    //
    // Called as the final step so it operates on the fully-assembled PDF
    // (merged + outline + optional cells).  When there are no internal links
    // the step is skipped entirely.
    //
    // Coordinate accuracy note: src_page and rect are APPROXIMATE (96→72 dpi
    // scaling, geom-derived page height, top-down → bottom-up y flip).
    // Dest-page resolution (named-dest map) is exact.  Full validation deferred
    // to T6 oracle comparison.
    let pre_link = final_path;
    let copy_src = if !link_annots.is_empty() {
        let linked = work.path().join("linked.pdf");
        wkhtmltox_pdf_sys::add_links(&pre_link, &linked, &link_annots)
            .map_err(WkError::Pdf)?;
        linked
    } else {
        pre_link
    };

    // Copy result to the caller-supplied destination before `work` is dropped.
    std::fs::copy(&copy_src, out).map_err(|e| WkError::Io(e.to_string()))?;

    // `work` drops here → TempDir removes the directory unconditionally.
    Ok(AssemblyReport {
        pages: total_pages,
        objects: objects.len(),
    })
}

/// Render the optional cover page from `opts.cover` into `work/cover.pdf`.
///
/// Returns `(Some(cover_path), cover_page_count)` when a cover is configured,
/// or `(None, 0)` when `opts.cover` is `None`.
fn render_cover(
    r: &mut dyn Renderer,
    geom: &PageGeometry,
    work: &Path,
    opts: &AssembleOpts,
) -> Result<(Option<PathBuf>, u32)> {
    if let Some(cover_src) = &opts.cover {
        let cv = r.open(cover_src, &opts.load)?;
        r.wait_ready(cv, &ReadyPolicy::default())?;
        let bytes = r.print_pdf(cv, geom)?;
        let cpath = work.join("cover.pdf");
        std::fs::write(&cpath, &bytes).map_err(|e| WkError::Io(e.to_string()))?;
        let n = wkhtmltox_pdf_sys::page_count(&cpath).map_err(WkError::Pdf)?;
        Ok((Some(cpath), n))
    } else {
        Ok((None, 0))
    }
}

/// Inner implementation of the TOC + fixed-point loop path.
///
/// Called only when `opts.with_toc = true`; separated so the `TempDir` borrow in
/// the caller stays in scope across the copy.
///
/// `cover` is a `(cover_path, cover_pages)` pair from the pre-rendered cover (if
/// any); the cover is prepended to the merged output before the TOC and content.
fn assemble_with_toc(
    r: &mut dyn Renderer,
    objects: &[Source],
    geom: &PageGeometry,
    out: &Path,
    opts: &AssembleOpts,
    work: &Path,
    cover: (Option<&Path>, u32),
) -> Result<AssemblyReport> {
    let (cover_path, cover_pages) = cover;
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
        let ph = r.open(src, &opts.load)?;
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
    // Global page layout (0-based):
    //   [cover_pages ... | toc_pages ... | content_pages ...]
    //   cover_pages is constant; toc_pages is what we are solving for.

    let mut toc_pages: u32 = 1; // initial estimate: assume 1 TOC page
    const MAX_ITERS: u32 = 3;

    let mut final_toc_path = work.join("toc0.pdf");
    let mut outline_items: Vec<(String, u32, u8)> = Vec::new();
    let mut converged = false;

    for _iter in 0..MAX_ITERS {
        // Recompute global (0-based) page offsets for every heading, assuming
        // the TOC occupies `toc_pages` pages placed after `cover_pages` pages.
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
        let tp = r.open(&Source::Html(toc_html), &opts.load)?;
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

    // ── Phase 3: assemble [cover?][toc][content…] and set outline ─────────────

    // Prepend a top-level "Table of Contents" bookmark pointing at the first
    // TOC page.  Content headings follow with their global 0-based page numbers.
    // The cover is excluded from the bookmark tree entirely.
    let mut all_outline: Vec<(String, u32, u8)> = Vec::with_capacity(1 + outline_items.len());
    all_outline.push(("Table of Contents".to_string(), cover_pages, 1u8));
    all_outline.extend_from_slice(&outline_items);

    // Merge order: cover (if any) → TOC → content objects.
    let mut parts: Vec<PathBuf> =
        Vec::with_capacity(cover_path.map_or(0, |_| 1) + 1 + content_parts.len());
    if let Some(cp) = cover_path {
        parts.push(cp.to_owned());
    }
    parts.push(final_toc_path);
    for cp in &content_parts {
        parts.push(cp.path.clone());
    }

    let content_pages: u32 = content_parts.iter().map(|cp| cp.page_count).sum();
    let total_pages = cover_pages + toc_pages + content_pages;

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
    // Cover pages receive all-empty cells; non-cover pages numbered from 1.
    let final_path = if has_cells {
        let stamped = work.join("cells.pdf");
        let cell_strings =
            build_cell_strings(opts, &all_outline, total_pages, cover_pages);
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
/// # Cover page convention
/// The first `cover_pages` pages (0-based indices `0..cover_pages`) receive
/// six empty strings each — no header or footer is stamped on the cover.
/// For non-cover pages:
/// - `[page]` = `global_page_0based - cover_pages + 1`  (starts at 1)
/// - `[topage]` = `total_pages - cover_pages`            (excludes cover)
///
/// Token substitution is performed here in Rust; the shim is a dumb stamper.
fn build_cell_strings(
    opts: &AssembleOpts,
    outline_items: &[(String, u32, u8)],
    total_pages: u32,
    cover_pages: u32,
) -> Vec<String> {
    let non_cover_pages = total_pages.saturating_sub(cover_pages);
    let (date, time) = crate::headerfooter::now_date_time();
    let mut cells: Vec<String> = Vec::with_capacity(total_pages as usize * 6);

    for page_0based in 0..total_pages {
        // Cover pages: emit all-empty cells — no header/footer stamp.
        if page_0based < cover_pages {
            for _ in 0..6 {
                cells.push(String::new());
            }
            continue;
        }

        // Non-cover pages: page numbering starts at 1; [topage] = non_cover_pages.
        let page_1based = page_0based - cover_pages + 1;
        let (section, subsection) =
            crate::headerfooter::active_section_subsection(outline_items, page_0based);

        let ctx = crate::headerfooter::PageCtx {
            page: page_1based,
            topage: non_cover_pages,
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
