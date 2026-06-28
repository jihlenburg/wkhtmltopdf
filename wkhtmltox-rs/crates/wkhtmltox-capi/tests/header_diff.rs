// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// header_diff: assert that vendored include/*.h files are byte-identical to
// the upstream headers at src/lib/*.h. If this test fails the vendored headers
// have drifted and must be re-copied verbatim from the source tree.

use std::fs;

fn upstream(name: &str) -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR == wkhtmltox-rs/crates/wkhtmltox-capi
    // upstream root is three levels up: ../../.. from the crate dir
    let manifest = env!("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("../../../src/lib")
        .join(name)
}

fn vendored(name: &str) -> std::path::PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest)
        .join("include")
        .join(name)
}

fn assert_byte_identical(name: &str) {
    let up_path = upstream(name);
    let vend_path = vendored(name);

    let up = fs::read(&up_path)
        .unwrap_or_else(|e| panic!("failed to read upstream {}: {}", up_path.display(), e));
    let vend = fs::read(&vend_path)
        .unwrap_or_else(|e| panic!("failed to read vendored {}: {}", vend_path.display(), e));

    assert_eq!(
        up.len(),
        vend.len(),
        "header {} byte length differs: upstream={} vendored={}",
        name,
        up.len(),
        vend.len()
    );

    assert_eq!(
        up, vend,
        "header {} is NOT byte-identical to upstream src/lib/{}\n\
         Run: cp src/lib/{name} wkhtmltox-rs/crates/wkhtmltox-capi/include/{name}",
        name, name
    );
}

#[test]
fn pdf_h_matches_upstream() {
    assert_byte_identical("pdf.h");
}

#[test]
fn image_h_matches_upstream() {
    assert_byte_identical("image.h");
}
