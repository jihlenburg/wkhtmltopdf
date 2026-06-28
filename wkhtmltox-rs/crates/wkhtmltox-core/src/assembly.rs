// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::Path;

use crate::error::{Result, WkError};
use crate::outline;
use crate::render::{LoadSettings, PageGeometry, ReadyPolicy, Renderer, Source};

/// Summary returned by [`assemble_pdf`].
pub struct AssemblyReport {
    pub pages: u32,
    pub objects: usize,
}

/// Render each [`Source`] in `objects`, merge them into a single PDF at `out`,
/// attach a combined `/Outlines` bookmark tree derived from the per-object heading
/// probes, and — when `number` is `true` — stamp page-number footers.
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
    number: bool,
) -> Result<AssemblyReport> {
    // Fix 2: reject empty input early.
    if objects.is_empty() {
        return Err(WkError::BadArg("no input objects".into()));
    }

    // Fix 1: use a randomised, mode-0700, fail-if-exists temp directory.
    // TempDir auto-removes on drop — cleanup is now unconditional (leak-on-error fixed).
    let work = tempfile::TempDir::new().map_err(|e| WkError::Io(e.to_string()))?;

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

    // Optionally stamp page-number footers.
    let final_path = if number {
        let numbered = work.path().join("numbered.pdf");
        wkhtmltox_pdf_sys::stamp_footer(&after_outline, &numbered, "[page] / [topage]", 1)
            .map_err(WkError::Pdf)?;
        numbered
    } else {
        after_outline
    };

    // Copy result to the caller-supplied destination before `work` is dropped.
    std::fs::copy(&final_path, out).map_err(|e| WkError::Io(e.to_string()))?;

    // `work` drops here → TempDir removes the directory unconditionally.
    Ok(AssemblyReport {
        pages: offset,
        objects: objects.len(),
    })
}
