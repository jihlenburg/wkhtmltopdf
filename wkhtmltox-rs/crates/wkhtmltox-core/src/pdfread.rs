// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Decode-only PDF parsing of engine-generated output using `lopdf` (pure safe Rust).
// Reads:
//   - /Outlines bookmark tree → exact per-heading page numbers (Chromium path).
//   - /Names /Dests name-tree + /Dests flat dict → named destination → page index map.

use std::collections::HashMap;
use std::path::Path;

use lopdf::{Document, Object};

/// Decode a PDF string value (UTF-16BE with BOM, or Latin-1 / PDFDocEncoding) to a
/// Rust `String`.  Silently replaces any undecodable sequences.
fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE with BOM
        let shorts: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16(&shorts).unwrap_or_default()
    } else {
        // Latin-1 / PDFDocEncoding — lossless for the ASCII subset we care about
        String::from_utf8_lossy(bytes).into_owned()
    }
}

// ── Named destination extraction ─────────────────────────────────────────────

/// Parse named destinations from a PDF and return a map of name → 0-based page index.
///
/// Reads:
/// - The catalog `/Names` → `/Dests` name-tree (Chromium's format).
/// - The catalog `/Dests` flat dict (legacy, PDF 1.1 § 8.2.1).
///
/// Malformed or unresolvable entries are silently skipped.  Never panics.
pub fn extract_named_dests(pdf_path: &Path) -> HashMap<String, u32> {
    let doc = match Document::load(pdf_path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };

    // Build page object-id → 0-based page index.
    let page_map: HashMap<lopdf::ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(pg_num, obj_id)| (obj_id, pg_num - 1))
        .collect();

    let mut result: HashMap<String, u32> = HashMap::new();

    // ── Path 1: /Names /Dests name-tree ──────────────────────────────────────
    if let Some(root_id) = catalog_names_dests_root(&doc) {
        walk_name_tree(&doc, root_id, &page_map, &mut result);
    }

    // ── Path 2: /Dests flat dict in catalog (legacy) ─────────────────────────
    catalog_dests_flat(&doc, &page_map, &mut result);

    result
}

/// Locate the root of the `/Names /Dests` name-tree in the catalog.
/// Returns `None` if not present or if parsing fails.
fn catalog_names_dests_root(doc: &Document) -> Option<lopdf::ObjectId> {
    // Phase 1: get /Names reference from catalog.
    let names_ref: lopdf::ObjectId = {
        let catalog = doc.catalog().ok()?;
        catalog.get(b"Names").ok()?.as_reference().ok()?
    };

    // Phase 2: get /Dests reference from /Names dict.
    let dests_ref: lopdf::ObjectId = {
        let names_obj = doc.get_object(names_ref).ok()?;
        let names_dict = names_obj.as_dict().ok()?;
        names_dict.get(b"Dests").ok()?.as_reference().ok()?
    };

    Some(dests_ref)
}

/// Walk a name-tree node (and its descendants) rooted at `node_id`, inserting
/// `name → page_index` pairs into `out`.
fn walk_name_tree(
    doc: &Document,
    node_id: lopdf::ObjectId,
    page_map: &HashMap<lopdf::ObjectId, u32>,
    out: &mut HashMap<String, u32>,
) {
    // Extract /Names pairs and /Kids refs as owned data so the borrow on `doc`
    // is released before the recursive call.
    let (name_pairs, kid_refs) = {
        let Ok(obj) = doc.get_object(node_id) else { return };
        let Ok(dict) = obj.as_dict() else { return };

        // Leaf node: /Names [name dest name dest ...]
        let names: Vec<(Vec<u8>, Object)> =
            if let Ok(arr) = dict.get(b"Names").and_then(|o| o.as_array()) {
                arr.chunks(2)
                    .filter_map(|pair| {
                        if pair.len() < 2 {
                            return None;
                        }
                        let key = match &pair[0] {
                            Object::String(b, _) => b.clone(),
                            _ => return None,
                        };
                        Some((key, pair[1].clone()))
                    })
                    .collect()
            } else {
                vec![]
            };

        // Intermediate node: /Kids [ref ref ...]
        let kids: Vec<lopdf::ObjectId> =
            if let Ok(arr) = dict.get(b"Kids").and_then(|o| o.as_array()) {
                arr.iter().filter_map(|o| o.as_reference().ok()).collect()
            } else {
                vec![]
            };

        (names, kids)
    }; // doc borrow released here

    for (key_bytes, dest_obj) in name_pairs {
        let name = String::from_utf8_lossy(&key_bytes).into_owned();
        if let Some(page) = resolve_dest_page(doc, &dest_obj, page_map) {
            out.insert(name, page);
        }
    }

    for kid_id in kid_refs {
        walk_name_tree(doc, kid_id, page_map, out);
    }
}

