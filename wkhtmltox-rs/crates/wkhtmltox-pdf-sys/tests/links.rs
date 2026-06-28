// wkhtmltox-rs — LGPL-3.0-or-later.
//
// Integration tests for `wkx_pdf_add_links` / `wkhtmltox_pdf_sys::add_links`.
mod common;

use wkhtmltox_pdf_sys::LinkSpec;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal but valid 3-page PDF and write it to `path`.
fn write_3page(path: &str) {
    common::write_min_pdf(path, 3);
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Add one /Link annotation from page 0 (src) to page 2 (dest) and verify
/// via lopdf:
///   1. Page 1 (0-based 0) has an /Annots array.
///   2. That array contains exactly one annotation.
///   3. The annotation has /Subtype /Link.
///   4. Its /Dest first element references the 3rd page object.
#[test]
fn add_link_annot_page0_to_page2() {
    let d = std::env::temp_dir();
    let inp = d.join(format!("wkx_lnk_in_{}.pdf", std::process::id()));
    let out = d.join(format!("wkx_lnk_out_{}.pdf", std::process::id()));

    write_3page(inp.to_str().unwrap());

    let links: &[LinkSpec] = &[(0, [10.0, 10.0, 100.0, 30.0], 2)];
    wkhtmltox_pdf_sys::add_links(&inp, &out, links).expect("add_links should succeed");

    // ── lopdf assertions ──────────────────────────────────────────────────────
    let doc = lopdf::Document::load(&out).expect("lopdf must load output");
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 3, "output must still have 3 pages");

    // Page 1 (lopdf 1-based) = 0-based page 0 = the source page.
    let page1_id = *pages.get(&1).expect("page 1 must exist");

    let (annots_ref, subtype_name, dest_page_ref) = {
        let page_obj = doc.get_object(page1_id).expect("page 1 obj");
        let page_dict = page_obj.as_dict().expect("page 1 dict");

        // /Annots must exist.
        let annots_val = page_dict.get(b"Annots").expect("page 1 must have /Annots");
        let annots_arr = annots_val.as_array().expect("/Annots must be an array");
        assert_eq!(annots_arr.len(), 1, "/Annots must have exactly one entry");

        // The annotation may be a direct dict or an indirect reference.
        let annot_ref = annots_arr[0].as_reference().expect("/Annots[0] must be a ref");

        // Resolve annotation dict; extract /Subtype and /Dest[0] as owned.
        let annot_obj = doc.get_object(annot_ref).expect("resolve annot");
        let annot_dict = annot_obj.as_dict().expect("annot must be a dict");

        let subtype = annot_dict
            .get(b"Subtype")
            .expect("annot must have /Subtype")
            .as_name()
            .expect("/Subtype must be a name")
            .to_owned();

        let dest_arr = annot_dict
            .get(b"Dest")
            .expect("annot must have /Dest")
            .as_array()
            .expect("/Dest must be an array");
        assert!(!dest_arr.is_empty(), "/Dest must be non-empty");
        let dest_page = dest_arr[0].as_reference().expect("/Dest[0] must be a page ref");

        (annot_ref, subtype, dest_page)
    };

    let _ = annots_ref; // used above; suppress unused-var warning in some toolchains

    // lopdf stores Name objects without the leading '/'.
    assert_eq!(subtype_name, b"Link", "/Subtype must be /Link");

    // Page 3 (lopdf 1-based) = 0-based page 2 = the destination page.
    let page3_id = *pages.get(&3).expect("page 3 must exist");
    assert_eq!(
        dest_page_ref, page3_id,
        "/Dest[0] must reference the 3rd page object"
    );

    let _ = std::fs::remove_file(&inp);
    let _ = std::fs::remove_file(&out);
}

/// Adding a link with a dest page index that is out of range must return `Err`
/// without crashing.  The input PDF is left unmodified (no output written).
#[test]
fn out_of_range_dest_returns_err() {
    let d = std::env::temp_dir();
    let inp = d.join(format!("wkx_lnk_oor_in_{}.pdf", std::process::id()));
    let out = d.join(format!("wkx_lnk_oor_out_{}.pdf", std::process::id()));

    write_3page(inp.to_str().unwrap());

    // dest_page=5 is out of range for a 3-page PDF.
    let links: &[LinkSpec] = &[(0, [10.0, 10.0, 100.0, 30.0], 5)];
    let result = wkhtmltox_pdf_sys::add_links(&inp, &out, links);
    assert!(result.is_err(), "out-of-range dest page must return Err");
    // The error message must mention the out-of-range condition.
    let msg = result.unwrap_err();
    assert!(
        msg.contains("out of range") || msg.contains("rc=3"),
        "error message should mention out-of-range, got: {msg}"
    );

    let _ = std::fs::remove_file(&inp);
    // out may or may not exist; attempt cleanup regardless.
    let _ = std::fs::remove_file(&out);
}
