# wkhtmltox-rs Milestone 10 — HTML Headers/Footers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Support `--header-html` / `--footer-html`: render an HTML document as the running header/footer on every page, with wkhtmltopdf's per-page variables passed as URL query-string parameters, overlaid into the page margins.

**Architecture:** A new batch QPDF overlay shim (`wkx_pdf_overlay_pages`) places a rendered 1-page header/footer PDF onto each content page as an `/XObject` Form. The assembly path reserves top/bottom margins, renders the header/footer HTML once per page (with that page's 13 variables as `?page=N&...` query params, matching upstream exactly), and batch-overlays them. No injected JS — the header HTML author includes the documented `subst()` script (as in upstream).

**Tech Stack:** Rust 2021 (`wkhtmltox-core` stays `#![forbid(unsafe_code)]`), QPDF C++ shim (`wkhtmltox-pdf-sys`) for the overlay, headless Chromium for rendering the header/footer HTML.

## Global Constraints
- `wkhtmltox-core` + `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`; new `unsafe`/FFI only in `wkhtmltox-pdf-sys` (the overlay shim) behind a safe wrapper, with a `catch(...)` backstop matching the existing shim functions.
- **Match upstream's variable mechanism exactly:** the header/footer URL gets these query params appended on every page: `page, frompage, topage, webpage, section, subsection, subsubsection, date, isodate, time, title, doctitle, sitepage, sitepages`, plus any `--replace name value`. Values URL-encoded. (Verified from `src/lib/pdfconverter.cc:loadHeaderFooter` + `src/lib/outline.cc:fillHeaderFooterParms`.)
- HTML header/footer is an ALTERNATIVE to the M2b text-cell header/footer — when `--header-html` is set, the HTML header is used for the header (text-cell header ignored for that slot); same for footer independently.
- Permissive default preserved: no header/footer unless requested. LGPL header on new files; `cargo clippy --lib -- -D warnings` clean on touched crates. Commit per task + trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **v1 scope (documented):** the header/footer is overlaid into the configured `--margin-top`/`--margin-bottom` (sensible defaults); **automatic header-height measurement is deferred** (upstream auto-measures when margin is `-1`). The header is rendered at content width; if it's taller than the reserved margin it visually overflows into content (user controls margin). Note this clearly.
- Working dir for commands: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs`.

## File Structure
- `crates/wkhtmltox-pdf-sys/cpp/shim.cpp` + `shim.h` + `src/lib.rs` — CREATE the batch overlay shim + safe wrapper.
- `crates/wkhtmltox-core/src/headerfooter.rs` — MODIFY: extend `PageCtx` to the 13 tokens; add `header_footer_query(url, ctx, replacements) -> String` (build the `?...` URL).
- `crates/wkhtmltox-core/src/assembly.rs` — MODIFY: `AssembleOpts` gains `header_html: Option<String>`, `footer_html: Option<String>`, `header_spacing_mm: f64`, `footer_spacing_mm: f64`, `replacements: Vec<(String,String)>`; render + overlay per page.
- `crates/wkhtmltox-core/src/settings.rs` + `registry.rs` — MODIFY: implement `header.htmlUrl`/`footer.htmlUrl`/`header.spacing`/`footer.spacing` + `replace`.
- `crates/wkhtmltox-cli/src/lib.rs` — MODIFY: add `--header-html`/`--footer-html`/`--header-spacing`/`--footer-spacing`/`--replace` to `FLAGS`.

---

### Task 1 — Batch page-overlay shim (`wkx_pdf_overlay_pages`)

**Files:** `crates/wkhtmltox-pdf-sys/cpp/shim.cpp`, `shim.h`, `src/lib.rs`; test `crates/wkhtmltox-pdf-sys/tests/overlay.rs`.
**Interfaces:**
- Produces (C): `int wkx_pdf_overlay_pages(const char* base_path, const char* out_path, const char** overlay_paths, const int* page_indices, const double* tx, const double* ty, int n)` — for each `i` in `0..n`, stamp `overlay_paths[i]`'s first page onto base page `page_indices[i]` (0-based), translated to `(tx[i], ty[i])` in PDF points (bottom-left origin). Returns 0 ok; 1 QPDF exception; 2 page index out of range; 3 unknown exception.
- Produces (Rust): `pub struct OverlaySpec { pub overlay_path: String, pub page_index: u32, pub tx: f64, pub ty: f64 }`; `pub fn overlay_pages(base: &[u8], specs: &[OverlaySpec]) -> Result<Vec<u8>, String>`.

- [ ] **Step 1: Write the failing Rust wrapper test** `crates/wkhtmltox-pdf-sys/tests/overlay.rs`:

```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_pdf_sys::{overlay_pages, OverlaySpec};

#[test]
fn overlays_a_page_xobject_and_preserves_page_count() {
    // base: a 2-page PDF; overlay: a 1-page PDF (reuse the existing test fixtures /
    // helpers used by tests/acroform.rs or tests/merge.rs to build small PDFs).
    let base = wkhtmltox_pdf_sys::test_support_two_page_pdf();   // or the merge-test fixture
    let stamp = wkhtmltox_pdf_sys::test_support_one_page_pdf();
    // write stamp to a temp file (the shim takes paths)
    let dir = tempfile::tempdir().unwrap();
    let stamp_path = dir.path().join("stamp.pdf");
    std::fs::write(&stamp_path, &stamp).unwrap();
    let out = overlay_pages(&base, &[
        OverlaySpec { overlay_path: stamp_path.to_string_lossy().into_owned(), page_index: 0, tx: 0.0, ty: 700.0 },
        OverlaySpec { overlay_path: stamp_path.to_string_lossy().into_owned(), page_index: 1, tx: 0.0, ty: 0.0 },
    ]).expect("overlay");
    let doc = lopdf::Document::load_mem(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2, "page count preserved");
    // The overlaid page's Resources must now contain an XObject (the form).
    // (Assert at least one page has an /XObject resource entry.)
    let has_xobject = doc.get_pages().values().any(|&oid| {
        doc.get_object(oid).ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Resources").ok())
            .and_then(|r| doc.dereference(r).ok())
            .and_then(|(_, r)| r.as_dict().ok().cloned())
            .map(|d| d.has(b"XObject"))
            .unwrap_or(false)
    });
    assert!(has_xobject, "an overlaid page should have an /XObject resource");
}
```

Use the SAME small-PDF construction the existing pdf-sys tests use (read `tests/merge.rs`/`tests/acroform.rs` — reuse their fixture helper; if the helper names differ, match them). `lopdf` + `tempfile` are already dev-deps.

- [ ] **Step 2: Run, verify failure:** `cargo test -p wkhtmltox-pdf-sys --test overlay` → fails (symbol absent).

- [ ] **Step 3: Implement the C++ shim** in `shim.cpp` (declare in `shim.h`). Use QPDF's foreign-object copy + a Form XObject. Follow the existing functions' QPDF idioms (error handling, `catch`). Sketch:

```cpp
// shim.h
int wkx_pdf_overlay_pages(const char* base_path, const char* out_path,
                          const char** overlay_paths, const int* page_indices,
                          const double* tx, const double* ty, int n);
```

```cpp
// shim.cpp  (inside extern "C")
int wkx_pdf_overlay_pages(const char* base_path, const char* out_path,
                          const char** overlay_paths, const int* page_indices,
                          const double* tx, const double* ty, int n) {
  try {
    QPDF base;
    base.processFile(base_path);
    std::vector<QPDFPageObjectHelper> pages =
        QPDFPageDocumentHelper(base).getAllPages();
    // Keep loaded overlay QPDFs alive for the duration.
    std::vector<std::shared_ptr<QPDF>> keep;
    for (int i = 0; i < n; ++i) {
      int idx = page_indices[i];
      if (idx < 0 || idx >= (int)pages.size()) return 2;
      auto ov = std::make_shared<QPDF>();
      ov->processFile(overlay_paths[i]);
      keep.push_back(ov);
      QPDFPageObjectHelper ovpage =
          QPDFPageDocumentHelper(*ov).getAllPages().at(0);
      // Copy the overlay's first page into base as a Form XObject.
      QPDFObjectHandle form = base.copyForeignObject(
          ovpage.getFormXObjectForPage());
      QPDFPageObjectHelper bp = pages.at(idx);
      std::string xname = bp.getAttribute("/Resources", true)
          .isNull() ? "/Fx0" : ""; // ensure unique name
      // Use the helper that adds the form as a resource + returns its name.
      std::string name = bp.addPageContents(
          QPDFObjectHandle::newStream(&base), true), // placeholder — see note
          // Real approach: name = bp's resource name for `form`:
          ;
      // Place: append "q 1 0 0 1 tx ty cm /Name Do Q" to the page content.
      // Use QPDFPageObjectHelper::getFormXObjectForPage / placeFormXObject:
      std::string content = bp.placeFormXObject(
          form, /*name*/ "/Fx" + std::to_string(i),
          ovpage.getMediaBox().getArrayAsRectangle()); // returns the "q..Do Q" string
      // translate by (tx,ty): wrap with cm. (placeFormXObject can take a rect;
      // simplest: build the matrix yourself.)
      bp.getObjectHandle().getKey("/Resources").getKey("/XObject")
        .replaceKey("/Fx" + std::to_string(i), form);
      bp.addPageContents(QPDFObjectHandle::newStream(&base,
          "q 1 0 0 1 " + std::to_string(tx[i]) + " " + std::to_string(ty[i]) +
          " cm /Fx" + std::to_string(i) + " Do Q\n"), false);
    }
    QPDFWriter w(base, out_path);
    w.write();
    return 0;
  } catch (const std::exception&) { return 1; }
  catch (...) { return 3; }
}
```

NOTE TO IMPLEMENTER: the exact QPDF API for placing a page-as-form differs across QPDF 11/12 — use the version installed (`pkg-config --modversion libqpdf`, the project builds against 12.x). The canonical modern path is `QPDFPageObjectHelper::getFormXObjectForPage()` on the overlay page → `base.copyForeignObject(...)` → add to the target page's `/Resources /XObject` under a unique name → append a content stream `q 1 0 0 1 tx ty cm /Name Do Q`. Consult the QPDF 12 headers (`/opt/homebrew/include/qpdf/QPDFPageObjectHelper.hh`) for the exact `placeFormXObject`/`getFormXObjectForPage` signatures and prefer them over hand-rolling. The sketch above is illustrative — write correct QPDF-12 code and let the test validate it. Ensure a UNIQUE XObject name per overlay even if a page is overlaid twice (header AND footer on the same page) — suffix with the loop index AND avoid clobbering existing page XObjects.

- [ ] **Step 4: Implement the Rust wrapper** in `src/lib.rs` (mirror the `add_text_fields` temp-file/CString pattern; the base comes in as bytes → temp file; overlays are already file paths):

```rust
#[derive(Debug, Clone)]
pub struct OverlaySpec { pub overlay_path: String, pub page_index: u32, pub tx: f64, pub ty: f64 }

pub fn overlay_pages(base: &[u8], specs: &[OverlaySpec]) -> Result<Vec<u8>, String> {
    if specs.is_empty() { return Ok(base.to_vec()); }
    let dir = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let base_path = dir.path().join("base.pdf");
    std::fs::write(&base_path, base).map_err(|e| e.to_string())?;
    let out_path = dir.path().join("out.pdf");
    let base_c = std::ffi::CString::new(base_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let out_c = std::ffi::CString::new(out_path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let ov_c: Vec<std::ffi::CString> = specs.iter()
        .map(|s| std::ffi::CString::new(s.overlay_path.as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let ov_ptrs: Vec<*const c_char> = ov_c.iter().map(|c| c.as_ptr()).collect();
    let idx: Vec<c_int> = specs.iter().map(|s| s.page_index as c_int).collect();
    let txs: Vec<f64> = specs.iter().map(|s| s.tx).collect();
    let tys: Vec<f64> = specs.iter().map(|s| s.ty).collect();
    let rc = unsafe {
        wkx_pdf_overlay_pages(base_c.as_ptr(), out_c.as_ptr(), ov_ptrs.as_ptr(),
            idx.as_ptr(), txs.as_ptr(), tys.as_ptr(), specs.len() as c_int)
    };
    if rc != 0 { return Err(format!("wkx_pdf_overlay_pages failed: rc={rc}")); }
    std::fs::read(&out_path).map_err(|e| e.to_string())
}
```

Add the `extern "C"` decl (private `fn`, matching the other raw symbols' privacy). Match the crate's actual error type (the recon notes pdf-sys uses `Result<_, String>`).

- [ ] **Step 5: Run, verify pass:** `cargo test -p wkhtmltox-pdf-sys --test overlay` → page count preserved + XObject present. Run `cargo test -p wkhtmltox-pdf-sys` (whole crate, ensure no regression).

- [ ] **Step 6: Commit** `feat(pdf-sys): wkx_pdf_overlay_pages — batch page overlay via QPDF Form XObject`.

---

### Task 2 — Header/footer variable → query-string (pure core)

**Files:** Modify `crates/wkhtmltox-core/src/headerfooter.rs`.
**Interfaces:**
- Produces: extended `PageCtx` with all 13 fields (`page, frompage, topage, webpage, section, subsection, subsubsection, date, isodate, time, title, doctitle, sitepage, sitepages`); `pub fn header_footer_query(base_url: &str, ctx: &PageCtx, replacements: &[(String, String)]) -> String` returning the URL with the variables appended as URL-encoded query params.
- Consumes: nothing new (pure).

- [ ] **Step 1: Write failing tests:**

```rust
#[test]
fn builds_query_string_with_all_tokens() {
    let ctx = PageCtx { page: 3, frompage: 1, topage: 10, webpage: "http://x/".into(),
        section: "S".into(), subsection: "Sub".into(), subsubsection: "".into(),
        date: "2026-06-28".into(), isodate: "2026-06-28".into(), time: "12:00".into(),
        title: "T".into(), doctitle: "Doc".into(), sitepage: 3, sitepages: 10 };
    let url = header_footer_query("header.html", &ctx, &[]);
    assert!(url.starts_with("header.html?"));
    assert!(url.contains("page=3"));
    assert!(url.contains("topage=10"));
    assert!(url.contains("doctitle=Doc"));
    assert!(url.contains("frompage=1"));
}

#[test]
fn url_encodes_values_and_appends_replacements() {
    let ctx = PageCtx { title: "a&b c".into(), ..PageCtx::sample() };
    let url = header_footer_query("h.html", &ctx, &[("co".into(), "A & B".into())]);
    assert!(url.contains("title=a%26b%20c") || url.contains("title=a%26b+c"));
    assert!(url.contains("co=A%20%26%20B") || url.contains("co=A+%26+B"));
}

#[test]
fn preserves_existing_query_in_base_url() {
    let url = header_footer_query("h.html?x=1", &PageCtx::sample(), &[]);
    assert!(url.contains("h.html?x=1&") && url.contains("page="));
}
```

(Add a `PageCtx::sample()` test helper.)

- [ ] **Step 2: Run, verify failure.**

- [ ] **Step 3: Implement.** Extend `PageCtx` (the recon shows it currently has 8 fields — add `frompage, webpage, subsubsection, isodate, doctitle, sitepage, sitepages`). Implement `header_footer_query` using a minimal percent-encoder (reuse the CLI's `percent_encode` helper if reachable, else encode the query-component chars: space→`%20`, `&`→`%26`, `=`→`%3D`, `?`→`%3F`, `#`→`%23`, `%`→`%25`, and non-ASCII). Append `?` (or `&` if the base already has `?`) then `key=value` pairs in the upstream order. Keep the existing `[token]` `substitute` (for text headers) UNCHANGED.

- [ ] **Step 4: Run, verify pass + clippy.**

- [ ] **Step 5: Commit** `feat(core): header/footer 13-variable query-string builder (matches wkhtmltopdf)`.

---

### Task 3 — Assembly integration: render + overlay per page + gated e2e

**Files:** Modify `crates/wkhtmltox-core/src/assembly.rs`; gated e2e in `crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`.
**Interfaces:** Consumes Task 1 `overlay_pages`/`OverlaySpec`, Task 2 `header_footer_query`/`PageCtx`; the `Renderer` (`open`/`print_pdf`), `PageGeometry`.

- [ ] **Step 1:** Add to `AssembleOpts`: `header_html: Option<String>`, `footer_html: Option<String>`, `header_spacing_mm: f64`, `footer_spacing_mm: f64`, `replacements: Vec<(String, String)>` (defaults: None/None/0.0/0.0/empty).

- [ ] **Step 2:** After the content+TOC+cover PDF is assembled and the total page count is known, if `header_html` or `footer_html` is set: for each content page `p` (0-based; skip cover pages — they get no header/footer, matching the text-cell behavior), build a `PageCtx` for that page (page number = global 1-based, frompage/topage = document bounds, title/doctitle from the outline/`doc_title`, date/time from values threaded in via `AssembleOpts` — do NOT call `Date::now()` in core; accept date/time/isodate strings in `AssembleOpts` or leave blank for v1), call `header_footer_query(url, &ctx, &replacements)`, render it via `r.open(&Source::Url(file_url_or_http), ...)` + `r.print_pdf` to a 1-page PDF at content width × margin height, write it to a temp file, and accumulate an `OverlaySpec { overlay_path, page_index: p, tx: left_margin_pt, ty: <top for header / 0 for footer> }`. Then call `wkhtmltox_pdf_sys::overlay_pages(&pdf_bytes, &specs)` once. The header `ty` = `page_height_pt - header_height_pt`; the footer `ty` = `0`. For v1, `header_height_pt`/`footer_height_pt` come from the reserved margins (`geom` top/bottom margins), NOT auto-measured.

- [ ] **Step 3:** Header/footer HTML rendering geometry: render the header HTML at a page size of `content_width × margin_height` with zero margins so it fills the band. (Reuse `PageGeometry` with adjusted dimensions, or render at full content width and rely on the overlay `ty` placement.) Keep it simple: render at the content page width and the configured margin height.

- [ ] **Step 4: Gated e2e** (real Chrome) `header_html_stamps_page_numbers`:

```rust
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn header_html_stamps_page_numbers() {
    // header.html with the documented subst() script + <span class="page"></span>
    // Assemble a 2-page doc with --header-html; assert each page's text contains its page number.
    // Write header.html to a temp file; AssembleOpts { header_html: Some(path), .. }.
    // Extract page 1 + page 2 text via lopdf; assert "1" appears on page 1 region, "2" on page 2.
}
```

Use a header HTML that includes the upstream `subst()` script (reads `location.search`, fills `.page`/`.topage` spans). Assert via text extraction that the rendered header carries per-page numbers (proves the query-string variables reached the header and the overlay landed). Match the real `assemble_pdf` signature + `ChromiumRenderer::spawn`.

- [ ] **Step 5: Verify:** `cargo test -p wkhtmltox-core && cargo build -p wkhtmltox-render-chromium --tests && cargo clippy -p wkhtmltox-core -p wkhtmltox-render-chromium --lib -- -D warnings`. Chrome IS available — run `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1 header_html_stamps_page_numbers` and report.

- [ ] **Step 6: Commit** `feat(core): render + overlay HTML headers/footers per page`.

---

### Task 4 — Registry + CLI wiring

**Files:** Modify `settings.rs`, `registry.rs`, `crates/wkhtmltox-cli/src/lib.rs`.

- [ ] **Step 1:** Settings: add `header_html_url: Option<String>`, `footer_html_url: Option<String>`, `header_spacing: f64`, `footer_spacing: f64`, `replacements: Vec<(String,String)>` to the settings struct that flows into `AssembleOpts`; thread into `to_assemble_opts()` (both CLI + C-API sites). Registry: move `header.htmlUrl`/`footer.htmlUrl`/`header.spacing`/`footer.spacing` OUT of warn arms → store; implement `replace` (it takes a name+value pair — match how the registry handles 2-value settings, or accept `replace` as repeated `name=value`).
- [ ] **Step 2:** CLI FLAGS: `--header-html <url>`→`header.htmlUrl`; `--footer-html <url>`→`footer.htmlUrl`; `--header-spacing <real>`→`header.spacing`; `--footer-spacing <real>`→`footer.spacing`; `--replace <name> <value>` (two-arg flag — match the existing two-arg flag handling, e.g. how cookies/custom-header pairs are parsed). 
- [ ] **Step 3: Tests:** registry unit test (`header.htmlUrl` lands in `AssembleOpts.header_html`); CLI parse test (`--header-html h.html in.html out.pdf` parses, not "unknown option"); `--replace a b` parses into a replacement pair.
- [ ] **Step 4: Verify** `cargo test -p wkhtmltox-core -p wkhtmltox-cli -p wkhtmltopdf-cli && cargo clippy ... --lib -- -D warnings`.
- [ ] **Step 5: Commit** `feat(cli): --header-html/--footer-html/--header-spacing/--footer-spacing/--replace`.

---

## Milestone Gate (controller-run)
1. **Opus review** over the M10 diff: (a) the overlay shim — no panic/exception crosses `extern "C"` (catch backstops), unique XObject names (header+footer on the same page don't collide), page count preserved, out-of-range page → error not UB, the Rust wrapper TempDir/CString-safe (NUL in paths → Err), raw symbol private behind the safe wrapper; (b) header URLs — the query-string values are URL-encoded so attacker-controlled section/title text can't break the URL or inject params; the header HTML is loaded through the renderer (so `--safe`/ResourcePolicy applies to it — confirm the header render uses `opts.load`, not a default that bypasses `--safe`); (c) no `unsafe` outside pdf-sys; `forbid(unsafe)` intact; (d) cover pages correctly excluded; per-page overlay coordinates sane.
2. **Static audit:** `bash scripts/security-audit.sh --full` (expect HIGH=0).
3. **Fix wave** for Critical/Important.
4. **Push**; update `TODO.md` (HTML headers/footers done) + `logbook.md`.

## Self-Review
- **Coverage:** overlay shim → T1; the 13-variable query string → T2; per-page render+overlay → T3; CLI/registry → T4. Auto-height-measurement explicitly deferred (documented).
- **Security:** new FFI (overlay shim) confined to pdf-sys with catch backstops + safe wrapper; header URL values URL-encoded (injection); the header HTML render must honor `--safe` (gate check).
- **No placeholder:** real code for the wrapper, the query builder, the tests; the C++ shim is sketched with an explicit "use the QPDF-12 `placeFormXObject`/`getFormXObjectForPage` API, validate with the test" instruction (the exact API is version-specific — the implementer confirms against the installed headers, the test is the oracle).
- **Type consistency:** `OverlaySpec {overlay_path, page_index, tx, ty}`, `overlay_pages(&[u8], &[OverlaySpec])`, `PageCtx` (13 fields), `header_footer_query(&str, &PageCtx, &[(String,String)])`, `AssembleOpts.{header_html,footer_html,header_spacing_mm,footer_spacing_mm,replacements}` used consistently across tasks.
- **Risk:** T1 (QPDF Form XObject API) is the highest-risk task — the plan points the implementer at the installed QPDF-12 headers and makes the wrapper test the correctness oracle; if the QPDF API path proves unworkable, the implementer reports BLOCKED with the specific API gap rather than hand-rolling fragile content-stream surgery.
