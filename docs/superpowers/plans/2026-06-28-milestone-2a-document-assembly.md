# wkhtmltox-rs Milestone 2a — Document Assembly Core — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Turn N rendered objects into one assembled PDF with a correct bookmark/outline tree and continuous page numbering — the engine-independent document-assembly core of the spec §5 pipeline.

**Architecture:** A safe-Rust `outline` + `assembly` layer in `wkhtmltox-core` drives the page→structure model from the JS "probe" + Chromium's `generateDocumentOutline`; the actual PDF mutation (merge, /Outlines, page-number overlay) is done in the `wkhtmltox-pdf-sys` QPDF C++ shim behind a thin safe Rust wrapper. Tested with `MockRenderer` + real QPDF on synthetic PDFs; validated end-to-end against the oracle harness.

**Tech Stack:** Rust (core, safe); QPDF C++ shim via FFI (pdf-sys); `serde_json` (probe parsing); `lopdf` 0.42 (dev-only PDF assertions); Chromium/CDP (existing renderer) for integration.

## Global Constraints

- Rust edition 2021. No Qt. `wkhtmltox-core` and `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`; `unsafe`/FFI only in `wkhtmltox-pdf-sys`.
- Every source file starts with the LGPLv3 header. Pristine build (no warnings).
- C++ `extern "C"` functions MUST have `catch(const std::exception&)` + `catch(...)` backstops (no exception crosses FFI); bounds-check all indices.
- Do not break the existing `Renderer` trait or the C ABI headers. Reuse `wkhtmltox-core::render::*` types.
- qpdf 12.3.2 is installed (Homebrew); the shim links it via pkg-config (already wired in build.rs). If a QPDF 12.x API signature differs from the code below, adapt minimally and note it — do not change exported C symbol names/signatures.
- All work under `wkhtmltox-rs/`. Commit per task with the `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.

---

### Task 1: Outline model from probe JSON (pure safe Rust)

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/src/outline.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct Heading { pub level: u8, pub text: String, pub anchor: Option<String>, pub page: u32 }`
  - `pub struct OutlineNode { pub title: String, pub page: u32, pub children: Vec<OutlineNode> }`
  - `pub const PROBE_JS: &str` — the script handed to `eval_json` to extract document structure.
  - `pub fn parse_probe(v: &serde_json::Value) -> Vec<Heading>` — flat heading list from probe JSON.
  - `pub fn build_outline(headings: &[Heading]) -> Vec<OutlineNode>` — nests by `level` into a tree.

- [ ] **Step 1: Write the failing test**

