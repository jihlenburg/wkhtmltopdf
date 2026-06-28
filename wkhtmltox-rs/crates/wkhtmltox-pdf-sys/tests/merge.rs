// wkhtmltox-rs — LGPL-3.0-or-later.
mod common;

#[test]
fn merges_pages_in_order() {
    let d = std::env::temp_dir();
    let a = d.join("wkx_m_a.pdf");
    let b = d.join("wkx_m_b.pdf");
    let o = d.join("wkx_m_out.pdf");
    common::write_min_pdf(a.to_str().unwrap(), 1);
    common::write_min_pdf(b.to_str().unwrap(), 2);
    let n = wkhtmltox_pdf_sys::merge(&[a.clone(), b.clone()], &o).expect("merge");
    let _ = n;
    let doc = lopdf::Document::load(&o).unwrap();
    assert_eq!(doc.get_pages().len(), 3, "1+2 pages");
}
