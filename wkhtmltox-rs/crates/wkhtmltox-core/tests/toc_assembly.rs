// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Integration tests for `assemble_pdf` with `with_toc = true`, exercised via
// MockRenderer so no browser is required.

use wkhtmltox_core::{
    assembly::{assemble_pdf, AssembleOpts},
    render::{PageGeometry, Source},
    testing::MockRenderer,
};

// ── PDF builder helpers ───────────────────────────────────────────────────────

/// Build a minimal valid 2-page PDF whose `/Outlines` tree contains one item
/// titled `"H"` with its `/Dest` pointing at the **second** page (0-based index 1).
///
/// Objects:
///  1 – Catalog  (/Pages 2 0 R  /Outlines 5 0 R)
///  2 – Pages    (/Kids [3 0 R  4 0 R]  /Count 2)
///  3 – Page 1
///  4 – Page 2   ← "H" bookmark destination
///  5 – Outlines root (/Count 1  /First 6 0 R  /Last 6 0 R)
///  6 – Outline item  (/Title(H)  /Dest[4 0 R /XYZ 0 0 0]  /Parent 5 0 R)
fn build_min_pdf_with_h_outline() -> Vec<u8> {
    const N: usize = 6;
    let mut buf = String::new();
    buf.push_str("%PDF-1.4\n");
    let mut offsets = vec![0usize; N + 1]; // index 0 unused; [1]..=[6]

    offsets[1] = buf.len();
    buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R/Outlines 5 0 R>>\nendobj\n");

    offsets[2] = buf.len();
    buf.push_str("2 0 obj\n<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>\nendobj\n");

    offsets[3] = buf.len();
    buf.push_str("3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");

    offsets[4] = buf.len();
    buf.push_str("4 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");

    offsets[5] = buf.len();
    buf.push_str("5 0 obj\n<</Type/Outlines/Count 1/First 6 0 R/Last 6 0 R>>\nendobj\n");

    offsets[6] = buf.len();
    // /Dest [4 0 R /XYZ 0 0 0] — page 2 (obj 4, 0-based index 1)
    buf.push_str("6 0 obj\n<</Title(H)/Dest[4 0 R /XYZ 0 0 0]/Parent 5 0 R>>\nendobj\n");

    let xref_offset = buf.len();
    buf.push_str(&format!("xref\n0 {}\n", N + 1));
    buf.push_str("0000000000 65535 f \n");
    for i in 1..=N {
        buf.push_str(&format!("{:010} 00000 n \n", offsets[i]));
    }
    buf.push_str(&format!(
        "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{}\n%%EOF\n",
        N + 1,
        xref_offset
    ));

    buf.into_bytes()
}

/// Decode a PDF string object (UTF-16BE with BOM or Latin-1) to a Rust String.
fn pdf_string_to_str(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let shorts: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], if c.len() > 1 { c[1] } else { 0 }]))
            .collect();
        String::from_utf16(&shorts).unwrap_or_default()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Core test: with `with_toc = true` and a MockRenderer that always returns a