`wkhtmltox-rs/crates/wkhtmltox-core/src/outline.rs` (tests at bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn probe() -> serde_json::Value {
        serde_json::json!({"headings":[
            {"level":1,"text":"A","anchor":"a","page":1},
            {"level":2,"text":"A.1","anchor":"a1","page":1},
            {"level":1,"text":"B","anchor":"b","page":3}
        ]})
    }
    #[test]
    fn parses_and_nests() {
        let hs = parse_probe(&probe());
        assert_eq!(hs.len(), 3);
        let tree = build_outline(&hs);
        assert_eq!(tree.len(), 2);                 // A, B at top level
        assert_eq!(tree[0].title, "A");
        assert_eq!(tree[0].children.len(), 1);     // A.1 under A
        assert_eq!(tree[0].children[0].page, 1);
        assert_eq!(tree[1].title, "B");
        assert_eq!(tree[1].page, 3);
    }
    #[test]
    fn probe_js_is_present() { assert!(PROBE_JS.contains("headings")); }
}
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core outline`
Expected: FAIL — module `outline` not found.

- [ ] **Step 3: Implement**

`wkhtmltox-rs/crates/wkhtmltox-core/src/outline.rs` (above tests):
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.

/// JS injected via `Renderer::eval_json` to extract document structure as JSON.
/// `page` is filled later from engine destinations; here it defaults to 0.
pub const PROBE_JS: &str = r#"(() => {
  const hs = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].map(h => ({
    level: Number(h.tagName.substring(1)),
    text: (h.textContent || '').trim(),
    anchor: h.id || null,
    page: 0
  }));
  return { headings: hs };
})()"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading { pub level: u8, pub text: String, pub anchor: Option<String>, pub page: u32 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode { pub title: String, pub page: u32, pub children: Vec<OutlineNode> }

pub fn parse_probe(v: &serde_json::Value) -> Vec<Heading> {
    v.get("headings").and_then(|h| h.as_array()).map(|arr| {
        arr.iter().filter_map(|h| {
            Some(Heading {
                level: h.get("level")?.as_u64()? as u8,
                text: h.get("text")?.as_str()?.to_string(),
                anchor: h.get("anchor").and_then(|a| a.as_str()).map(|s| s.to_string()),
                page: h.get("page").and_then(|p| p.as_u64()).unwrap_or(0) as u32,
            })
        }).collect()
    }).unwrap_or_default()
}

/// Nest a flat heading list into a tree by `level` (a deeper heading becomes a
/// child of the nearest preceding shallower one).
pub fn build_outline(headings: &[Heading]) -> Vec<OutlineNode> {
    let mut roots: Vec<OutlineNode> = Vec::new();
    // stack of (level, path-of-indices) — we descend by tracking the last node at each level
    let mut stack: Vec<(u8, *mut OutlineNode)> = Vec::new(); // not used; see safe impl below
    let _ = &mut stack;
    // Safe implementation via index paths:
    fn push(nodes: &mut Vec<OutlineNode>, levels: &mut Vec<u8>, h: &Heading) {
        levels.push(h.level);
        nodes.push(OutlineNode { title: h.text.clone(), page: h.page, children: Vec::new() });
    }
    // Recursive builder over a peekable iterator.
    fn build(it: &mut std::iter::Peekable<std::slice::Iter<Heading>>, parent_level: u8) -> Vec<OutlineNode> {
        let mut out = Vec::new();
        while let Some(h) = it.peek() {
            if h.level <= parent_level { break; }
            let cur = (*h).clone();
            it.next();
            let mut node = OutlineNode { title: cur.text, page: cur.page, children: Vec::new() };
            node.children = build(it, cur.level);
            out.push(node);
        }
        out
    }
    let mut it = headings.iter().peekable();
    roots = build(&mut it, 0);
    let _ = push; let _ = levels_unused();
    roots
}
fn levels_unused() {}
```
NOTE to implementer: the helper scaffolding (`push`, `stack`, `levels_unused`) is noise — DELETE it; keep only the recursive `build` + the `let mut it`/`roots` lines. Final `build_outline` body should be just the peekable recursion. Ensure `cargo clippy` is clean.

- [ ] **Step 4: Declare module + run test**

Add to `lib.rs`: `pub mod outline;`
Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core outline` → PASS (`parses_and_nests`, `probe_js_is_present`). Also `cargo clippy -p wkhtmltox-core` clean.

- [ ] **Step 5: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-core/src/outline.rs wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs
git commit -m "feat(core): outline model (probe JS + heading tree)"
```

---

### Task 2: QPDF multi-document merge (shim + safe wrapper)

