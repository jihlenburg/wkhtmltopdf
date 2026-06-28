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

#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored"]
fn networking_settings_accepted_headers_and_auth() {
    // Uses a data: URL so no network is needed; exercises the CDP networking
    // calls (Network.enable, Network.setExtraHTTPHeaders with Basic auth header)
    // and verifies the page still renders to a valid PDF.
    // Note: cookies require http/https origins, so they are tested separately
    // with real HTTP targets; this test focuses on custom-headers + auth + JS.
    let mut r = ChromiumRenderer::spawn().expect("spawn chrome");
    let load = LoadSettings {
        custom_headers: vec![("X-Test-Header".to_string(), "wkhtmltox".to_string())],
        username: Some("testuser".to_string()),
        password: Some("testpass".to_string()),
        enable_javascript: true,
        allow_local_file_access: true,
        ..Default::default()
    };
    // data: URL — no network required; the CDP networking calls are the point.
    let p = r.open(
        &Source::Url("data:text/html,<html><body><h1>Networking Test</h1></body></html>".into()),
        &load,
    ).expect("open must succeed with networking settings applied");
    r.wait_ready(p, &ReadyPolicy::default()).expect("wait_ready must succeed");
    let pdf = r.print_pdf(p, &PageGeometry::default()).expect("print_pdf must succeed");
    assert!(pdf.starts_with(b"%PDF"), "output must be a valid PDF");
}
