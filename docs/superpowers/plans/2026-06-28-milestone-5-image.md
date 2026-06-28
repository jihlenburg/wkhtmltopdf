# wkhtmltox-rs Milestone 5 — Image pipeline (wkhtmltoimage)

> SDD execution. Adds HTML→image: the `Renderer::snapshot` backend, a core image pipeline, the `wkhtmltoimage_*` C ABI (image.h already vendored), and the `wkhtmltoimage` CLI. Reuses the Chromium renderer + registry pattern from M2–M4.

## Global Constraints
- Rust 2021; no Qt. `wkhtmltox-core`/`-render-chromium` stay `#![forbid(unsafe_code)]`; FFI only in pdf-sys + capi. catch_unwind on every C export; `const char*`/byte returns borrow per-converter owned storage. LGPL headers; `cargo clippy -D warnings` clean. Commit per task + trailer. Security: same ResourcePolicy applies (image render fetches subresources too).

### Task 1 — snapshot backend + core ImagePipeline
- Implement `ChromiumRenderer::snapshot(page, &SnapshotOpts) -> RawImage` via CDP `Page.captureScreenshot { format: png|jpeg, quality(jpeg), clip: {x,y,width,height,scale} (when cropping), captureBeyondViewport:true (full page), optionalFromSurface }`. Honor the existing Fetch/policy pump during load (reuse the M4 path). Return the encoded bytes + format.
- `wkhtmltox-core/src/image.rs`: add `image` crate to core deps. `pub fn produce(raw: RawImage, opts: &ImageOpts) -> Result<Vec<u8>>` — decode the screenshot, apply `--width`/`--height` resize (preserve aspect unless both given), optional crop (if not done via CDP clip), `--transparent` (PNG alpha), re-encode to PNG or JPEG at `--quality`. `ImageOpts { format, width: Option<u32>, height: Option<u32>, quality: u8, transparent: bool, crop: Option<(u32,u32,u32,u32)>, zoom: f64, screen_width: Option<u32> }`.
- Tests: pure pipeline tests (synthesize a small image → resize to a target → assert dims + format magic bytes (PNG \x89PNG / JPEG \xFF\xD8); transparent PNG keeps alpha). Gated (Chrome): screenshot a simple page → PNG decodes, dims > 0. Update MockRenderer::snapshot to return a real tiny PNG.
- Commit `feat: HTML->image snapshot (CDP captureScreenshot) + core image pipeline`.

### Task 2 — ImageSettings + registry + image C ABI
- `wkhtmltox-core/src/settings.rs`: `ImageGlobalSettings` (input `in`, `out`, `fmt`, `quality`, `screenWidth`/`smartWidth`, `crop.x/y/w/h`, `transparent`, `zoom`, plus the shared web/load fields) + registry `set_image_global(name,value)` (names per upstream `src/image/imagecommandlineparser.cc`/`imagesettings.cc`); `to_image_opts()` + `to_load_settings()`.
- `wkhtmltox-capi`: implement the `wkhtmltoimage_*` exports per `include/image.h` (init/deinit/version/extended_qt, create/destroy/set/get global settings, create_converter, convert, callbacks, current_phase/phase_count/phase_description/progress_string/http_error_code, get_output). Same pattern as the pdf ABI: opaque Boxes, catch_unwind, per-converter str_cache + output Vec, ownership-transfer of settings. `convert` = spawn ChromiumRenderer → open the input → snapshot → image::produce → store bytes; drive callbacks.
- C consumer test (gated, ASan/leaks) exercising the image ABI → output is a PNG.
- Commit `feat(capi): wkhtmltoimage_* C ABI (image) + ImageSettings registry`.

### Task 3 — `wkhtmltoimage` CLI
- New bin `wkhtmltoimage` (in a `wkhtmltoimage-cli` crate or extend the cli crate): parse the image flags (`-f/--format`, `--quality`, `--width`, `--height`, `--crop-x/-y/-w/-h`, `--transparent`, `--zoom`, `--enable-local-file-access`/`--safe`, etc. via the registry) + `<input> <output>` (+ `-` stdin/stdout). Render via snapshot+produce. Exit codes (validate input existence like wkhtmltopdf-cli). `--help`/`--version`.
- Test: gated — `wkhtmltoimage in.html out.png` → PNG; `--version`; unknown flag nonzero; bad input nonzero.
- Commit `feat(cli): wkhtmltoimage executable`.

### Task 4 — oracle comparison + milestone gate
- Harness `--image` mode: render a corpus page to PNG via our `wkhtmltoimage` and the oracle (`wkhtmltoimage --format png in.html out.png`); compare dimensions (±tol) + SSIM (single image, no pagination → SHOULD be reasonably high) + that both produce a valid PNG. Record `results-image.md`.
- Whole-milestone Opus review (incl. security: image render also fetches subresources → ResourcePolicy must apply) + static audit + fix wave + push.

## Self-Review
Covers v1 image parity (wkhtmltoimage CLI + C ABI + pipeline). Single-image render means NO pagination → fidelity to the oracle should be higher than PDF (worth measuring). Reuses M4's Fetch/policy enforcement (so `--safe` covers image too — verify in T4). Deferred: SVG output (post-v1), exhaustive image-flag parity.