**Files:**
- Modify: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.cpp`, `cpp/shim.h`, `src/lib.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/tests/merge.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/src/pdf.rs` (safe wrapper), modify `lib.rs`

**Interfaces:**
- Produces (C ABI): `int wkx_pdf_merge(const char** in_paths, int n, const char* out_path)` → 0 ok; concatenates pages of all inputs in order.
- Produces (Rust): `wkhtmltox_pdf_sys::wkx_pdf_merge(...)`; and safe `wkhtmltox_core::pdf::merge(inputs: &[PathBuf], out: &Path) -> Result<()>` (FFI confined to pdf-sys; core wrapper builds the C arrays).

  (Wait — core is `forbid(unsafe_code)`; it CANNOT call the raw FFI. Resolve: put the safe wrapper that touches FFI in `wkhtmltox-pdf-sys` as a safe `pub fn merge(...)`, and have core depend on `wkhtmltox-pdf-sys` for assembly. So: the safe `merge`/`add_outline`/`number_pages` wrappers live in `wkhtmltox-pdf-sys/src/lib.rs` (allowed `unsafe`), exposing a safe Rust API; `wkhtmltox-core` calls those.)

- [ ] **Step 1: Failing test** — `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/tests/merge.rs`:
```rust
// LGPL-3.0-or-later.
use std::path::PathBuf;
fn write_min_pdf(path: &str, pages: usize) {
    // minimal N-page PDF
    let mut objs = String::from("%PDF-1.4\n");
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3+i)).collect();
    objs.push_str("1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n");
    objs.push_str(&format!("2 0 obj<</Type/Pages/Kids[{}]/Count {}>>endobj\n", kids.join(" "), pages));
    for i in 0..pages { objs.push_str(&format!("{} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n", 3+i)); }
    objs.push_str("trailer<</Root 1 0 R>>\n%%EOF\n");
    std::fs::write(path, objs).unwrap();
}
#[test]
fn merges_pages_in_order() {
    let d = std::env::temp_dir();
    let a = d.join("wkx_m_a.pdf"); let b = d.join("wkx_m_b.pdf"); let o = d.join("wkx_m_out.pdf");
    write_min_pdf(a.to_str().unwrap(), 1);
    write_min_pdf(b.to_str().unwrap(), 2);
    let n = wkhtmltox_pdf_sys::merge(&[a.clone(), b.clone()], &o).expect("merge");
    let _ = n;
    let doc = lopdf::Document::load(&o).unwrap();
    assert_eq!(doc.get_pages().len(), 3, "1+2 pages");
}
```

- [ ] **Step 2: Run → fail** (`wkhtmltox_pdf_sys::merge` missing). `cargo test -p wkhtmltox-pdf-sys merge`.

- [ ] **Step 3: Shim** — append to `cpp/shim.cpp` (and declare in `shim.h`):
```cpp
#include <vector>
extern "C" int wkx_pdf_merge(const char** in_paths, int n, const char* out_path) {
    try {
        if (n <= 0) return 2;
        QPDF out; out.emptyPDF();
        for (int i = 0; i < n; ++i) {
            QPDF in; in.processFile(in_paths[i]);
            for (auto& page : QPDFPageDocumentHelper(in).getAllPages())
                QPDFPageDocumentHelper(out).addPage(page, false);
        }
        QPDFWriter w(out, out_path); w.write();
        return 0;
    } catch (const std::exception&) { return 1; } catch (...) { return 2; }
}
```
`shim.h`: add `int wkx_pdf_merge(const char** in_paths, int n, const char* out_path);`

- [ ] **Step 4: Rust FFI + safe wrapper** — in `pdf-sys/src/lib.rs` add the extern decl and a safe wrapper:
```rust
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
extern "C" { fn wkx_pdf_merge(in_paths: *const *const c_char, n: c_int, out: *const c_char) -> c_int; }