/// Try to read a `/Dests` flat dict directly from the catalog (PDF 1.1 legacy).
/// Each key is an ASCII name; each value is a dest array or indirect ref to one.
fn catalog_dests_flat(
    doc: &Document,
    page_map: &HashMap<lopdf::ObjectId, u32>,
    out: &mut HashMap<String, u32>,
) {
    // Get /Dests object-id from catalog.
    let dests_id: lopdf::ObjectId = {
        let Ok(catalog) = doc.catalog() else { return };
        let Ok(val) = catalog.get(b"Dests") else { return };
        let Ok(id) = val.as_reference() else { return };
        id
    };

    // Extract all (key, value) pairs as owned data.
    let pairs: Vec<(Vec<u8>, Object)> = {
        let Ok(obj) = doc.get_object(dests_id) else { return };
        let Ok(dict) = obj.as_dict() else { return };
        dict.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };

    for (key_bytes, dest_obj) in pairs {
        let name = String::from_utf8_lossy(&key_bytes).into_owned();
        if let Some(page) = resolve_dest_page(doc, &dest_obj, page_map) {
            out.insert(name, page);
        }
    }
}

/// Resolve a destination value to a 0-based page index.
///
/// A destination is either an inline array `[page_ref /XYZ ...]` or an indirect
/// reference that resolves to such an array.
fn resolve_dest_page(
    doc: &Document,
    dest_obj: &Object,
    page_map: &HashMap<lopdf::ObjectId, u32>,
) -> Option<u32> {
    match dest_obj {
        Object::Array(arr) => {
            let page_ref = arr.first()?.as_reference().ok()?;
            page_map.get(&page_ref).copied()
        }
        Object::Reference(id) => {
            // Dest stored as indirect reference — resolve then inspect first element.
            let page_ref = {
                let obj = doc.get_object(*id).ok()?;
                let arr = obj.as_array().ok()?;
                arr.first()?.as_reference().ok()?
            };
            page_map.get(&page_ref).copied()
        }
        _ => None,
    }
}

// ── Outline extraction ────────────────────────────────────────────────────────

