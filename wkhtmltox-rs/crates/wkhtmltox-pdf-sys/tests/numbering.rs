// wkhtmltox-rs — LGPL-3.0-or-later.
mod common;

#[test]
fn page_numbers_footer() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_num_in.pdf");
    let outp = d.join("wkx_num_out.pdf");

    // Build a 3-page PDF.
    common::write_min_pdf(inp.to_str().unwrap(), 3);

    wkhtmltox_pdf_sys::stamp_footer(&inp, &outp, "[page] / [topage]", 1)
        .expect("stamp_footer");

    let doc = lopdf::Document::load(&outp).expect("load stamped PDF");
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 3, "should still have 3 pages");

    // Page 1 should have "(1 / 3)" in its content stream.
    let page1_id = *pages.get(&1).expect("page 1 must exist");
    let content1 = doc.get_page_content(page1_id).expect("page 1 content");
    assert!(
        content1.windows(7).any(|w| w == b"(1 / 3)"),
        "page 1 content should contain Tj operand '(1 / 3)', got: {:?}",
        String::from_utf8_lossy(&content1),
    );

    // Page 3 should have "(3 / 3)" in its content stream.
    let page3_id = *pages.get(&3).expect("page 3 must exist");
    let content3 = doc.get_page_content(page3_id).expect("page 3 content");
    assert!(
        content3.windows(7).any(|w| w == b"(3 / 3)"),
        "page 3 content should contain Tj operand '(3 / 3)', got: {:?}",
        String::from_utf8_lossy(&content3),
    );
}
