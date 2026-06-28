// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::os::raw::{c_char, c_int};

extern "C" {
    pub fn wkx_pdf_roundtrip(in_path: *const c_char, out_path: *const c_char) -> c_int;
    pub fn wkx_pdf_add_text_field(
        in_path: *const c_char, out_path: *const c_char,
        field_name: *const c_char, page_index: c_int,
        x: f64, y: f64, w: f64, h: f64,
    ) -> c_int;
}
