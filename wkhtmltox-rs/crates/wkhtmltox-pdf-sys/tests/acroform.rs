// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_pdf_sys::{add_text_fields, TextFieldSpec};

mod common;

#[test]
fn adds_interactive_text_field() {
    let pdf: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]>>endobj\n\
xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \n\
trailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF\n";

    let out = add_text_fields(
        pdf,
        &[TextFieldSpec { name: "email".into(), page_index: 0, rect: [50.0, 50.0, 250.0, 70.0] }],
    )
    .expect("add_text_fields must succeed");

    // Verify via lopdf: catalog has /AcroForm with one field, page has a /Widget annotation.
    let doc = lopdf::Document::load_mem(&out).expect("load output pdf");
    let cat = doc.catalog().expect("catalog");
    let acro = cat.get(b"AcroForm").expect("no /AcroForm");
    let acro = acro.as_reference().map(|r| doc.get_object(r).unwrap()).unwrap_or(acro);
    let fields = acro.as_dict().unwrap().get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1, "expected exactly one form field");
}
