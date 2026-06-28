// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;

#[test]
fn qpdf_roundtrip_preserves_pages() {
    // a minimal valid 1-page PDF
    let pdf: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \n\
trailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF\n";
    let dir = std::env::temp_dir();
    let inp = dir.join("wkx_in.pdf");
    let outp = dir.join("wkx_out.pdf");
    std::fs::write(&inp, pdf).unwrap();

    let ci = CString::new(inp.to_string_lossy().as_bytes()).unwrap();
    let co = CString::new(outp.to_string_lossy().as_bytes()).unwrap();
    let rc = unsafe { wkhtmltox_pdf_sys::wkx_pdf_roundtrip(ci.as_ptr(), co.as_ptr()) };
    assert_eq!(rc, 0, "roundtrip failed");

    let out = lopdf::Document::load(&outp).expect("load out");
    assert_eq!(out.get_pages().len(), 1);
}
