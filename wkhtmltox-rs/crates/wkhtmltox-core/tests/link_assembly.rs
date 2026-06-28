// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Integration test for T5: clickable internal link annotation synthesis.
//
// Uses MockRenderer: no browser required.  The test verifies that after
// `assemble_pdf` the output PDF contains a `/Link` annotation on the source page
// whose `/Dest` first element references the correct global destination page.
// Exact rect coordinates are NOT asserted (they are approximate — see assembly.rs).

use wkhtmltox_core::{
    assembly::{assemble_pdf, AssembleOpts},
    render::{PageGeometry, Source},
    testing::MockRenderer,
};

// ── PDF builder helpers ───────────────────────────────────────────────────────

/// Build a minimal valid 3-page PDF that has one named destination
/// `"x"` pointing at the third page (0-based index 2).
///
/// Object layout:
///   1 – Catalog  (/Pages 2 0 R  /Names 6 0 R)
///   2 – Pages    (/Kids [3 0 R 4 0 R 5 0 R]  /Count 3)
///   3 – Page 1   (MediaBox [0 0 200 842] — matches A4 height ≈ 842 pt so the
///               probe `top:100` lands on page 0 even with geom-based height)
///   4 – Page 2
///   5 – Page 3   ← destination for named dest "x"
///   6 – /Names dict  (/Dests 7 0 R)
///   7 – name-tree leaf  (/Names [(x) [5 0 R /XYZ 0 0 0]])
fn build_3page_with_named_dest() -> Vec<u8> {
    const N: usize = 7;
    let mut buf = String::new();
    buf.push_str("%PDF-1.4\n");
    let mut offsets = vec![0usize; N + 1];

    offsets[1] = buf.len();
    buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R/Names 6 0 R>>\nendobj\n");

    offsets[2] = buf.len();
    buf.push_str("2 0 obj\n<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3>>\nendobj\n");

    // Use A4-height MediaBox so the geom-derived page height (≈ 841.9 pt) is
    // consistent, ensuring `top:100 CSS px` resolves to page 0.
    offsets[3] = buf.len();
    buf.push_str(
        "3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>\nendobj\n",
    );

    offsets[4] = buf.len();
    buf.push_str(
        "4 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>\nendobj\n",
    );

    offsets[5] = buf.len();
    buf.push_str(
        "5 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>\nendobj\n",
    );

    offsets[6] = buf.len();
    buf.push_str("6 0 obj\n<</Dests 7 0 R>>\nendobj\n");

    offsets[7] = buf.len();
    buf.push_str("7 0 obj\n<</Names[(x) [5 0 R /XYZ 0 0 0]]>>\nendobj\n");

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

// ── Test ──────────────────────────────────────────────────────────────────────

/// Core T5 test: one internal link `{href:"#x", top:100, rect:[10,100,200,120]}`
/// in the probe → after `assemble_pdf` the output page 1 carries a `/Link`
/// annotation whose `/Dest[0]` references page 3 (the global dest for "x").
///
/// No cover, no TOC, no footer.  Single object with a 3-page PDF containing
/// named dest "x" → page 2 (0-based).  Global dest = 0 + 0 + 2 = 2 → lopdf pg 3.
#[test]
fn internal_link_produces_link_annot_on_correct_page() {
    let out = std::env::temp_dir().join(format!(
        "wkx_link_asm_{}.pdf",
        std::process::id()
    ));

    let mut mock = MockRenderer::new();
    // 3-page PDF with named dest "x" → page 2 (0-based).
    mock.pdf = build_3page_with_named_dest();
    // Probe has no headings but one internal link referencing "#x".
    mock.probe = serde_json::json!({
        "headings": [],
        "links": [
            {
                "href": "#x",
                "internal": true,
                "top": 100.0,
                "rect": [10.0, 100.0, 200.0, 120.0]
            }
        ]
    });

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("<p>content with <a href='#x'>link</a></p>".into())],
        &PageGeometry::default(),
        &out,
        &AssembleOpts::default(),
    )
    .expect("assemble_pdf should succeed");

    assert_eq!(report.pages, 3, "single 3-page object → 3 total pages");
    assert_eq!(report.objects, 1);

    // ── lopdf structural assertions ──────────────────────────────────────────
    let doc = lopdf::Document::load(&out).expect("lopdf must load output");
    let pages_map = doc.get_pages();
    assert_eq!(pages_map.len(), 3, "output must have 3 pages");

    // Source page: link.top=100 CSS px, geom.height_mm=297 → page_height_px≈1122 px,
    // so local_src_page = floor(100/1122) = 0 → global page 0 → lopdf page 1.
    let page1_id = *pages_map.get(&1).expect("lopdf page 1 must exist");

    // Destination page: named dest "x" → local 0-based 2 → global 2 → lopdf page 3.
    let page3_id = *pages_map.get(&3).expect("lopdf page 3 must exist");

    // Page 1 must have /Annots.
    let (annot_subtype, annot_dest_page_ref) = {
        let page_obj = doc.get_object(page1_id).expect("page 1 obj");
        let page_dict = page_obj.as_dict().expect("page 1 must be a dict");

        let annots_arr = page_dict
            .get(b"Annots")
            .expect("page 1 must have /Annots")
            .as_array()
            .expect("/Annots must be an array");

        assert!(!annots_arr.is_empty(), "/Annots must be non-empty");

        // Find a /Link annotation (there may be only one).
        let annot_ref = annots_arr
            .iter()
            .find_map(|o| o.as_reference().ok())
            .expect("/Annots must contain at least one indirect ref");

        let annot_obj = doc.get_object(annot_ref).expect("resolve annot");
        let annot_dict = annot_obj.as_dict().expect("annot must be a dict");

        let subtype = annot_dict
            .get(b"Subtype")
            .expect("annot must have /Subtype")
            .as_name()
            .expect("/Subtype must be a name")
            .to_owned();

        let dest = annot_dict
            .get(b"Dest")
            .expect("annot must have /Dest")
            .as_array()
            .expect("/Dest must be an array");

        assert!(!dest.is_empty(), "/Dest must be non-empty");
        let dest_page_ref = dest[0].as_reference().expect("/Dest[0] must be a page ref");

        (subtype, dest_page_ref)
    };

    // lopdf stores Name objects without the leading '/'.
    assert_eq!(annot_subtype, b"Link", "/Subtype must be /Link");
    assert_eq!(
        annot_dest_page_ref, page3_id,
        "/Dest[0] must reference page 3 (global dest for anchor 'x')"
    );

    let _ = std::fs::remove_file(&out);
}
