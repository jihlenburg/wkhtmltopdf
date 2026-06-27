# Design: A Qt-free, drop-in HTML→PDF/Image engine ("wkhtmltox-rs")

- **Date:** 2026-06-28
- **Status:** Design — **engine decision finalized via spikes** (SPIKE 1 GO, SPIKE 2 NO-GO); ready for Milestone 1 planning.
- **Topic:** Replace wkhtmltopdf/wkhtmltoimage's Qt/QtWebKit core with a Qt-free engine that reproduces its features, while remaining a drop-in replacement at both the CLI and C-ABI levels.
- **Repo context:** This document lives in the macOS-arm64 fork of wkhtmltopdf. The existing product is built on a *vendored, patched Qt 4.8.7 / QtWebKit* (the `qt/` submodule). This design describes a *new* engine, not a modification of the current one.

> **Engine decision (changed from the initial draft):** the renderer is a **single out-of-process headless Chromium driven over the DevTools Protocol (CDP)**, not per-platform native WebKit. This change was forced by a devil's-advocate review plus two reproducible spikes (see §11). The original per-platform-WebKit choice had a measured, disqualifying failure mode.

---

## 1. Goal & motivation

Build a new engine that offers the **same features as wkhtmltopdf** (HTML→PDF and HTML→image, including the hard differentiators: table of contents, PDF outlines/bookmarks, multi-document merge, cover pages, variable headers/footers, forms) but with **no dependency on Qt at all** — including QtCore.

### Why a new engine (validated by background research)

Migrating the existing code to a modern Qt is a dead end:

- QtWebKit was deprecated in Qt 5.5 and **removed in Qt 5.6 (2016)**; Qt 6 ships no official QtWebKit. The community QtWebKit fork is **Qt5-only and abandoned since 2020**.
- The current product's value lives in *patches to WebKit itself* (the `QWebPrinter` class, gated by `__EXTENSIVE_WKHTMLTOPDF_QT_HACK__`). No modern engine offers those patches, so "same features" must be **reconstructed as a document layer on top of a reused engine**.

### Why out-of-process headless Chromium (and not QtWebEngine, and not native WebKit)

The earlier objections to "Chromium" actually applied to **embedding `QtWebEngine` as a library** (cannot be statically linked, Chromium-sized payload, async DOM removing synchronous `QWebElement` access). Those objections **do not apply** to **driving a headless Chromium process out-of-process over CDP**:

- The "async DOM" objection is handled by the `Renderer` trait's blocking-over-async pump (§4) — exactly the pattern the SPIKE 1 driver used.
- Chromium now generates a **PDF document outline natively** (`Page.printToPDF { generateDocumentOutline: true }` / `--generate-pdf-document-outline`), and **emits page-accurate named destinations** for in-document anchors (verified, §11).
- One engine renders **identically on every OS** and is **genuinely headless** on Linux servers/CI, Windows, and macOS — restoring wkhtmltopdf's defining cross-platform-consistency property that per-platform WebKit would have destroyed.
- Native WebKit was rejected after SPIKE 2 (§11): `WKWebView.createPDF` produces a single content-sized page (no `@page` pagination, no outline, no destinations); the paginated alternative (`NSPrintOperation`) is GUI/WindowServer-bound and unverified. WPE (the would-be Linux backend) has no print-to-PDF API at all.

---

## 2. Locked decisions

