// wkhtmltox-rs — LGPL-3.0-or-later.
mod common;

#[test]
fn nested_outline_bookmarks() {
    let d = std::env::temp_dir();
    let inp = d.join("wkx_ol_in.pdf");
    let outp = d.join("wkx_ol_out.pdf");

    // Build a 3-page PDF.
    common::write_min_pdf(inp.to_str().unwrap(), 3);

    // Set nested outline: A (level 1, page 0), A.1 (level 2, page 0), B (level 1, page 2).
    let items: Vec<(String, u32, u8)> = vec![
        ("A".into(), 0, 1),
        ("A.1".into(), 0, 2),
        ("B".into(), 2, 1),
    ];
    wkhtmltox_pdf_sys::set_outline(&inp, &outp, &items).expect("set_outline");

    // Verify the output via lopdf.
    let doc = lopdf::Document::load(&outp).expect("load out");
    let catalog = doc.catalog().expect("catalog");

    // /Outlines must exist on the catalog.
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

    // /Count should be total number of outline items = 3.
    let count = outlines
        .get(b"Count")
        .expect("/Outlines must have /Count")
        .as_i64()
        .expect("/Count must be integer");
    assert_eq!(count, 3, "/Outlines /Count should be 3 (all descendants)");

    // Follow /First to get item A.
    let first_ref = outlines
        .get(b"First")
        .expect("/Outlines must have /First")
        .as_reference()
        .expect("/First must be a ref");
    let item_a = doc
        .get_object(first_ref)
        .expect("resolve /First (A)")
        .as_dict()
        .expect("item A must be a dict");

    // Check title of A.
    let title_a = item_a.get(b"Title").expect("A must have /Title");
    let title_a_bytes = match title_a {
        lopdf::Object::String(bytes, _) => bytes.clone(),
        _ => panic!("A /Title must be a string object"),
    };
    // PDF Unicode strings start with BOM (0xFE 0xFF); strip it and decode.
    let title_a_str = pdf_string_to_str(&title_a_bytes);
    assert_eq!(title_a_str, "A", "first top-level item should be 'A'");

    // A must have a child: /First should point to A.1.
    let a_first_ref = item_a
        .get(b"First")
        .expect("A must have /First (A.1)")
        .as_reference()
        .expect("A /First must be a ref");
    let item_a1 = doc
        .get_object(a_first_ref)
        .expect("resolve A.1")
        .as_dict()
        .expect("A.1 must be a dict");
    let title_a1 = item_a1.get(b"Title").expect("A.1 must have /Title");
    let title_a1_bytes = match title_a1 {
        lopdf::Object::String(bytes, _) => bytes.clone(),
        _ => panic!("A.1 /Title must be a string object"),
    };
    assert_eq!(pdf_string_to_str(&title_a1_bytes), "A.1", "child of A should be 'A.1'");

    // Follow A's /Next to get item B.
    let a_next_ref = item_a
        .get(b"Next")
        .expect("A must have /Next (B)")
        .as_reference()
        .expect("A /Next must be a ref");
    let item_b = doc
        .get_object(a_next_ref)
        .expect("resolve B")
        .as_dict()
        .expect("B must be a dict");
    let title_b = item_b.get(b"Title").expect("B must have /Title");
    let title_b_bytes = match title_b {
        lopdf::Object::String(bytes, _) => bytes.clone(),
        _ => panic!("B /Title must be a string object"),
    };
    assert_eq!(pdf_string_to_str(&title_b_bytes), "B", "second top-level item should be 'B'");

    // B's /Dest first element must resolve to the 3rd page (index 2).
    let b_dest = item_b
        .get(b"Dest")
        .expect("B must have /Dest")
        .as_array()
        .expect("/Dest must be an array");
    assert!(!b_dest.is_empty(), "/Dest must be non-empty");
    let page3_ref = b_dest[0].as_reference().expect("/Dest[0] must be a page ref");

    // The 3rd page (0-based index 2) — get the page ids in order.
    let pages = doc.get_pages();
    // get_pages returns a BTreeMap<u32, ObjectId>, page numbers 1-based.
    let page3_id = pages.get(&3).expect("page 3 must exist");
    assert_eq!(
        page3_ref, *page3_id,
        "B /Dest[0] must reference the 3rd page object"
    );
}

/// Decode a PDF string (may be UTF-16BE with BOM, or Latin-1) to a Rust String.
fn pdf_string_to_str(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE with BOM
        let shorts: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], if c.len() > 1 { c[1] } else { 0 }]))
            .collect();
        String::from_utf16(&shorts).unwrap_or_default()
    } else {
        // Latin-1 / PDFDocEncoding — treat as UTF-8 lossy
        String::from_utf8_lossy(bytes).into_owned()
    }
}
