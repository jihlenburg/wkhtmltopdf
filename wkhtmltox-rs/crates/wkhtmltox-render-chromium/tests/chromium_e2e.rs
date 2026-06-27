// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_core::render::*;
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored"]
fn renders_paginated_pdf_with_outline() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/toc.html");
    let url = format!("file://{path}");
    let mut r = ChromiumRenderer::spawn().expect("spawn chrome");
    let p = r.open(&Source::Url(url), &LoadSettings { enable_javascript: true, ..Default::default() }).unwrap();
    r.wait_ready(p, &ReadyPolicy::default()).unwrap();
    let pdf = r.print_pdf(p, &PageGeometry::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF"), "not a pdf");

    let doc = lopdf::Document::load_mem(&pdf).expect("parse pdf");
    let pages = doc.get_pages().len();
    assert!(pages >= 3, "expected >=3 pages from forced breaks, got {pages}");
    // generateDocumentOutline must yield a catalog /Outlines entry
    let catalog = doc.catalog().expect("catalog");
    assert!(catalog.get(b"Outlines").is_ok(), "no /Outlines (generateDocumentOutline failed)");
}
