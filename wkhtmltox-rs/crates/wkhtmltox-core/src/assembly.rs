// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Result, WkError};
use crate::outline;
use crate::render::{LoadSettings, PageGeometry, ReadyPolicy, Renderer, Source};

static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Summary returned by [`assemble_pdf`].
pub struct AssemblyReport {
    pub pages: u32,
    pub objects: usize,
}

/// Render each [`Source`] in `objects`, merge them into a single PDF at `out`,
/// attach a combined `/Outlines` bookmark tree derived from the per-object heading
/// probes, and — when `number` is `true` — stamp page-number footers.
///
/// # Page-offset note (M2b TODO)
/// Each heading currently points to its **object's first page** (`offset` at the
/// time the object is processed).  Per-heading exact page numbers within each
/// object are deferred to Milestone 2b, where render-engine destinations will
/// fill in the accurate page index.
pub fn assemble_pdf(
    r: &mut dyn Renderer,
    objects: &[Source],
    geom: &PageGeometry,
    out: &Path,
    number: bool,
) -> Result<AssemblyReport> {
    let call_id = CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir().join(format!(
        "wkx-asm-{}-{}",
        std::process::id(),
        call_id
    ));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| WkError::Io(e.to_string()))?;

    let mut parts: Vec<PathBuf> = Vec::with_capacity(objects.len());
    // Outline items accumulated across all objects: (title, 0-based page in merged doc, level).
    let mut outline_items: Vec<(String, u32, u8)> = Vec::new();
    let mut offset: u32 = 0;

    for (i, src) in objects.iter().enumerate() {
        let p = r.open(src, &LoadSettings::default())?;
        r.wait_ready(p, &ReadyPolicy::default())?;
        let probe = r.eval_json(p, outline::PROBE_JS)?;
        let bytes = r.print_pdf(p, geom)?;

        let part = tmp_dir.join(format!("part{i}.pdf"));
        std::fs::write(&part, &bytes).map_err(|e| WkError::Io(e.to_string()))?;

        let n = wkhtmltox_pdf_sys::page_count(&part).map_err(WkError::Pdf)?;

        // M2b TODO: resolve per-heading page destinations from engine anchors.
        // For now every heading points to its object's first page (`offset`).
        for h in outline::parse_probe(&probe) {
            outline_items.push((h.text, offset, h.level));
        }

        offset += n;
        parts.push(part);
    }

    // Merge all part PDFs into a single document.
    let merged = tmp_dir.join("merged.pdf");
    wkhtmltox_pdf_sys::merge(&parts, &merged).map_err(WkError::Pdf)?;

    // Optionally embed the combined outline (bookmarks).
    let after_outline = if !outline_items.is_empty() {
        let outlined = tmp_dir.join("outlined.pdf");
        wkhtmltox_pdf_sys::set_outline(&merged, &outlined, &outline_items)
            .map_err(WkError::Pdf)?;
        outlined
    } else {
        merged
    };

    // Optionally stamp page-number footers.
    let final_path = if number {
        let numbered = tmp_dir.join("numbered.pdf");
        wkhtmltox_pdf_sys::stamp_footer(&after_outline, &numbered, "[page] / [topage]", 1)
            .map_err(WkError::Pdf)?;
        numbered
    } else {
        after_outline
    };

    std::fs::copy(&final_path, out).map_err(|e| WkError::Io(e.to_string()))?;

    // Best-effort cleanup — ignore errors so a failure here never masks the result.
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(AssemblyReport {
        pages: offset,
        objects: objects.len(),
    })
}