/// Parse the `/Outlines` bookmark tree embedded by the render engine and return a flat
/// list of `(title, local_page_0based, level)` tuples in tree-order (depth-first).
///
/// Returns an **empty** `Vec` if the file cannot be loaded, has no `/Outlines` root,
/// or has an empty tree.  Malformed individual entries are silently skipped — the
/// function never panics on corrupt engine output.
pub fn extract_outline(pdf_path: &Path) -> Vec<(String, u32, u8)> {
    let doc = match Document::load(pdf_path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    // Build an inverted map: page ObjectId → 0-based page index.
    // `get_pages()` returns BTreeMap<u32 (1-based), ObjectId>.
    let page_id_to_index: HashMap<lopdf::ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(pg_num, obj_id)| (obj_id, pg_num - 1))
        .collect();

    // Locate /Outlines in the document catalog.
    let outlines_ref = match doc.catalog() {
        Ok(catalog) => match catalog.get(b"Outlines").and_then(|o| o.as_reference()) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    // Catalog borrow released here.

    // Resolve /Outlines dict and find the /First child.
    let first_ref = match doc.get_object(outlines_ref).and_then(|o| o.as_dict()) {
        Ok(d) => match d.get(b"First").and_then(|o| o.as_reference()) {
            Ok(r) => r,
            Err(_) => return Vec::new(), // empty /Outlines tree
        },
        Err(_) => return Vec::new(),
    };
    // Outlines-dict borrow released here.

    let mut result = Vec::new();
    walk_outline(&doc, first_ref, 1, &page_id_to_index, &mut result);
    result
}

/// Depth-first walk starting at `start_ref` (a sibling chain) at the given `level`.
/// Pushes `(title, page_0based, level)` for each item that has a valid title and dest.
fn walk_outline(
    doc: &Document,
    start_ref: lopdf::ObjectId,
    level: u8,
    page_id_to_index: &HashMap<lopdf::ObjectId, u32>,
    out: &mut Vec<(String, u32, u8)>,
) {
    let mut current_ref = start_ref;
    loop {
        // Extract all needed values as owned data so the borrow of `doc` is released
        // before the recursive call and the next iteration.
        let (title, page_0based, first_child, next_sibling) = {
            let dict = match doc.get_object(current_ref).and_then(|o| o.as_dict()) {
                Ok(d) => d,
                Err(_) => break,
            };

            let title = dict
                .get(b"Title")
                .ok()
                .and_then(|o| match o {
                    Object::String(bytes, _) => Some(decode_pdf_string(bytes)),
                    _ => None,
                })
                .unwrap_or_default();

            // /Dest is expected to be an array whose first element is a page object ref.
            let page_0based = dict
                .get(b"Dest")
                .ok()
                .and_then(|dest| -> Option<u32> {
                    let arr = dest.as_array().ok()?;
                    let page_ref = arr.first()?.as_reference().ok()?;
                    page_id_to_index.get(&page_ref).copied()
                })
                .unwrap_or(0);

            let first_child = dict.get(b"First").and_then(|o| o.as_reference()).ok();
            let next_sibling = dict.get(b"Next").and_then(|o| o.as_reference()).ok();

            (title, page_0based, first_child, next_sibling)
        }; // dict (and doc borrow) released here

        if !title.is_empty() {
            out.push((title, page_0based, level));
        }

        // Recurse into children before advancing to the next sibling.
        if let Some(child_ref) = first_child {
            walk_outline(doc, child_ref, level.saturating_add(1), page_id_to_index, out);
        }

        match next_sibling {
            Some(r) => current_ref = r,
            None => break,
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid 3-page PDF that has one named destination
    /// `"x"` pointing at the third page (0-based index 2).
    ///
    /// PDF object layout:
    ///   1 – Catalog  (/Pages 2 0 R  /Names 6 0 R)
    ///   2 – Pages    (/Kids [3 0 R 4 0 R 5 0 R]  /Count 3)
    ///   3 – Page 1
    ///   4 – Page 2
    ///   5 – Page 3   ← destination for "x"
    ///   6 – /Names dict  (/Dests 7 0 R)
    ///   7 – /Dests name-tree leaf  (/Names [(x) [5 0 R /XYZ 0 0 0]])
    fn build_pdf_with_named_dest() -> Vec<u8> {
        const N: usize = 7;
        let mut buf = String::new();
        buf.push_str("%PDF-1.4\n");
        let mut offsets = vec![0usize; N + 1]; // index 0 unused; [1..=7]

        offsets[1] = buf.len();
        buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R/Names 6 0 R>>\nendobj\n");

        offsets[2] = buf.len();
        buf.push_str("2 0 obj\n<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3>>\nendobj\n");

        offsets[3] = buf.len();
        buf.push_str("3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");

        offsets[4] = buf.len();
        buf.push_str("4 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");

        offsets[5] = buf.len();
        buf.push_str("5 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");

        offsets[6] = buf.len();
        buf.push_str("6 0 obj\n<</Dests 7 0 R>>\nendobj\n");

        offsets[7] = buf.len();
        // Name-tree leaf: /Names array with one (name, dest) pair.
        // (x) = string literal "x"; [5 0 R /XYZ 0 0 0] = dest array pointing at page 3 obj.
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

    #[test]
    fn extract_named_dests_finds_x_on_page2() {
        let tmp = std::env::temp_dir()
            .join(format!("wkx_ndr_{}.pdf", std::process::id()));
        std::fs::write(&tmp, build_pdf_with_named_dest()).unwrap();

        let dests = extract_named_dests(&tmp);
        let _ = std::fs::remove_file(&tmp);

        assert!(!dests.is_empty(), "named dests map must not be empty");
        assert_eq!(
            dests.get("x").copied(),
            Some(2),
            "named dest 'x' must resolve to 0-based page index 2"
        );
    }

    #[test]
    fn extract_named_dests_on_no_dests_pdf_returns_empty() {
        let tmp = std::env::temp_dir()
            .join(format!("wkx_ndr_empty_{}.pdf", std::process::id()));
        // A 2-page PDF with no /Names or /Dests.
        let n_objs = 4usize;
        let mut buf = String::new();
        buf.push_str("%PDF-1.4\n");
        let mut offs = Vec::with_capacity(n_objs);
        offs.push(buf.len());
        buf.push_str("1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n");
        offs.push(buf.len());
        buf.push_str("2 0 obj\n<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>\nendobj\n");
        offs.push(buf.len());
        buf.push_str("3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");
        offs.push(buf.len());
        buf.push_str("4 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>\nendobj\n");
        let xref_off = buf.len();
        buf.push_str(&format!("xref\n0 {}\n", n_objs + 1));
        buf.push_str("0000000000 65535 f \n");
        for &o in &offs {
            buf.push_str(&format!("{:010} 00000 n \n", o));
        }
        buf.push_str(&format!(
            "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{}\n%%EOF\n",
            n_objs + 1,
            xref_off
        ));
        std::fs::write(&tmp, buf).unwrap();

        let dests = extract_named_dests(&tmp);
        let _ = std::fs::remove_file(&tmp);

        assert!(dests.is_empty(), "no /Names → empty map expected");
    }
}
