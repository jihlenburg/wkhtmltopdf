// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

extern "C" {
    pub fn wkx_pdf_roundtrip(in_path: *const c_char, out_path: *const c_char) -> c_int;
    fn wkx_pdf_add_text_field(
        in_path: *const c_char,
        out_path: *const c_char,
        field_name: *const c_char,
        page_index: c_int,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> c_int;
    fn wkx_pdf_merge(in_paths: *const *const c_char, n: c_int, out: *const c_char) -> c_int;
    fn wkx_pdf_set_outline(
        in_path: *const c_char,
        out_path: *const c_char,
        titles: *const *const c_char,
        pages: *const c_int,
        levels: *const c_int,
        n: c_int,
    ) -> c_int;
    fn wkx_pdf_stamp_footer(
        in_path: *const c_char,
        out_path: *const c_char,
        fmt: *const c_char,
        start: c_int,
    ) -> c_int;
    fn wkx_pdf_page_count(in_path: *const c_char) -> c_int;
    fn wkx_pdf_add_links(
        in_path: *const c_char,
        out_path: *const c_char,
        src_pages: *const c_int,
        rects: *const f64,
        dest_pages: *const c_int,
        n: c_int,
    ) -> c_int;
    fn wkx_pdf_stamp_cells(
        in_path: *const c_char,
        out_path: *const c_char,
        cells: *const *const c_char,
        n_pages: c_int,
        font_size: f64,
    ) -> c_int;
}

/// Safe wrapper: build a nested PDF `/Outlines` (bookmarks) tree on a copy of `in_path`
/// written to `out_path`.
///
/// `items` is a slice of `(title, page, level)` tuples where `page` is 0-based and
/// `level >= 1` (1 = top-level bookmark, 2 = child, etc.).  All unsafe is confined here.
pub fn set_outline(
    in_path: &Path,
    out_path: &Path,
    items: &[(String, u32, u8)],
) -> Result<(), String> {
    let ci = CString::new(in_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let co = CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;

    let titles: Vec<CString> = items
        .iter()
        .map(|(t, _, _)| CString::new(t.as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let title_ptrs: Vec<*const c_char> = titles.iter().map(|c| c.as_ptr()).collect();
    let pages_c: Vec<c_int> = items.iter().map(|(_, p, _)| *p as c_int).collect();
    let levels_c: Vec<c_int> = items.iter().map(|(_, _, l)| *l as c_int).collect();

    let rc = unsafe {
        wkx_pdf_set_outline(
            ci.as_ptr(),
            co.as_ptr(),
            title_ptrs.as_ptr(),
            pages_c.as_ptr(),
            levels_c.as_ptr(),
            items.len() as c_int,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("wkx_pdf_set_outline rc={rc}"))
    }
}

/// Safe wrapper: stamp a centered page-number footer on every page of `in_path`,
/// writing the result to `out_path`.
///
/// `fmt` may contain `[page]` (substituted with the current page number) and `[topage]`
/// (substituted with the last page number).  `start` is the number assigned to the first page
/// (typically `1`).
pub fn stamp_footer(in_path: &Path, out_path: &Path, fmt: &str, start: u32) -> Result<(), String> {
    let ci = CString::new(in_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let co = CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let cf = CString::new(fmt.as_bytes()).map_err(|e| e.to_string())?;
    let rc = unsafe { wkx_pdf_stamp_footer(ci.as_ptr(), co.as_ptr(), cf.as_ptr(), start as c_int) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("wkx_pdf_stamp_footer rc={rc}"))
    }
}

/// Safe wrapper: return the number of pages in the PDF at `p`.
pub fn page_count(p: &Path) -> Result<u32, String> {
    let cs = CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let rc = unsafe { wkx_pdf_page_count(cs.as_ptr()) };
    if rc < 0 {
        Err(format!("wkx_pdf_page_count rc={rc}"))
    } else {
        Ok(rc as u32)
    }
}

/// Safe wrapper: stamp pre-substituted header/footer text cells on every page.
///
/// `cells` must have exactly `n_pages * 6` entries (panics otherwise).  For
/// page `p` (0-based) the six entries at `p*6 + 0..5` are:
/// `[top-left, top-center, top-right, bottom-left, bottom-center, bottom-right]`.
/// An empty string skips that cell.
///
/// `font_size`: Helvetica point size used for all non-empty cells.
pub fn stamp_cells(
    in_path: &Path,
    out_path: &Path,
    cells: &[String],
    n_pages: u32,
    font_size: f64,
) -> Result<(), String> {
    if cells.len() != n_pages as usize * 6 {
        return Err(format!(
            "stamp_cells: cells.len()={} != n_pages*6={}",
            cells.len(),
            n_pages as usize * 6
        ));
    }
    let ci = CString::new(in_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let co = CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;

    let cstrings: Vec<CString> = cells
        .iter()
        .map(|s| CString::new(s.as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();

    let rc = unsafe {
        wkx_pdf_stamp_cells(
            ci.as_ptr(),
            co.as_ptr(),
            ptrs.as_ptr(),
            n_pages as c_int,
            font_size,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("wkx_pdf_stamp_cells rc={rc}"))
    }
}

/// A single clickable link annotation: `(src_page, rect, dest_page)`.
///
/// - `src_page` — 0-based index of the page that carries the clickable area.
/// - `rect` — `[x0, y0, x1, y1]` in PDF user-space points, origin at the
///   **bottom-left** of the page (the standard PDF coordinate system).
/// - `dest_page` — 0-based index of the page the link navigates to.
pub type LinkSpec = (u32, [f64; 4], u32);

/// Safe wrapper: add clickable `/Link` annotations to a copy of `in_path`.
///
/// For each `(src_page, rect, dest_page)` in `links`, a `/Type /Annot /Subtype /Link`
/// annotation is appended to page `src_page`'s `/Annots` array.  The `/Dest` is a
/// `/XYZ null null null` GoTo destination pointing at `dest_page`, which preserves
/// the viewer's current zoom and position.
///
/// Returns `Err` if any page index is out of range, if a QPDF error occurs, or if a
/// path contains interior NUL bytes.
pub fn add_links(in_path: &Path, out_path: &Path, links: &[LinkSpec]) -> Result<(), String> {
    let ci = CString::new(in_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let co = CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;

    let src_pages_c: Vec<c_int> = links.iter().map(|(s, _, _)| *s as c_int).collect();
    let dest_pages_c: Vec<c_int> = links.iter().map(|(_, _, d)| *d as c_int).collect();
    let rects_c: Vec<f64> = links
        .iter()
        .flat_map(|(_, r, _)| r.iter().copied())
        .collect();

    let rc = unsafe {
        wkx_pdf_add_links(
            ci.as_ptr(),
            co.as_ptr(),
            src_pages_c.as_ptr(),
            rects_c.as_ptr(),
            dest_pages_c.as_ptr(),
            links.len() as c_int,
        )
    };
    match rc {
        0 => Ok(()),
        3 => Err("wkx_pdf_add_links: page index out of range".into()),
        r => Err(format!("wkx_pdf_add_links rc={r}")),
    }
}

/// One interactive text field to add: name, 0-based page, PDF-point rect
/// `[x0, y0, x1, y1]` (bottom-up origin, same coordinate system as [`add_links`]).
#[derive(Debug, Clone)]
pub struct TextFieldSpec {
    pub name: String,
    pub page_index: u32,
    pub rect: [f64; 4],
}

/// Add interactive `/Tx` AcroForm text fields to `pdf` bytes, returning new bytes.
///
/// Reuses the tested single-field shim [`wkx_pdf_add_text_field`]; each call
/// rewrites the file, so this is O(n) in field count — fine for the handful of
/// fields a page typically carries.
///
/// The shim receives `(x, y, w, h)` coordinates; width and height are derived
/// from `rect`: `w = rect[2] - rect[0]`, `h = rect[3] - rect[1]`.
pub fn add_text_fields(pdf: &[u8], fields: &[TextFieldSpec]) -> Result<Vec<u8>, String> {
    if fields.is_empty() {
        return Ok(pdf.to_vec());
    }
    let dir = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let mut cur = dir.path().join("cur.pdf");
    std::fs::write(&cur, pdf).map_err(|e| e.to_string())?;
    for (i, f) in fields.iter().enumerate() {
        let out = dir.path().join(format!("o{i}.pdf"));
        let in_c = CString::new(cur.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let out_c = CString::new(out.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let name_c = CString::new(f.name.as_str()).map_err(|e| e.to_string())?;
        let rc = unsafe {
            wkx_pdf_add_text_field(
                in_c.as_ptr(),
                out_c.as_ptr(),
                name_c.as_ptr(),
                f.page_index as c_int,
                f.rect[0],
                f.rect[1],
                f.rect[2] - f.rect[0],
                f.rect[3] - f.rect[1],
            )
        };
        if rc != 0 {
            return Err(format!("wkx_pdf_add_text_field rc={rc}"));
        }
        cur = out;
    }
    std::fs::read(&cur).map_err(|e| e.to_string())
}

/// Safe wrapper: merge `inputs` (in order) into `out`. Confines all unsafe here.
pub fn merge(inputs: &[PathBuf], out: &Path) -> Result<(), String> {
    let cs: Vec<CString> = inputs
        .iter()
        .map(|p| CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let ptrs: Vec<*const c_char> = cs.iter().map(|c| c.as_ptr()).collect();
    let co = CString::new(out.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let rc = unsafe { wkx_pdf_merge(ptrs.as_ptr(), ptrs.len() as c_int, co.as_ptr()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("wkx_pdf_merge rc={rc}"))
    }
}
