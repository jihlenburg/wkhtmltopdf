// wkhtmltox-rs — LGPL-3.0-or-later.
// Shared test helpers for wkhtmltox-pdf-sys integration tests.

/// Write a minimal but structurally valid N-page PDF to `path` (xref + /Size).
pub fn write_min_pdf(path: &str, pages: usize) {
    // Object layout:
    //   1 0 obj  /Catalog
    //   2 0 obj  /Pages
    //   3..2+pages  /Page
    let n_objs = 2 + pages; // objects 1..=2+pages (obj 0 is free)
    let mut buf = String::new();
    buf.push_str("%PDF-1.4\n");

    // Remember byte offsets for each object (1-indexed: offsets[i] = offset of (i+1) 0 obj).
    let mut offsets: Vec<usize> = Vec::with_capacity(n_objs);

    // --- Object 1: Catalog ---
    offsets.push(buf.len());
    buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n");

    // --- Object 2: Pages node ---
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    offsets.push(buf.len());
    buf.push_str(&format!(
        "2 0 obj\n<</Type/Pages/Kids[{}]/Count {}>>\nendobj\n",
        kids.join(" "),
        pages
    ));

    // --- Objects 3..2+pages: Page ---
    for i in 0..pages {
        offsets.push(buf.len());
        buf.push_str(&format!(
            "{} 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n",
            3 + i
        ));
    }

    // --- Cross-reference table ---
    let xref_offset = buf.len();
    buf.push_str(&format!("xref\n0 {}\n", n_objs + 1));
    buf.push_str("0000000000 65535 f \n");
    for &off in &offsets {
        buf.push_str(&format!("{:010} 00000 n \n", off));
    }

    // --- Trailer ---
    buf.push_str(&format!(
        "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{}\n%%EOF\n",
        n_objs + 1,
        xref_offset
    ));

    std::fs::write(path, buf).unwrap();
}
