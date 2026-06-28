// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Decode-only PDF parsing of engine-generated output using `lopdf` (pure safe Rust).
// This module is intentionally narrow: it only reads the `/Outlines` bookmark tree
// to produce exact per-heading page numbers from the Chromium-generated PDF.

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
