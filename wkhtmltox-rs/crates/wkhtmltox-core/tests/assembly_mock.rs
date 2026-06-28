// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Integration test for `assemble_pdf` using MockRenderer so no browser is needed.

use wkhtmltox_core::{
    assembly::assemble_pdf,
    render::{PageGeometry, Source},
    testing::MockRenderer,
};

/// Build a minimal but structurally valid N-page PDF in memory (xref + /Size).
fn min_pdf_bytes(pages: usize) -> Vec<u8> {
    let n_objs = 2 + pages; // objects 1..=2+pages (obj 0 is free)
    let mut buf = String::new();
    buf.push_str("%PDF-1.4\n");

    let mut offsets: Vec<usize> = Vec::with_capacity(n_objs);

    // Object 1: Catalog
    offsets.push(buf.len());
    buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n");

    // Object 2: Pages node
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    offsets.push(buf.len());
    buf.push_str(&format!(
        "2 0 obj\n<</Type/Pages/Kids[{}]/Count {}>>\nendobj\n",
        kids.join(" "),
        pages
    ));

    // Objects 3..2+pages: Page
    for i in 0..pages {
        offsets.push(buf.len());
        buf.push_str(&format!(
            "{} 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n",
            3 + i
        ));
    }

    // Cross-reference table
    let xref_offset = buf.len();
    buf.push_str(&format!("xref\n0 {}\n", n_objs + 1));
    buf.push_str("0000000000 65535 f \n");
    for &off in &offsets {
        buf.push_str(&format!("{:010} 00000 n \n", off));
    }

    // Trailer
    buf.push_str(&format!(
        "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{}\n%%EOF\n",
        n_objs + 1,
        xref_offset
    ));

    buf.into_bytes()
}

/// Build a minimal valid 2-page PDF whose `/Outlines` tree contains one item
/// titled `"Deep"` with its `/Dest` pointing at the **second** page (0-based index 1).
///
/// Objects:
///  1 – Catalog  (/Pages 2 0 R  /Outlines 5 0 R)
///  2 – Pages    (/Kids [3 0 R  4 0 R]  /Count 2)
///  3 – Page 1
///  4 – Page 2   ← "Deep" bookmark destination
///  5 – Outlines root (/Count 1  /First 6 0 R  /Last 6 0 R)
///  6 – Outline item  (/Title(Deep)  /Dest[4 0 R /XYZ 0 0 0]  /Parent 5 0 R)
fn build_min_pdf_with_outline() -> Vec<u8> {
    const N: usize = 6; // object numbers 1..=6
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
    // /Dest [4 0 R /XYZ 0 0 0] — page 2 (obj 4, 0-based index 1), /XYZ destination
    buf.push_str(
        "6 0 obj\n<</Title(Deep)/Dest[4 0 R /XYZ 0 0 0]/Parent 5 0 R>>\nendobj\n",
    );

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

/// Decode a PDF string object (UTF-16BE-with-BOM or Latin-1) to a Rust String.
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

#[test]
fn assemble_two_objects_two_pages_each() {
    let out = std::env::temp_dir().join(format!(
        "wkx_asm_mock_{}.pdf",
        std::process::id()
    ));

    // MockRenderer returns a valid 2-page PDF and a 1-heading probe per call.
    // Use field assignment after new() because `next` is a private field.
    let mut mock = MockRenderer::new();
    mock.pdf = min_pdf_bytes(2);
    mock.probe = serde_json::json!({
        "headings": [{"level": 1, "text": "H", "anchor": "h", "page": 0}]
    });

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("a".into()), Source::Html("b".into())],
        &PageGeometry::default(),
        &out,
        true, // stamp page numbers
    )
    .expect("assemble_pdf should succeed");

    // 2 objects × 2 pages = 4 total pages.
    assert_eq!(report.pages, 4, "expected 4 pages total");
    assert_eq!(report.objects, 2);

    // ── lopdf structural assertions ──────────────────────────────────────────
    let doc = lopdf::Document::load(&out).expect("lopdf must load the output");

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

    // There are 2 top-level entries (one per object's heading "H").
    let count = outlines
        .get(b"Count")
        .expect("/Outlines must have /Count")
        .as_i64()
        .expect("/Count must be integer");
    assert_eq!(count, 2, "/Outlines /Count must be 2 (two top-level entries)");

    // Follow /First → first bookmark.
    let first_ref = outlines
        .get(b"First")
        .expect("/Outlines must have /First")
        .as_reference()
        .expect("/First must be a ref");
    let item_first = doc
        .get_object(first_ref)
        .expect("resolve first bookmark")
        .as_dict()
        .expect("first bookmark must be a dict");

    // Follow first's /Next → second bookmark.
    let second_ref = item_first
        .get(b"Next")
        .expect("first bookmark must have /Next")
        .as_reference()
        .expect("/Next must be a ref");
    let item_second = doc
        .get_object(second_ref)
        .expect("resolve second bookmark")
        .as_dict()
        .expect("second bookmark must be a dict");

    // Both are titled "H" (same text, different pages).
    let title_first = item_first.get(b"Title").expect("first must have /Title");
    let title_second = item_second.get(b"Title").expect("second must have /Title");
    let t1_bytes = match title_first {
        lopdf::Object::String(b, _) => b.clone(),
        _ => panic!("first /Title must be a String"),
    };
    let t2_bytes = match title_second {
        lopdf::Object::String(b, _) => b.clone(),
        _ => panic!("second /Title must be a String"),
    };
    assert_eq!(pdf_string_to_str(&t1_bytes), "H");
    assert_eq!(pdf_string_to_str(&t2_bytes), "H");

    // The two /Dest arrays must reference different page objects —
    // first points to page 0, second points to page 2 of the merged doc.
    let dest_first = item_first
        .get(b"Dest")
        .expect("first must have /Dest")
        .as_array()
        .expect("/Dest must be an array");
    let dest_second = item_second
        .get(b"Dest")
        .expect("second must have /Dest")
        .as_array()
        .expect("/Dest must be an array");

    let page_ref_first = dest_first[0]
        .as_reference()
        .expect("/Dest[0] must be a page ref");
    let page_ref_second = dest_second[0]
        .as_reference()
        .expect("/Dest[0] must be a page ref");

    assert_ne!(
        page_ref_first, page_ref_second,
        "first and second headings must point to different page objects"
    );

    // Cross-check: the merged doc has 4 pages; the page object for
    // entry 1 must match page 1 and entry 2 must match page 3 in lopdf's
    // 1-based page map (pages 0 and 2 in 0-based PDF terms).
    let pages = doc.get_pages();
    let page1_id = *pages.get(&1).expect("page 1 must exist");
    let page3_id = *pages.get(&3).expect("page 3 must exist");
    assert_eq!(
        page_ref_first, page1_id,
        "first heading must point to page 1 (0-based page 0)"
    );
    assert_eq!(
        page_ref_second, page3_id,
        "second heading must point to page 3 (0-based page 2)"
    );

    let _ = std::fs::remove_file(&out);
}

