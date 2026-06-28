// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

extern "C" {
    pub fn wkx_pdf_roundtrip(in_path: *const c_char, out_path: *const c_char) -> c_int;
    pub fn wkx_pdf_add_text_field(
        in_path: *const c_char, out_path: *const c_char,
        field_name: *const c_char, page_index: c_int,
        x: f64, y: f64, w: f64, h: f64,
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
pub fn stamp_footer(
    in_path: &Path,
    out_path: &Path,
    fmt: &str,
    start: u32,
) -> Result<(), String> {
    let ci = CString::new(in_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let co = CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let cf = CString::new(fmt.as_bytes()).map_err(|e| e.to_string())?;
    let rc = unsafe {
        wkx_pdf_stamp_footer(ci.as_ptr(), co.as_ptr(), cf.as_ptr(), start as c_int)
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("wkx_pdf_stamp_footer rc={rc}"))
    }
}

/// Safe wrapper: merge `inputs` (in order) into `out`. Confines all unsafe here.
pub fn merge(inputs: &[PathBuf], out: &Path) -> Result<(), String> {
    let cs: Vec<CString> = inputs
        .iter()
        .map(|p| {
            CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string())
        })
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