| Decision | Choice | Rationale |
|---|---|---|
| Engine strategy | **Reuse a mature engine** (don't build a layout engine) | "Same features" implies a full browser (HTML5+CSS+JS); building that is person-decades. |
| Engine | **Single out-of-process headless Chromium over CDP** (managed subprocess) | One engine, identical output everywhere, genuinely headless on all platforms incl. Windows; native outline + page-accurate destinations (§11). |
| Compatibility | **Drop-in CLI _and_ preserve the `libwkhtmltox` C ABI** (`pdf.h`, `image.h`) | Existing scripts, CI, wrappers, and lib consumers keep working. |
| Qt | **None at all**, including QtCore | Hard requirement. |
| Language | **Rust** core; FFI only to QPDF/libxslt | Greenfield removes C++'s reuse advantage; Rust eliminates the C-ABI string-lifetime bug class. The Chromium driver is pure-Rust (subprocess + websocket + JSON), needing **no** FFI. |
| v1 scope | **Full PDF + image parity, all three desktop OSes (Linux, Windows, macOS incl. arm64)** | The C ABI includes `wkhtmltoimage_*`; one engine makes Windows free on day one. |
| Chromium integration | **Bundle `chrome-headless-shell`** and drive it as a managed subprocess; system-Chrome path as a config fallback | Self-contained, offline, uniform; matches the production-proven Gotenberg pattern. |
| Implementation posture | **Greenfield + compatibility shim**, built **test-first** | Cleanest long-term code; drift risk mitigated by porting upstream flag tables verbatim + a diff-against-reference harness. |

### Non-goals (v1)

- Native-WebKit backend (dropped from v1; revisit only as a future research spike via `NSPrintOperation` if a "max-fidelity macOS" backend is ever wanted — `createPDF` is a dead end).
- A single static self-contained binary (the engine is a separate Chromium process; the deliverable bundles `chrome-headless-shell`, ~150–200 MB; documented).
- Modifying or shipping the legacy Qt-based code.

---

## 3. Architecture & workspace layout

A Cargo workspace that keeps a large safe-Rust core and confines `unsafe`/FFI to **two** edges (down from four — the Chromium driver is FFI-free).

```
wkhtmltox-rs/                         (Cargo workspace)
├── crates/
│   ├── wkhtmltox-core/      ← SAFE Rust engine; knows nothing about Chromium
│   │   ├── settings/        ← Global/Object/Image settings as typed value structs
│   │   ├── registry/        ← string-key ⇄ field table (THE compat linchpin)
│   │   ├── converter/       ← orchestration: phases, progress, multi-doc
│   │   ├── render/          ← the `Renderer` trait (the seam) + DTOs
│   │   ├── outline/  toc/   ← outline & TOC model (libxslt FFI for --xsl-style-sheet)
│   │   ├── pdf/             ← assembly: merge, bookmarks, header/footer overlay,
│   │   │                       pagination, cover, forms→AcroForm, synth link annots
│   │   └── image/           ← screenshot → crop → scale → encode
│   ├── wkhtmltox-render-chromium/   ← Renderer impl over CDP (PURE RUST):
│   │                                   subprocess mgmt + websocket + serde_json
│   ├── wkhtmltox-pdf-sys/           ← bindgen FFI: QPDF (C API) + libxslt   (unsafe edge #1)
│   ├── wkhtmltox-capi/             ← cdylib/staticlib: exports wkhtmltopdf_*/wkhtmltoimage_*
│   │   └── include/{pdf.h,image.h}    (vendored VERBATIM; header-diff test)   (unsafe edge #2)
│   ├── wkhtmltopdf-cli/   wkhtmltoimage-cli/   ← the two executables
├── tests/{compat,capi}/   ← golden/structural + diff-vs-real-wkhtmltopdf; C consumer linking the lib
└── xtask/                 ← packaging, header-sync check, stage chrome-headless-shell
```

### Principles

- **The `unsafe` surface is exactly two edges:** inbound C ABI (`wkhtmltox-capi`) and QPDF/libxslt (`wkhtmltox-pdf-sys`). The Chromium backend is safe Rust talking CDP over a pipe/websocket.
- **The `registry` table makes "drop-in" true.** The CLI parser *and* the C ABI's `set_*_setting(name,value)` resolve names through one table ported verbatim from upstream's `reflect` data.
- **Adding/replacing a backend is one crate.** The core depends on the `Renderer` *trait*; the Chromium backend is just the first (and only v1) implementation.
- **Three-layer separation:** CLI/C-ABI → settings (via registry) → converter (owns `Box<dyn Renderer>`).

---

## 4. The `Renderer` trait & core components

### The seam

```rust
/// Implemented by the Chromium/CDP backend (and, hypothetically, others).
/// Methods are *blocking* from the core's view — the backend internally drives
/// the async CDP message loop (a single-threaded runtime block_on per round-trip).
pub trait Renderer {
    fn open(&mut self, src: &Source, load: &LoadSettings) -> Result<PageHandle>;

    /// Block until "ready": load finished, then honor javascript_delay
    /// and/or a window.status target — wkhtmltopdf's readiness rules.
    fn wait_ready(&mut self, p: &PageHandle, r: &ReadyPolicy) -> Result<()>;

    /// THE DOM primitive. Run JS, get parsed JSON back (CDP Runtime.evaluate).
    /// Replaces every synchronous QWebElement/QWebFrame call.
    fn eval_json(&mut self, p: &PageHandle, script: &str) -> Result<serde_json::Value>;

    /// Render one object's body to a standalone PDF (CDP Page.printToPDF),
    /// honoring geometry; generateDocumentOutline=true.
    fn print_pdf(&mut self, p: &PageHandle, g: &PageGeometry) -> Result<Vec<u8>>;

    /// Capture full page or a crop region (CDP Page.captureScreenshot{clip}).
    fn snapshot(&mut self, p: &PageHandle, o: &SnapshotOpts) -> Result<RawImage>;

    fn page_info(&self, p: &PageHandle) -> Result<PageInfo>; // title, final URL, content h
}
```

Supporting DTOs: `Source { Url | Html | Stdin }`, `LoadSettings` (cookies, custom headers, proxy, auth, JS on/off, local-file policy, user stylesheet, …), `ReadyPolicy`, `PageGeometry`, `SnapshotOpts`, `PageInfo`.

### The Chromium backend

A managed `chrome-headless-shell` subprocess driven over CDP. `open` = `Target.createTarget` + `Page.navigate`; `wait_ready` = await `Page.loadEventFired` then apply delay/`Runtime.evaluate` on `window.status`; `eval_json` = `Runtime.evaluate { returnByValue }`; `print_pdf` = `Page.printToPDF { generateDocumentOutline, preferCSSPageSize, margins, printBackground }`; `snapshot` = `Page.captureScreenshot { clip }`. One process can serve multiple sequential objects.

### The injected JS "probe"

A single script returns everything the document layer needs as JSON (title, headings, anchors, links, form fields, content height, header/footer variables), so all outline/TOC/anchor/form logic lives in safe Rust and is testable without a browser.

### Core components

| Component | Responsibility | Key interface |
|---|---|---|
| `settings` | Typed value structs mirroring wkhtmltopdf defaults | `GlobalSettings`, `PdfObjectSettings`, `ImageSettings` |
| `registry` | Name→field table; CLI **and** C ABI write through it | `set(target, name, value) -> Result<()>` |
| `converter` | Phase machine, multi-doc loop, emits callbacks | `Converter::run()`; `Progress/Phase/Warning/Error/Finished` |
| `outline`/`toc` | Build outline tree from probe JSON + outline source; TOC via libxslt | `Outline::from_probe(..)`; `Toc::render(xsl, outline)` |
| `pdf` | Assemble final document (QPDF + pdf-writer) | `merge / add_bookmarks / overlay_headers / number_pages / insert_toc / make_acroform / synth_links / write` |
| `image` | screenshot → crop → scale → encode | `ImagePipeline::produce(..)` |

---

## 5. The PDF pipeline

```
 ① settings → object list (cover? toc? + N content objects, each w/ per-obj opts)
 ② FOR EACH content object:
       open(load) → wait_ready(delay/window-status) → eval_json(PROBE)
       → print_pdf(geom, generateDocumentOutline)  ⇒ {pdf_bytes, page_count, outline, dests, probe}
 ③ resolve anchor→page map      ← from engine outline + named destinations
 ④ build outline/bookmark tree from headings across objects
 ⑤ if toc: outline-XML → libxslt(XSL) → TOC HTML → render as its OWN object
            ⇒ inserting it shifts later pages → FIXED-POINT loop
 ⑥ if cover: render as own object (no header/footer/number)
 ⑦ assemble: QPDF merge  [cover][toc][obj1..N]
 ⑧ overlay headers/footers per page + variable substitution
 ⑨ number pages (continuous | per-object reset | --page-offset)
 ⑩ synthesize clickable GOTO link annotations from destinations (covers Chromium bug 347674894)
 ⑪ if --enable-forms: probe.forms → AcroForm widgets
 ⑫ bookmarks + metadata + (linearize / compress / encrypt) + engine/version watermark in metadata
 ⑬ write(file | stdout) → Finished
```

### 5.1 Anchor → page-number mapping (the crux — now verified, §11)

Chromium provides the page mapping directly, from **two** correct sources (both survive CSS fragmentation — verified in SPIKE 1):

- **Primary — `generateDocumentOutline`.** A complete heading→page bookmark tree built from document structure; covers *every* heading (even ones nothing links to), with correct post-fragmentation page numbers. This is the primary TOC/bookmark source.
- **Secondary — named destinations.** Chromium emits page-accurate named destinations for `id`'d anchors that are link targets. For arbitrary (non-heading) anchors, ensure completeness with the planned pre-print JS injection (make each target an `id`'d, linked element).
- **Known gap, recovered in post-processing:** Chromium does **not** emit clickable internal *link annotations* (bug 347674894). Since we hold every destination's page+position, phase ⑩ **synthesizes** the `/Link` GOTO annotations ourselves (QPDF/pdf-writer). Missing clickable links ≠ wrong page numbers.
- **Fallback (gap-fill only, never the delivered source):** `floor(top / content_height_per_page)` from probe measurement, for anchors with no destination; clamped monotonic against known neighbours.

