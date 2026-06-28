// wkhtmltox-rs — LGPL-3.0-or-later.

fn write_min_pdf(path: &str, pages: usize) {
    // minimal N-page PDF
    let mut objs = String::from("%PDF-1.4\n");
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    objs.push_str("1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n");
    objs.push_str(&format!(
        "2 0 obj<</Type/Pages/Kids[{}]/Count {}>>endobj\n",
        kids.join(" "),
        pages
    ));
    for i in 0..pages {
        objs.push_str(&format!(
            "{} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n",
            3 + i
        ));
    }
    objs.push_str("trailer<</Root 1 0 R>>\n%%EOF\n");
    std::fs::write(path, objs).unwrap();
}

#[test]
fn merges_pages_in_order() {
    let d = std::env::temp_dir();
    let a = d.join("wkx_m_a.pdf");
    let b = d.join("wkx_m_b.pdf");
    let o = d.join("wkx_m_out.pdf");
    write_min_pdf(a.to_str().unwrap(), 1);
    write_min_pdf(b.to_str().unwrap(), 2);
    let n = wkhtmltox_pdf_sys::merge(&[a.clone(), b.clone()], &o).expect("merge");
    let _ = n;
    let doc = lopdf::Document::load(&o).unwrap();
    assert_eq!(doc.get_pages().len(), 3, "1+2 pages");
}
