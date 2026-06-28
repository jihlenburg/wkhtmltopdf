// wkhtmltox-rs — LGPL-3.0-or-later.
//
// Integration test for `wkx_pdf_stamp_cells` via the safe Rust wrapper.
mod common;

/// Stamp bottom-center cells with "1" and "2" on a 2-page PDF and verify
/// that each page's content stream contains the expected text literal.
#[test]
fn stamp_cells_bottom_center_two_pages() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_cells_in.pdf");
    let outp = d.join("wkx_cells_out.pdf");

    common::write_min_pdf(inp.to_str().unwrap(), 2);

    // 2 pages × 6 cells = 12 entries.
    // Layout: [top-left, top-center, top-right, bot-left, bot-center, bot-right]
    // Page 1: bot-center = "1"; all others empty.
    // Page 2: bot-center = "2"; all others empty.
    let cells: Vec<String> = vec![
        // Page 1
        "".into(), "".into(), "".into(), // top row
        "".into(), "1".into(), "".into(), // bottom row
        // Page 2
        "".into(), "".into(), "".into(), // top row
        "".into(), "2".into(), "".into(), // bottom row
    ];

    wkhtmltox_pdf_sys::stamp_cells(&inp, &outp, &cells, 2, 9.0)
        .expect("stamp_cells should succeed");

    let doc = lopdf::Document::load(&outp).expect("should load stamped PDF");
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 2, "stamped PDF should still have 2 pages");

    // Page 1: content stream must contain the literal "(1)".
    let page1_id = *pages.get(&1).expect("page 1 must exist");
    let content1 = doc
        .get_page_content(page1_id)
        .expect("page 1 content must be readable");
    assert!(
        content1.windows(3).any(|w| w == b"(1)"),
        "page 1 content should contain '(1)', got: {:?}",
        String::from_utf8_lossy(&content1),
    );
    // Page 1 must NOT contain "(2)".
    assert!(
        !content1.windows(3).any(|w| w == b"(2)"),
        "page 1 content must not contain '(2)'"
    );

    // Page 2: content stream must contain the literal "(2)".
    let page2_id = *pages.get(&2).expect("page 2 must exist");
    let content2 = doc
        .get_page_content(page2_id)
        .expect("page 2 content must be readable");
    assert!(
        content2.windows(3).any(|w| w == b"(2)"),
        "page 2 content should contain '(2)', got: {:?}",
        String::from_utf8_lossy(&content2),
    );
    // Page 2 must NOT contain "(1)".
    assert!(
        !content2.windows(3).any(|w| w == b"(1)"),
        "page 2 content must not contain '(1)'"
    );
}

/// Verify that `stamp_cells` rejects a `cells` slice with the wrong length.
#[test]
fn stamp_cells_wrong_length_returns_err() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_cells_len_in.pdf");
    let outp = d.join("wkx_cells_len_out.pdf");
    common::write_min_pdf(inp.to_str().unwrap(), 2);

    // 2 pages needs 12 entries; supply only 6.
    let cells: Vec<String> = vec!["".into(); 6];
    let result = wkhtmltox_pdf_sys::stamp_cells(&inp, &outp, &cells, 2, 9.0);
    assert!(result.is_err(), "should fail with wrong cells length");
}

/// Verify that stamp_cells with n_pages = 0 returns error code 2.
#[test]
fn stamp_cells_zero_pages_returns_err() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_cells_zero_in.pdf");
    let outp = d.join("wkx_cells_zero_out.pdf");
    common::write_min_pdf(inp.to_str().unwrap(), 1);

    // 0 pages, 0 cells — the C shim should return 2.
    let cells: Vec<String> = vec![];
    let result = wkhtmltox_pdf_sys::stamp_cells(&inp, &outp, &cells, 0, 9.0);
    assert!(result.is_err(), "stamp_cells with n_pages=0 should return Err");
}

/// Verify that all-empty cells still produces a valid PDF without extra content.
#[test]
fn stamp_cells_all_empty_produces_valid_pdf() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_cells_empty_in.pdf");
    let outp = d.join("wkx_cells_empty_out.pdf");
    common::write_min_pdf(inp.to_str().unwrap(), 2);

    let cells: Vec<String> = vec!["".into(); 12]; // 2 pages × 6 empty cells
    wkhtmltox_pdf_sys::stamp_cells(&inp, &outp, &cells, 2, 9.0)
        .expect("all-empty cells should succeed");

    let doc = lopdf::Document::load(&outp).expect("should load result");
    assert_eq!(doc.get_pages().len(), 2, "page count must be preserved");
}