### 5.2 TOC fixed-point loop

The TOC is itself rendered HTML; its page count is unknown until rendered and inserting it renumbers later pages. Build outline with placeholder numbers → render TOC → learn `toc_page_count` → recompute global numbers → re-render → **iterate until `toc_page_count` is stable** (typically 1–2 passes), capped with a logged fallback.

### 5.3 Variable headers/footers (overlay, not engine-native)

- **Text headers** (`--header-left/center/right`): per page, substitute `[page]/[topage]/[section]/[title]/[date]/…`; `[section]` from the active outline entry. Drawn into the margin with `pdf-writer`.
- **HTML headers** (`--header-html URL`): rendered via the `Renderer` with variables as query params, stamped into the margin; **cached by resolved-variable-set**.
- **Margin feedback:** header/footer height + `--header-spacing` is added to the effective margin **before** `print_pdf` (phase ②).

### 5.4 Image pipeline (wkhtmltoimage)

`open → wait_ready → [crop] → Page.captureScreenshot(scale/clip) → resize(--width/--height) → encode(png|jpg, --quality, --transparent) → write`. Reuses the same `Renderer`.

---

## 6. Error handling, ABI/CLI fidelity & security

### 6.1 Internal error model

`thiserror` enum `WkError { Load{url,http_status}, Render, Pagination, Pdf, Xslt, Io, BadArg, Engine, Security }` as `Result<T, WkError>`. Warnings accumulate; errors abort the phase. `--load-error-handling`/`--load-media-error-handling`: `abort` / `ignore` / `skip` faithfully.

