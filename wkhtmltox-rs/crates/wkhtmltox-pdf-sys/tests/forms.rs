// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_pdf_sys::{add_text_fields, TextFieldSpec};

mod common;

#[test]
fn adds_two_text_fields_to_acroform() {
    // Build a minimal one-page PDF via the shared helper used by the other
    // integration tests (mirrors acroform.rs / roundtrip.rs fixture setup).
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path_str = tmp.path().to_string_lossy().to_string();
    common::write_min_pdf(&path_str, 1);
    let pdf = std::fs::read(tmp.path()).expect("read input pdf");

    let out = add_text_fields(
        &pdf,
        &[
            TextFieldSpec {
                name: "a".into(),
                page_index: 0,
                rect: [72.0, 700.0, 272.0, 724.0],
            },
            TextFieldSpec {
                name: "b".into(),
                page_index: 0,
                rect: [72.0, 660.0, 272.0, 684.0],
            },
        ],
    )
    .expect("add_text_fields must succeed");

    // Verify via lopdf: catalog /AcroForm exists and has exactly 2 /Fields.
    let doc = lopdf::Document::load_mem(&out).expect("load output pdf");
    let cat = doc.catalog().expect("catalog");
    let acro = cat.get(b"AcroForm").expect("no /AcroForm in catalog");
    // Dereference if the value is an indirect reference.
    let acro = acro
        .as_reference()
        .map(|r| doc.get_object(r).expect("deref /AcroForm"))
        .unwrap_or(acro);
    let fields = acro
        .as_dict()
        .expect("/AcroForm must be a dict")
        .get(b"Fields")
        .expect("/AcroForm must have /Fields")
        .as_array()
        .expect("/Fields must be an array");
    assert_eq!(fields.len(), 2, "expected exactly 2 form fields");
}
