# wkhtmltox-rs Milestone 3 — libwkhtmltox C ABI (PDF)

> SDD execution. Design basis: spec §4 (registry as compat linchpin), §6.2 (C-ABI fidelity, string-lifetime fix, catch_unwind). Builds on the M2 assembly core + ChromiumRenderer. Scope: the **PDF** C ABI (`wkhtmltopdf_*`); the image ABI is M5.

**Goal:** A `libwkhtmltox` cdylib exposing the wkhtmltopdf C ABI verbatim, so existing C consumers link and run unchanged, routing through the settings registry → assembly core → Chromium.

## Global Constraints
- Rust 2021; no Qt. `wkhtmltox-core`/`-render-chromium` stay `#![forbid(unsafe_code)]`; `unsafe`/FFI only in `wkhtmltox-pdf-sys` and the new `wkhtmltox-capi`. Every `extern "C"` export wraps its body in `std::panic::catch_unwind` (no panic crosses FFI) and returns a safe default on panic.
- `const char*` returns borrow from per-converter owned storage (a `HashMap<_,CString>`), never a dropped temporary (the designed-out bug). LGPL headers; `cargo clippy -D warnings` clean. Commit per task + trailer.
- Vendored `pdf.h` must byte-match the upstream `src/lib/pdf.h` (a header-diff test enforces it).

### Task 1 — Settings registry
- `wkhtmltox-core/src/registry.rs`: a table mapping wkhtmltopdf setting NAMES → typed setters on `GlobalSettings`/`PdfObjectSettings`. `pub fn set_global(g:&mut GlobalSettings, name:&str, value:&str)->Result<()>` and `set_object(...)`. Expand the settings structs to cover the engine-supported surface: page size (`size.pageSize`/`size.width`/`size.height`), margins (`margin.top/bottom/left/right`), `orientation`, `web.printMediaType`, `web.enableJavascript`, `load.jsdelay`, `header.*`/`footer.*` (left/center/right/fontSize/line), `toc`, `outline`, `colorMode`, etc. Recognized-but-unimplemented names → accept + warn (collect a warning). Unknown names → `Err(BadArg)`. Names/defaults referenced from upstream `src/lib/pdfsettings.cc` reflect tables.
- Map these settings into `PageGeometry` + `AssembleOpts` (a `to_geometry()`/`to_assemble_opts()` on the settings).
- Tests: round-trip a representative set of names (size/margins/orientation/jsdelay/header.center/toc); unknown name errors; unimplemented name warns.
- Commit `feat(core): settings registry (wkhtmltopdf names → typed fields)`.

### Task 2 — Vendor headers + capi skeleton
- Copy upstream `src/lib/pdf.h` → `wkhtmltox-capi/include/pdf.h` VERBATIM (also `src/lib/image.h` for completeness, used in M5). Create `wkhtmltox-capi` crate: `crate-type=["cdylib","staticlib"]`, depends on core + render-chromium + pdf-sys.
- Header-diff test (a Rust test or `tests/header_diff.rs`) asserting `include/pdf.h` is byte-identical to `../../../src/lib/pdf.h` (the repo's upstream header) — or, if intentional deltas are needed, a documented allowlist.
- Commit `feat(capi): vendor pdf.h verbatim + header-diff test + crate skeleton`.

### Task 3 — C ABI exports (PDF)
- Implement in `wkhtmltox-capi/src/lib.rs` the `wkhtmltopdf_*` surface to match `pdf.h`:
  - `wkhtmltopdf_init/deinit`, `wkhtmltopdf_version`.
  - global/object settings: `create_global_settings`, `set_global_setting`, `create_object_settings`, `set_object_setting`, `get_global_setting`/`get_object_setting` (string-cache returns), destroy.
  - converter: `create_converter(gs)`, `add_object(c, os, const char* data)` (data=HTML or NULL→use object's `page` URL), `convert(c)` → 1/0, `destroy_converter`, `http_error_code`.
  - callbacks: `set_progress_changed_callback`/`phase_changed`/`error`/`warning`/`finished`; `current_phase`/`phase_count`/`phase_description`/`progress_string`.
  - `get_output(c, const unsigned char**)` → output PDF bytes.
  - Each export: `catch_unwind` body; opaque pointers are `Box::into_raw`/`from_raw`; `const char*`/byte returns borrow per-converter owned `CString`/`Vec<u8>`.
  - `convert` wires: build `Vec<Source>` from added objects, map settings → geometry/opts via the registry, spawn `ChromiumRenderer`, run `assemble_pdf`, store output bytes; drive the callbacks (phase names matching wkhtmltopdf: "Loading pages"…"Done").
- Commit `feat(capi): wkhtmltopdf_* C ABI exports (settings/converter/callbacks/convert)`.

### Task 4 — C consumer test under ASan
- `wkhtmltox-capi/tests/` or a `ctest/` dir: a small C program that `#include "pdf.h"`, does init → settings → object (a small inline HTML) → converter → set callbacks → convert → get_output → deinit, and checks the output starts with `%PDF` and a callback fired. Build it against the cdylib, run under ASan/leak detection (gated by Chrome availability). Assert the `const char*` lifetime contract (hold `progress_string` across calls).
- Commit `test(capi): C consumer under ASan + lifetime check`.

### Task 5 — Milestone gate
- Validate the C-ABI output equals the example/assemble path on a sample (same page count/outline). Then whole-milestone Opus review + security audit + fix wave + push.

## Self-Review
Covers spec §4 (registry) + §6.2 (C-ABI fidelity, string cache, catch_unwind). Scope = PDF ABI; image ABI deferred to M5. Settings surface = engine-supported subset + accept/warn for the rest (full parity tracked via M3b CLI + later). Security: catch_unwind + owned-string returns are the key safety properties; the C consumer test under ASan is the gate.