/// Safe wrapper: merge `inputs` (in order) into `out`. Confines all unsafe here.
pub fn merge(inputs: &[PathBuf], out: &Path) -> Result<(), String> {
    let cs: Vec<CString> = inputs.iter()
        .map(|p| CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<_,_>>()?;
    let ptrs: Vec<*const c_char> = cs.iter().map(|c| c.as_ptr()).collect();
    let co = CString::new(out.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let rc = unsafe { wkx_pdf_merge(ptrs.as_ptr(), ptrs.len() as c_int, co.as_ptr()) };
    if rc == 0 { Ok(()) } else { Err(format!("wkx_pdf_merge rc={rc}")) }
}
```

- [ ] **Step 5: Run test → PASS** `cargo test -p wkhtmltox-pdf-sys merge` (3 pages). If QPDF 12.x `addPage`/`getAllPages` differs, adapt minimally + note.

- [ ] **Step 6: Commit** `feat(pdf-sys): wkx_pdf_merge + safe merge() wrapper`.

---

### Task 3: QPDF outline/bookmarks insertion

**Files:** modify `cpp/shim.cpp`, `cpp/shim.h`, `pdf-sys/src/lib.rs`; create `pdf-sys/tests/outline.rs`.

**Interfaces:**
- C ABI: `int wkx_pdf_set_outline(const char* in, const char* out, const char* outline_json)` — `outline_json` is a flat array `[{title,page,level}]` (0-based page); builds a nested `/Outlines` tree.
- Rust safe: `wkhtmltox_pdf_sys::set_outline(in_path, out_path, &[(title,page,level)]) -> Result<()>` (serializes to JSON, calls FFI).

- [ ] **Step 1: Failing test** `pdf-sys/tests/outline.rs`: build a 3-page PDF (reuse the `write_min_pdf` helper — extract it to a shared `tests/common/mod.rs` so it is not duplicated), call `set_outline` with `[("A",0,1),("A.1",0,2),("B",2,1)]`, assert via lopdf that the catalog has `/Outlines` with the right top-level count (2) and first/last destination pages.

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Shim** — parse the JSON in C++ (use a tiny hand parser is risky; instead pass already-flattened arrays). SIMPLER C ABI to avoid a JSON parser in C++: `int wkx_pdf_set_outline(const char* in,const char* out,const char** titles,const int* pages,const int* levels,int n)`. Build `/Outlines` with `/First /Last /Count` and per-item `/Title /Dest [pageobj /XYZ null null null] /Parent /Next /Prev`, nesting by `levels` with a stack. Wrap in catch backstops. (Reference: PDF 32000 §12.3.3.)

- [ ] **Step 4: Rust safe wrapper** `set_outline(in,out,&[(String,u32,u8)])` building the three parallel C arrays.

- [ ] **Step 5: Run → PASS** (lopdf: `/Outlines` present, top-level count 2, A.1 nested under A, B → page 3). Adapt to QPDF 12.x as needed.

- [ ] **Step 6: Commit** `feat(pdf-sys): wkx_pdf_set_outline + safe set_outline() (nested bookmarks)`.

---

### Task 4: Page-number footer overlay

**Files:** modify `cpp/shim.cpp`, `cpp/shim.h`, `pdf-sys/src/lib.rs`; create `pdf-sys/tests/numbering.rs`.

**Interfaces:**
- C ABI: `int wkx_pdf_stamp_footer(const char* in,const char* out,const char* fmt,int start)` — stamps a centered footer on each page; `fmt` supports `[page]` and `[topage]` tokens (substituted in C++), `start` = first page number.
- Rust safe: `wkhtmltox_pdf_sys::stamp_footer(in,out,fmt,start) -> Result<()>`.

- [ ] **Step 1: Failing test** `numbering.rs`: 3-page PDF, `stamp_footer(.., "[page] / [topage]", 1)`, then assert via lopdf `extract_text`/content contains "1 / 3" and "3 / 3" (or that each page's content stream grew / has a Tj with the expected string). Keep the assertion robust (check the content stream bytes contain the page strings).

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Shim** — for each page, build a small content stream `BT /Helv 9 Tf <x> 20 Td (<text>) Tj ET` with the substituted text (escape `()\\`), ensure a `/Helv` font resource exists on the page (add to `/Resources/Font`), and append the stream to the page contents (QPDFPageObjectHelper::addPageContents / append). catch backstops.

- [ ] **Step 4: Rust safe wrapper.**

- [ ] **Step 5: Run → PASS.** Adapt to QPDF 12.x.

- [ ] **Step 6: Commit** `feat(pdf-sys): wkx_pdf_stamp_footer + safe stamp_footer() (page numbers)`.

---

### Task 5: Converter multi-document assembly (wire it together)

**Files:** create `wkhtmltox-rs/crates/wkhtmltox-core/src/assembly.rs`; modify `lib.rs`; create `wkhtmltox-rs/crates/wkhtmltox-core/tests/assembly_mock.rs`.

**Interfaces:**
- Consumes: `Renderer` (MockRenderer in tests), `outline::{PROBE_JS,parse_probe,build_outline}`, `wkhtmltox_pdf_sys::{merge,set_outline,stamp_footer}`.
- Produces: `pub fn assemble_pdf(r: &mut dyn Renderer, objects: &[Source], geom: &PageGeometry, out: &Path, number: bool) -> Result<AssemblyReport>` where `AssemblyReport{ pages: u32, objects: usize }`. For each object: open → wait_ready → eval_json(PROBE_JS) → print_pdf → write temp; then merge all temps → (optional) set_outline from combined headings with page offsets → (optional) stamp_footer → write `out`.

- [ ] **Step 1: Failing test** `assembly_mock.rs`: a `MockRenderer` whose `print_pdf` returns a fixed 2-page minimal PDF and whose `eval_json` returns a 1-heading probe; run `assemble_pdf` over 2 objects with `number=true`; assert the output has 4 pages (2×2) and (via lopdf) an `/Outlines` entry. (MockRenderer may need to return a *valid* multi-page PDF — extend the test's mock or use the shared `write_min_pdf` bytes.)

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Implement `assemble_pdf`** — orchestrate render→temp-files→merge→outline(offset per object's page count)→footer→write. Use `tempfile`-style unique paths (std::env::temp_dir + a per-call counter; avoid fixed names). Compute per-object page counts (from lopdf? no — core can't dev-dep lopdf in non-test). Get page count from the merge step: have `wkx_pdf_merge` (or a `wkx_pdf_page_count`) return counts. ADD a `wkhtmltox_pdf_sys::page_count(path)->Result<u32>` (tiny shim `wkx_pdf_page_count`) so core can compute outline page offsets without lopdf. (Add that shim function here if not already present.)

- [ ] **Step 4: Run → PASS** (4 pages, outline present), `cargo test -p wkhtmltox-core assembly` + `cargo clippy` clean.

- [ ] **Step 5: Commit** `feat(core): assemble_pdf — multi-doc merge + outline + page numbers`.

---

### Task 6: End-to-end harness validation (real Chromium + oracle)

**Files:** modify `wkhtmltox-rs/crates/wkhtmltox-render-chromium/examples/render.rs` (or a new `examples/assemble.rs`) to expose multi-input assembly; extend `tests/compat/compare.py` with a 2-document merge case.

- [ ] **Step 1:** Add `examples/assemble.rs`: takes `out.pdf in1.html in2.html ...`, uses `ChromiumRenderer` + `assemble_pdf` (number=true). Build it.
- [ ] **Step 2:** Add a compat case: assemble two corpus docs (e.g. `text.html` + `headings.html`) via the new example, and via the oracle (`wkhtmltopdf ... in1 in2 out` — wkhtmltopdf natively merges multiple inputs). Compare page count + outline tree structurally.
- [ ] **Step 3:** Run it (gated/real Chrome); record results to `tests/compat/results-assembly.md`. Confirm our merged outline tree matches the oracle's structurally.
- [ ] **Step 4: Commit** `test(compat): 2-document assembly vs oracle`.

---

## Self-Review

**Spec coverage (spec §5 assembly portion):** probe/outline (§5.1, Task 1) ✓; merge (⑦, Task 2) ✓; bookmarks (④/⑪, Task 3) ✓; page numbering (⑨, Task 4) ✓; converter orchestration (②→⑦, Task 5) ✓; oracle validation (§8, Task 6) ✓. **Deferred to Milestone 2b (noted, not in this plan):** TOC + libxslt + fixed-point loop (§5.2), variable headers/footers HTML rendering + substitution (§5.3), cover page, forms→AcroForm wiring (the shim exists from M1), clickable-link synthesis (⑩). These need the TOC/XSLT and header-rendering machinery and get their own plan.

**Placeholder scan:** Task 1's implementation block contains scaffolding explicitly marked for deletion — the implementer must remove it and ship only the recursive `build`. No other placeholders; every shim function has concrete code + a "verify against QPDF 12.x" gate (the M1 AcroForm task validated this approach).

**Type consistency:** `merge/set_outline/stamp_footer/page_count` live in `wkhtmltox-pdf-sys` (safe wrappers over the `unsafe extern` block) and are consumed by `wkhtmltox-core::assembly` (keeping core `forbid(unsafe_code)`). `OutlineNode`/`Heading` defined in Task 1 are consumed in Task 5. The shared `write_min_pdf` test helper is extracted to `tests/common/mod.rs` (Task 3) to avoid duplication flagged by review.

**Security note:** new C++ shim functions handle attacker-irrelevant inputs today (paths from the converter; titles/text from the rendered DOM). All must keep the `catch(...)` backstop and bounds-check `n`/indices. The post-feature security hook will audit on the `feat:` commits.