/// Verify that `assemble_pdf` produces an **exact** bookmark page when the part PDF
/// already contains an `/Outlines` tree (the Chromium `generateDocumentOutline` path).
///
/// The mock renderer returns a 2-page PDF with one outline item "Deep" whose `/Dest`
/// points at page 2 (0-based index 1).  After assembly of a single object the
/// "Deep" bookmark in the output must reference page 2, not page 1.
#[test]
fn exact_page_from_engine_outline() {
    let out = std::env::temp_dir().join(format!(
        "wkx_asm_exact_{}.pdf",
        std::process::id()
    ));

    let mut mock = MockRenderer::new();
    // 2-page PDF with /Outlines "Deep" → page 2 (0-based: 1)
    mock.pdf = build_min_pdf_with_outline();
    // Probe has NO headings: ensures the fallback path is NOT taken.
    mock.probe = serde_json::json!({ "headings": [] });

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("<h2>Deep</h2><p>content</p>".into())],
        &PageGeometry::default(),
        &out,
        false, // no footer stamp — keep pipeline minimal
    )
    .expect("assemble_pdf should succeed");

    assert_eq!(report.pages, 2, "one 2-page object → 2 total pages");
    assert_eq!(report.objects, 1);

    // ── lopdf structural assertions ──────────────────────────────────────────
    let doc = lopdf::Document::load(&out).expect("lopdf must load the output");
    let catalog = doc.catalog().expect("PDF must have a catalog");

    // /Outlines must exist.
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

    // Exactly one top-level entry.
    let count = outlines
        .get(b"Count")
        .expect("/Outlines must have /Count")
        .as_i64()
        .expect("/Count must be integer");
    assert_eq!(count, 1, "/Outlines /Count must be 1");

    // Follow /First to the "Deep" bookmark.
    let first_ref = outlines
        .get(b"First")
        .expect("/Outlines must have /First")
        .as_reference()
        .expect("/First must be a ref");
    let item = doc
        .get_object(first_ref)
        .expect("resolve first bookmark")
        .as_dict()
        .expect("first bookmark must be a dict");

    // Verify the title is "Deep".
    let title_obj = item.get(b"Title").expect("bookmark must have /Title");
    let title_bytes = match title_obj {
        lopdf::Object::String(b, _) => b.clone(),
        _ => panic!("/Title must be a String object"),
    };
    assert_eq!(pdf_string_to_str(&title_bytes), "Deep");

    // The /Dest must reference the 2nd page of the merged document (1-based: 2).
    let dest = item
        .get(b"Dest")
        .expect("bookmark must have /Dest")
        .as_array()
        .expect("/Dest must be an array");
    let dest_page_ref = dest[0]
        .as_reference()
        .expect("/Dest[0] must be a page object ref");

    let pages = doc.get_pages();
    let page2_id = *pages.get(&2).expect("page 2 must exist in merged PDF");
    assert_eq!(
        dest_page_ref, page2_id,
        "\"Deep\" bookmark must point to page 2 (exact engine outline, not object's first page)"
    );

    let _ = std::fs::remove_file(&out);
}
