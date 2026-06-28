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