/// 2-page PDF, the fixed-point loop must converge, the output must have 4 pages
/// (2 TOC + 2 content), and the "H" bookmark must point to lopdf page 4.
///
/// Fixed-point trace (mock always returns 2 pages):
///   Iter 0: toc_pages=1 → render → new=2.  Updated toc_pages=2.
///   Iter 1: toc_pages=2 → render → new=2.  Converged. ✓
///
/// Global page math (0-based):
///   cover_pages=0, toc_pages=2, prior=0, local_0=1  →  global_0 = 3
///   set_outline page 3 (0-based) = lopdf page 4 (1-based). ✓
#[test]
fn toc_with_mock_converges_and_offsets_bookmarks() {
    let out = std::env::temp_dir().join(format!("wkx_toc_test_{}.pdf", std::process::id()));

    // MockRenderer: always returns the same 2-page PDF with "H" bookmark at
    // local 0-based page 1.  Probe has no headings so extract_outline is used.
    let mut mock = MockRenderer::new();
    mock.pdf = build_min_pdf_with_h_outline();
    mock.probe = serde_json::json!({ "headings": [] });

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("<h2>H</h2><p>content</p>".into())],
        &PageGeometry::default(),
        &out,
        &AssembleOpts {
            with_toc: true,
            ..Default::default()
        },
    )
    .expect("assemble_pdf with TOC should succeed");

    // 2 TOC pages + 2 content pages = 4 total pages.
    assert_eq!(report.pages, 4, "expected 4 pages (2 toc + 2 content)");
    assert_eq!(report.objects, 1, "expected 1 content object");

    // ── lopdf structural assertions ──────────────────────────────────────────
    let doc = lopdf::Document::load(&out).expect("lopdf must load the output PDF");

    let catalog = doc.catalog().expect("PDF must have a catalog");
    let outlines_ref = catalog
        .get(b"Outlines")
        .expect("catalog must have /Outlines")
        .as_reference()
        .expect("/Outlines must be an indirect ref");
    let outlines = doc
        .get_object(outlines_ref)
        .expect("resolve /Outlines")
        .as_dict()
        .expect("/Outlines must be a dict");

    // Two top-level bookmarks: "Table of Contents" + "H".
    let count = outlines
        .get(b"Count")
        .expect("/Outlines must have /Count")
        .as_i64()
        .expect("/Count must be integer");
    assert_eq!(count, 2, "/Outlines /Count must be 2");

    // Walk: /First → "Table of Contents" → /Next → "H"
    let first_ref = outlines
        .get(b"First")
        .expect("/Outlines must have /First")
        .as_reference()
        .expect("/First must be a ref");
    let toc_item = doc
        .get_object(first_ref)
        .expect("resolve first bookmark")
        .as_dict()
        .expect("first bookmark must be a dict");

    // Verify first entry is "Table of Contents".
    let toc_title_obj = toc_item
        .get(b"Title")
        .expect("first bookmark must have /Title");
    let toc_title_bytes = match toc_title_obj {
        lopdf::Object::String(b, _) => b.clone(),
        _ => panic!("/Title must be a String"),
    };
    assert_eq!(
        pdf_string_to_str(&toc_title_bytes),
        "Table of Contents",
        "first bookmark must be 'Table of Contents'"
    );

    // Advance to second bookmark ("H").
    let h_ref = toc_item
        .get(b"Next")
        .expect("first bookmark must have /Next")
        .as_reference()
        .expect("/Next must be a ref");
    let h_item = doc
        .get_object(h_ref)
        .expect("resolve second bookmark ('H')")
        .as_dict()
        .expect("second bookmark must be a dict");

    // Verify title is "H".
    let h_title_obj = h_item.get(b"Title").expect("H bookmark must have /Title");
    let h_title_bytes = match h_title_obj {
        lopdf::Object::String(b, _) => b.clone(),
        _ => panic!("H /Title must be a String"),
    };
    assert_eq!(
        pdf_string_to_str(&h_title_bytes),
        "H",
        "second bookmark title must be 'H'"
    );

    // The /Dest of "H" must point to lopdf page 4 (= 0-based global page 3).
    let dest = h_item
        .get(b"Dest")
        .expect("H bookmark must have /Dest")
        .as_array()
        .expect("/Dest must be an array");
    let dest_page_ref = dest[0]
        .as_reference()
        .expect("/Dest[0] must be a page object ref");

    let pages = doc.get_pages();
    let page4_id = *pages.get(&4).expect("page 4 must exist in merged PDF");
    assert_eq!(
        dest_page_ref, page4_id,
        "\"H\" bookmark must point to lopdf page 4 (toc_pages=2 + local_1based=2)"
    );

    let _ = std::fs::remove_file(&out);
}

/// Smoke-test: render_toc_html produces well-formed output (unit-level assertion
/// re-checked here so `cargo test -p wkhtmltox-core` covers it in one run).
#[test]
fn render_toc_html_unit() {
    let html = wkhtmltox_core::toc::render_toc_html(&[
        ("Intro".to_string(), 1u32, 1u8),
        ("Sub".to_string(), 2u32, 2u8),
    ]);
    assert!(html.contains("Intro"));
    assert!(html.contains("Sub"));
    assert!(html.contains('1'));
    assert!(html.contains('2'));
    assert!(html.contains("Table of Contents"));
}
