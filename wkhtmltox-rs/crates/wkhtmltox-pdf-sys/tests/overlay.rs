// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_pdf_sys::{overlay_pages, OverlaySpec};

#[test]
fn overlays_a_page_xobject_and_preserves_page_count() {
    // base: a 2-page PDF; overlay: a 1-page PDF (reuse the existing test fixtures /
    // helpers used by tests/acroform.rs or tests/merge.rs to build small PDFs).
    let base = wkhtmltox_pdf_sys::test_support_two_page_pdf();   // or the merge-test fixture
    let stamp = wkhtmltox_pdf_sys::test_support_one_page_pdf();
    // write stamp to a temp file (the shim takes paths)
    let dir = tempfile::tempdir().unwrap();
    let stamp_path = dir.path().join("stamp.pdf");
    std::fs::write(&stamp_path, &stamp).unwrap();
    let out = overlay_pages(&base, &[
        OverlaySpec { overlay_path: stamp_path.to_string_lossy().into_owned(), page_index: 0, tx: 0.0, ty: 700.0 },
        OverlaySpec { overlay_path: stamp_path.to_string_lossy().into_owned(), page_index: 1, tx: 0.0, ty: 0.0 },
    ]).expect("overlay");
    let doc = lopdf::Document::load_mem(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2, "page count preserved");
    // The overlaid page's Resources must now contain an XObject (the form).
    // (Assert at least one page has an /XObject resource entry.)
    let has_xobject = doc.get_pages().values().any(|&oid| {
        doc.get_object(oid).ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Resources").ok())
            .and_then(|r| doc.dereference(r).ok())
            .and_then(|(_, r)| r.as_dict().ok().cloned())
            .map(|d| d.has(b"XObject"))
            .unwrap_or(false)
    });
    assert!(has_xobject, "an overlaid page should have an /XObject resource");
}
