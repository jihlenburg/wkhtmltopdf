# wkhtmltox-rs Milestone 2b — TOC + chrome (exact pages, TOC, headers/footers, cover, links)

> SDD execution. Builds on M2a (`assemble_pdf`, QPDF shim: merge/set_outline/stamp_footer/page_count). Design basis: spec §5. Tested with MockRenderer + real QPDF; oracle-validated.

**Goal:** Turn the assembly core into a full document layer: exact per-heading page numbers, a generated Table of Contents, variable headers/footers, a cover page, and clickable internal links.

## Global Constraints
- Rust 2021; no Qt. `wkhtmltox-core`/`wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`; `unsafe`/FFI only in `wkhtmltox-pdf-sys` (C++ fns keep `catch(const std::exception&)`+`catch(...)`). LGPL headers; pristine build. Commit per task w/ Co-Authored-By trailer. qpdf 12.3.2.
- Decode-only PDF parsing of OUR engine output may use `lopdf` (now promoted to a core runtime dep) — trusted input.
- `--xsl-style-sheet` (custom XSLT TOC) is DEFERRED to post-v1; v1 TOC is generated in Rust from the outline tree.

### Task 1 — Exact per-heading page mapping
- Promote `lopdf` to `wkhtmltox-core` `[dependencies]`. In `assembly.rs`, after each object's part PDF is written, parse its `/Outlines` (Chromium emitted it via `generateDocumentOutline`) with lopdf → `Vec<(title,u32 local_page_0based,u8 level)>`; map to global `local + offset`. Use this EXACT outline instead of the object-level probe outline (keep probe as fallback when a part has no `/Outlines`).
- Test (`assembly_mock.rs` extension): MockRenderer returns a 2-page PDF that itself contains an `/Outlines` with one item whose `/Dest` is page 2; assemble one such object → resulting global bookmark page == 2 (exact), not 1. Add a `build_min_pdf_with_outline` test helper.
- Re-run the oracle assembly comparison (Task 6 harness) — outline page numbers should now track the oracle within ±1. Commit `feat(core): exact per-heading page mapping from engine outline`.

### Task 2 — TOC generation + fixed-point page recompute
- `wkhtmltox-core/src/toc.rs`: `render_toc_html(outline: &[OutlineNode]) -> String` (a default TOC: nested `<ul>`, each entry `title … page` with the heading text + the resolved page number; minimal inline CSS for leaders). 
- In `assembly.rs`: add a `with_toc: bool` path. Render the TOC HTML as a LEADING object via the Renderer, insert it before content. Because inserting the TOC shifts content pages, run a FIXED-POINT loop: build outline w/ placeholder pages → render TOC → learn its page count → recompute global pages (= local + cover_pages + toc_pages) → re-render TOC → iterate until `toc_page_count` stable (cap 3, log fallback).
- Test: MockRenderer scripted so the TOC object renders to a known page count; assert content page numbers are offset by the TOC length and the loop converges. Commit `feat(core): TOC generation + fixed-point page recompute`.

### Task 3 — Variable headers/footers (text)
- Extend the footer/header overlay: a `HeaderFooter { left,center,right, font_size, line: bool }` for top and bottom. Per page, substitute `[page] [topage] [frompage] [title] [section] [subsection] [date] [time]`. `[section]`/`[subsection]` = the nearest preceding outline entry of level 1/2 active on that page (tracked from the bookmark tree). Extend the QPDF shim to stamp at top and bottom with three cells (left/center/right) — generalize `wkx_pdf_stamp_footer` into `wkx_pdf_stamp_runninghdr` or add params; keep escaping + `q…Q`/`BT…ET` balance; use inheritance-aware `/Resources` access (the deferred M2a finding). 
- Test: a 3-page doc with `--footer-center "[page]/[topage]"` and `--header-right "[title]"` → assert per-page text. Commit `feat: variable headers/footers (text cells + [section]/[title]/[date])`.

### Task 4 — Cover page
- `assemble_pdf`: optional `cover: Option<Source>` rendered as the very first object, EXCLUDED from headers/footers and page numbering, and not in the TOC. Adjust page-offset math (cover_pages added to the global offset; numbering starts after the cover).
- Test: cover + 2 objects → cover is page 1 with no footer; content numbering starts at 1 on the cover's next page. Commit `feat(core): cover page support`.

### Task 5 — Clickable internal link synthesis
- Chromium omits clickable `/Link` GoTo annotations (bug 347674894) though destinations exist. Add `wkx_pdf_add_link(in,out, page, rect[4], dest_page)` (or batch) to the shim: create `/Annot /Subtype /Link /Rect /Dest [pageobj /XYZ ...] /Border [0 0 0]` on a page. In `assembly.rs`, for each internal anchor link from the probe (`links` with `internal:true` + rect + target anchor→page), synthesize the annotation. Also make TOC entries clickable (link each TOC line rect → its heading page).
- Test: a doc with an internal link → output page has a `/Link` annot with a GoTo `/Dest` to the right page (lopdf). Commit `feat: synthesize clickable internal link annotations (bug 347674894 mitigation)`.

### Task 6 — Oracle validation + milestone gate
- Extend the harness: assemble a TOC+cover+headers doc via our engine and the oracle (`wkhtmltopdf cover toc page ...`), compare outline tree + TOC entries + page counts structurally. Record `results-m2b.md`. Then: whole-milestone Opus review (correctness + security), fix wave, security audit, push.

## Self-Review
Covers spec §5.1 (exact pages, T1), §5.2 (TOC + fixed-point, T2), §5.3 (headers/footers, T3), cover (T4), clickable links / ⑩ (T5), oracle validation (T6). Deferred: `--xsl-style-sheet` custom XSLT, HTML headers/footers (only text cells in v1) → post-v1 or M2c. Security: new shim fns need catch backstops + bounds checks; link/header text from DOM is encoded via qpdf string APIs (safe, as M2a review confirmed).