### 6.2 C-ABI & CLI contract fidelity

| Contract surface | How we match it |
|---|---|
| Callbacks (`error`/`warning`/`phase_changed`/`progress_changed`/`finished`) | Signatures from the vendored header; wired to core hooks |
| Phase names & counts | Core emits the same names ("Loading pages", "Resolving links", "Printing pages", …) |
| `convert()` → `1`/`0`, `http_error_code()` | Mapped from `WkError`/last HTTP status |
| `const char*` returns | Owned `HashMap<_, CString>` in the converter; pointer borrows owned storage |

- **String-lifetime bug designed out** (the audit's high-severity image-bindings finding): the converter owns the storage; the returned pointer borrows it — the bug class does not compile, on both pdf and image.
- **Panics never cross FFI:** every `extern "C"` export wraps its body in `catch_unwind` → `error` callback + `0` return.
- **Exit codes are derived, not remembered:** the compat harness runs the reference binary across a matrix and we replicate.

### 6.3 Security model (the logic bugs Rust does not fix)

wkhtmltopdf's CVEs were **SSRF and local-file disclosure** (`file://`, http→file:// redirects). Enforcement lives in **one place**: a `ResourcePolicy` in the safe core, applied via **CDP request interception** (`Fetch.enable` + `Fetch.requestPaused`) on the Chromium backend — so the logic is engine-independent and testable without a browser.

- **Scheme allowlist** — block `file://` unless `--enable-local-file-access`; honor `--allow <path>`.
- **Redirect re-validation** — every redirect target re-checked (the redirect-to-`file://` hole).
- **Link/JS toggles** — `--disable-javascript`, `--disable-external-links`, `--disable-internal-links`.
- **SSRF hardening (new, opt-in)** — block private/link-local ranges.
- **Compatibility-vs-security tension:** match wkhtmltopdf's permissive defaults, ship faithful `--disable-local-file-access`/`--allow`, and add an opt-in **`--safe` profile** (deny `file://` + SSRF blocking). Compatibility by default; security one flag away; the choice explicit.

---

## 7. Dependencies & licensing

| Need | Choice | License |
|---|---|---|
| Rendering engine | **`chrome-headless-shell`** (bundled), driven over CDP (no FFI) | BSD |
| PDF structural ops (merge, overlay, outline, link annots, linearize, encrypt) | **QPDF** (`-pdf-sys`, C API) | Apache-2.0 |
| Generated content pages (TOC, cover, header/footer overlays) | **pdf-writer** (Rust) | MIT/Apache-2.0 |
| XSLT for `--xsl-style-sheet` TOC | **libxml2/libxslt** (FFI) | MIT |
| CDP transport / JS-bridge JSON | **serde_json** (+ a minimal websocket client) | MIT/Apache-2.0 |
| Image encoding | **image** + `png`/`mozjpeg` | MIT / BSD-like |
| Build | Cargo + `xtask` (stage headless-shell) | — |

`cargo-deny` enforces license + advisory policy. **AcroForm generation (forms→interactive widgets via QPDF) is now the top remaining High risk** → prototype in Milestone 1.

---

## 8. Testing & compatibility strategy

1. **Core unit tests (browser-free):** a `MockRenderer` returning canned probe JSON + PDF bytes exercises registry round-trips, pagination, the TOC fixed-point loop, bookmark tree, header/footer substitution, `ResourcePolicy`, link-annotation synthesis, and `pdf` ops on synthetic PDFs.
2. **C-ABI consumer test:** a C program compiled against the vendored headers, run under ASan/Valgrind for the `const char*` lifetime contract; plus a header-diff test.
3. **Golden/structural corpus:** assert page count, outline tree, TOC entries+pages, AcroForm fields, normalized per-page text, link destinations, metadata. Since there is now **one engine**, add **pixel/visual diffing** against real legacy documents as an additional acceptance bar (§11 SPIKE 5).
4. **Compatibility harness (diff vs real `wkhtmltopdf`):** same CLI args through both; compare structural output + exit codes + stderr categories; diff `--extended-help`/`--manpage` (the defense against flag drift).
5. **CI matrix + packaging:** Linux (x86_64+arm64) **headless Chromium container**, Windows, and macOS (arm64+x86_64) — all genuinely headless. `clippy`, `fmt`, `cargo-deny`, `cargo-fuzz` (opt-in) on the CLI grammar + probe-JSON + PDF FFI. Packaging bundles `chrome-headless-shell` per platform; the C lib ships with vendored headers.

**Build test-first:** the `MockRenderer` lets the entire document layer be written and verified before the Chromium backend exists.

---

## 9. Risks & open questions

| Risk | Severity | Mitigation |
|---|---|---|
| AcroForm/forms generation has weak Rust-native support | **High** | Prototype in Milestone 1 (QPDF FFI + dict work); gates the v1-forms claim |
| Behavioral drift from "drop-in" | High | Port upstream flag tables verbatim; flag-surface diff in CI; compat harness |
| Chromium clickable-link annotations missing (bug 347674894) | Medium | **Verified**; synthesize annotations from destinations (phase ⑩) |
| Chromium runtime dependency + distribution size (~150–200 MB) | Medium | Bundle `chrome-headless-shell`; document; offer system-Chrome config |
| Chromium version drift changing output | Medium | Pin the bundled headless-shell version; engine+version stamped in PDF metadata |
| AcroForm + variable HTML headers fidelity on engine output | Medium | Overlay-based design (engine-independent); covered by structural + visual tests |

**Open questions for review:**
1. Confirm **bundle `chrome-headless-shell`** as default vs. system-Chrome-only (affects size vs. convenience).
2. SVG image output (legacy could emit SVG via QtSvg) — v1 or deferred? (Raster PNG/JPG assumed.)
3. `--safe` hardened profile in v1, or deferred?

---

## 10. Milestones (high level — Milestone 1 to be detailed via writing-plans)

1. **Skeleton + spike the top risk:** workspace; the **AcroForm spike** (QPDF interactive widgets on engine PDF output); minimal Chromium-CDP `Renderer` (`open`/`eval_json`/`print_pdf`) reusing the SPIKE 1 driver.
2. **Core, test-first against `MockRenderer`:** settings, registry (ported tables), converter phases, outline, pagination (outline+destinations), TOC fixed-point, header/footer overlay, PDF assembly, link-annotation synthesis, ResourcePolicy.
3. **C ABI + CLI shims:** vendored headers, `extern "C"` + string cache, hand-rolled CLI grammar, help/manpage generation.
4. **Chromium backend hardening:** full `Renderer` (networking/cookies/proxy/auth via CDP, request interception for ResourcePolicy), image snapshots.
5. **Compatibility hardening:** diff-vs-reference harness, exit-code derivation, golden + visual corpus, forms.
6. **Packaging & docs** across Linux/Windows/macOS (bundle headless-shell).

---

## 11. Spike results (engine decision evidence)

Run 2026-06-28 on macOS arm64; Google Chrome 149; verification with PyMuPDF cross-checked against actual rendered text page.

**SPIKE 1 — headless Chromium over CDP: GO.** Test HTML with forced page breaks, a `break-inside:avoid` block, and multi-page sections rendered as 7× A4 pages.
- `Page.printToPDF{generateDocumentOutline:true}` produced a **correct bookmark tree** — entries at pages 1,1,3,3,5,6, **matching ground truth exactly** across all fragmentation.
- **Named destinations** for `id`'d anchors were page-accurate (sec1→1, sec2→3, sec3→5, sec4→6, deep→7), present **with and without** the outline flag.
- **Gap:** no clickable internal `/Link` GOTO annotations (Chromium bug 347674894) — recoverable via phase ⑩ since destinations exist.

**SPIKE 2 — `WKWebView.createPDF`: NO-GO for v1.** Same HTML →
- **1 page, 794×5498 pts** (single content-sized page; **no `@page` pagination**); all sections on one page.
- **No outline, no named destinations, no internal links.**
- Conclusion: `createPDF` is a content capture, not a paginating print; the paginated path (`NSPrintOperation`) is GUI/WindowServer-bound and unverified; WPE has no print-to-PDF. Native WebKit dropped from v1.

**Pending spikes (Milestone 1):** AcroForm-via-QPDF (top High risk); broader pagination corpus (widows/orphans, large splitting tables, multi-doc, RTL/CJK); single-engine visual/pixel diff vs legacy documents.
