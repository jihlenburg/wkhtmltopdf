// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Integration tests for `assemble_pdf` with `cover = Some(…)`, exercised via
// MockRenderer so no browser is required.

use wkhtmltox_core::{
    assembly::{assemble_pdf, AssembleOpts, CellText},
    render::{PageGeometry, Source},
};

// ── PDF builder helpers ───────────────────────────────────────────────────────

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

/// A MockRenderer that serves different PDFs for the first call (cover) vs
/// subsequent calls (content objects).
///
/// `cover_pdf`   — bytes returned on call #1 (the cover render)
/// `content_pdf` — bytes returned on calls #2+ (content objects)
struct CoverMockRenderer {
    cover_pdf: Vec<u8>,
    content_pdf: Vec<u8>,
    call_count: u64,
    next: u64,
}

impl CoverMockRenderer {
    fn new(cover_pdf: Vec<u8>, content_pdf: Vec<u8>) -> Self {
        Self { cover_pdf, content_pdf, call_count: 0, next: 0 }
    }
}

impl wkhtmltox_core::render::Renderer for CoverMockRenderer {
    fn open(
        &mut self,
        _s: &Source,
        _l: &wkhtmltox_core::render::LoadSettings,
    ) -> wkhtmltox_core::error::Result<wkhtmltox_core::render::PageHandle> {
        self.next += 1;
        self.call_count += 1;
        Ok(wkhtmltox_core::render::PageHandle(self.next))
    }
    fn wait_ready(
        &mut self,
        _p: wkhtmltox_core::render::PageHandle,
        _r: &wkhtmltox_core::render::ReadyPolicy,
    ) -> wkhtmltox_core::error::Result<()> {
        Ok(())
    }
    fn eval_json(
        &mut self,
        _p: wkhtmltox_core::render::PageHandle,
        _s: &str,
    ) -> wkhtmltox_core::error::Result<serde_json::Value> {
        Ok(serde_json::json!({ "headings": [] }))
    }
    fn print_pdf(
        &mut self,
        _p: wkhtmltox_core::render::PageHandle,
        _g: &wkhtmltox_core::render::PageGeometry,
    ) -> wkhtmltox_core::error::Result<Vec<u8>> {
        // Call #1 = cover render, calls #2+ = content (and TOC in TOC tests).
        if self.call_count == 1 {
            Ok(self.cover_pdf.clone())
        } else {
            Ok(self.content_pdf.clone())
        }
    }
    fn snapshot(
        &mut self,
        _p: wkhtmltox_core::render::PageHandle,
        o: &wkhtmltox_core::render::SnapshotOpts,
    ) -> wkhtmltox_core::error::Result<wkhtmltox_core::render::RawImage> {
        Ok(wkhtmltox_core::render::RawImage { bytes: vec![0u8; 8], format: o.format })
    }
    fn page_info(
        &self,
        _p: wkhtmltox_core::render::PageHandle,
    ) -> wkhtmltox_core::error::Result<wkhtmltox_core::render::PageInfo> {
        Ok(wkhtmltox_core::render::PageInfo {
            title: "mock".into(),
            final_url: "about:blank".into(),
            content_height_px: 1000.0,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Cover (1 page) + 1 content object (2 pages), no TOC.
///
/// Expected:
/// - total pages = 1 (cover) + 2 (content) = 3
/// - cover page (page 1) has NO footer text (all-empty cells)
/// - first content page (page 2) shows "(1/2)" in footer center
/// - last content page (page 3) shows "(2/2)" in footer center
#[test]
fn cover_excluded_from_footer_and_numbering() {
    let out = std::env::temp_dir().join(format!(
        "wkx_cover_footer_{}.pdf",
        std::process::id()
    ));

    // cover = 1 page, content = 2 pages
    let mut mock = CoverMockRenderer::new(min_pdf_bytes(1), min_pdf_bytes(2));

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("<p>content</p>".into())],
        &PageGeometry::default(),
        &out,
        &AssembleOpts {
            cover: Some(Source::Html("<h1>Cover</h1>".into())),
            footer: Some(CellText {
                left: String::new(),
                center: "[page]/[topage]".into(),
                right: String::new(),
            }),
            ..Default::default()
        },
    )
    .expect("assemble_pdf with cover should succeed");

    // Total pages = cover + content.
    assert_eq!(report.pages, 3, "expected 1 cover + 2 content = 3 pages");
    assert_eq!(report.objects, 1, "one content object");

    let doc = lopdf::Document::load(&out).expect("lopdf must load the output");
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 3, "merged PDF must have 3 pages");

    // Page 1 (cover): must NOT contain any page-number text.
    let page1_id = *pages.get(&1).expect("page 1 must exist");
    let content1 = doc
        .get_page_content(page1_id)
        .expect("page 1 content must be readable");
    let content1_str = String::from_utf8_lossy(&content1);
    assert!(
        !content1_str.contains("(1/"),
        "cover page must NOT contain page-number footer, got: {content1_str:?}"
    );

    // Page 2 (first content page): footer should show "(1/2)".
    let page2_id = *pages.get(&2).expect("page 2 must exist");
    let content2 = doc
        .get_page_content(page2_id)
        .expect("page 2 content must be readable");
    assert!(
        content2.windows(5).any(|w| w == b"(1/2)"),
        "first content page must contain '(1/2)', got: {:?}",
        String::from_utf8_lossy(&content2),
    );

    // Page 3 (second content page): footer should show "(2/2)".
    let page3_id = *pages.get(&3).expect("page 3 must exist");
    let content3 = doc
        .get_page_content(page3_id)
        .expect("page 3 content must be readable");
    assert!(
        content3.windows(5).any(|w| w == b"(2/2)"),
        "second content page must contain '(2/2)', got: {:?}",
        String::from_utf8_lossy(&content3),
    );

    let _ = std::fs::remove_file(&out);
}

/// Cover (1 page) + TOC + 1 content object (2 pages with one heading at page 2).
///
/// Fixed-point trace (mock always returns 2 pages for non-cover renders):
///   Iter 0: toc_pages=1 → render → new=2. Updated toc_pages=2.
///   Iter 1: toc_pages=2 → render → new=2. Converged. ✓
///
/// Expected:
/// - total pages = 1 (cover) + 2 (TOC) + 2 (content) = 5
/// - "H" content bookmark must point to global 0-based page:
///   cover_pages(1) + toc_pages(2) + local_0based(1) = 4 → lopdf page 5
/// - TOC bookmark ("Table of Contents") must point to lopdf page 2
///   (= 0-based global page 1 = cover_pages = 1)
/// - The cover page has no footer; content pages are numbered 1..3 with topage=4.
#[test]
fn cover_with_toc_offsets_bookmarks_correctly() {
    let out = std::env::temp_dir().join(format!(
        "wkx_cover_toc_{}.pdf",
        std::process::id()
    ));

    // Build a 2-page content PDF with an /Outlines "H" at local 0-based page 1.
    let content_pdf = build_min_pdf_with_h_outline();

    // cover = 1 page, all other renders (TOC iterations + content) = 2 pages.
    let mut mock = CoverMockRenderer::new(min_pdf_bytes(1), content_pdf);

    let report = assemble_pdf(
        &mut mock,
        &[Source::Html("<h2>H</h2><p>content</p>".into())],
        &PageGeometry::default(),
        &out,
        &AssembleOpts {
            cover: Some(Source::Html("<h1>Cover</h1>".into())),
            with_toc: true,
            ..Default::default()
        },
    )
    .expect("assemble_pdf with cover + TOC should succeed");

    // cover(1) + toc(2) + content(2) = 5
    assert_eq!(report.pages, 5, "expected 1+2+2=5 pages");
    assert_eq!(report.objects, 1, "one content object");

    let doc = lopdf::Document::load(&out).expect("lopdf must load the output");
    let pages_map = doc.get_pages();
    assert_eq!(pages_map.len(), 5, "merged PDF must have 5 pages");

    // ── Bookmark tree checks ─────────────────────────────────────────────────
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

    // Two top-level bookmarks: "Table of Contents" and "H".
    let count = outlines
        .get(b"Count")
        .expect("/Outlines must have /Count")
        .as_i64()
        .expect("/Count must be integer");
    assert_eq!(count, 2, "/Outlines /Count must be 2");

    // First bookmark = "Table of Contents" pointing at lopdf page 2
    // (0-based global 1 = cover_pages = 1; lopdf uses 1-based so = 2).
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

    let toc_dest = toc_item
        .get(b"Dest")
        .expect("TOC bookmark must have /Dest")
        .as_array()
        .expect("/Dest must be array");
    let toc_page_ref = toc_dest[0].as_reference().expect("/Dest[0] must be page ref");
    let page2_id = *pages_map.get(&2).expect("lopdf page 2 must exist");
    assert_eq!(
        toc_page_ref, page2_id,
        "TOC bookmark must point to lopdf page 2 (= 0-based global page cover_pages=1)"
    );

    // Second bookmark = "H" pointing at lopdf page 5
    // (0-based global: cover(1) + toc(2) + local(1) = 4 → lopdf page 5).
    let h_ref = toc_item
        .get(b"Next")
        .expect("TOC bookmark must have /Next")
        .as_reference()
        .expect("/Next must be a ref");
    let h_item = doc
        .get_object(h_ref)
        .expect("resolve H bookmark")
        .as_dict()
        .expect("H bookmark must be a dict");

    let h_dest = h_item
        .get(b"Dest")
        .expect("H bookmark must have /Dest")
        .as_array()
        .expect("/Dest must be array");
    let h_page_ref = h_dest[0].as_reference().expect("/Dest[0] must be page ref");
    let page5_id = *pages_map.get(&5).expect("lopdf page 5 must exist");
    assert_eq!(
        h_page_ref, page5_id,
        "H bookmark must point to lopdf page 5 (cover=1 + toc=2 + local_0=1 → 0-based 4)"
    );

    let _ = std::fs::remove_file(&out);
}

/// Build a minimal valid 2-page PDF whose `/Outlines` contains one item "H"
/// with its `/Dest` pointing at the second page (0-based index 1).
fn build_min_pdf_with_h_outline() -> Vec<u8> {
    const N: usize = 6;
    let mut buf = String::new();
    buf.push_str("%PDF-1.4\n");
    let mut offsets = vec![0usize; N + 1];

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
