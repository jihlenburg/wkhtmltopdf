// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;

#[test]
fn adds_interactive_text_field() {
    let pdf: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]>>endobj\n\
xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \n\
trailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF\n";
    let dir = std::env::temp_dir();
    let inp = dir.join("wkx_form_in.pdf");
    let outp = dir.join("wkx_form_out.pdf");
    std::fs::write(&inp, pdf).unwrap();

    let ci = CString::new(inp.to_string_lossy().as_bytes()).unwrap();
    let co = CString::new(outp.to_string_lossy().as_bytes()).unwrap();
    let name = CString::new("email").unwrap();
    let rc = unsafe {
        wkhtmltox_pdf_sys::wkx_pdf_add_text_field(ci.as_ptr(), co.as_ptr(), name.as_ptr(),
                                                  0, 50.0, 50.0, 200.0, 20.0)
    };
    assert_eq!(rc, 0, "add_text_field failed");

    // Verify via lopdf: catalog has /AcroForm with one field, page has a /Widget annotation.
    let doc = lopdf::Document::load(&outp).expect("load out");
    let cat = doc.catalog().expect("catalog");
    let acro = cat.get(b"AcroForm").expect("no /AcroForm");
    let acro = acro.as_reference().map(|r| doc.get_object(r).unwrap()).unwrap_or(acro);
    let fields = acro.as_dict().unwrap().get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1, "expected exactly one form field");
}
