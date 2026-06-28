# wkhtmltox-rs Milestone 6 — Compat hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the no-Qt engine measurably match the wkhtmltopdf 0.12.6 oracle (a CI-gated diff harness), close the remaining CLI/feature-parity gaps (exit codes, image device-metrics, interactive forms), and re-audit the M4 security pump.

**Architecture:** Five independent hardening tasks plus a milestone security gate. The oracle harness (`tests/compat/compare.py`) gains ordered-outline + body-only-text metrics and a pass/fail gate. The Chromium renderer gains `Emulation.setDeviceMetricsOverride` (zoom/screen size) and Fetch-based image suppression. Forms reuse the existing M2b px→PDF-point transform (`assembly.rs`) plus the QPDF AcroForm shim (`wkx_pdf_add_text_field`). The final gate re-audits `call_pumping`.

**Tech Stack:** Rust 2021 (workspace at `wkhtmltox-rs/`), headless Chromium over CDP, QPDF 12.x C++ shim via `cc`, lopdf 0.42 (PDF inspection), Python 3 + PyMuPDF + scikit-image (oracle harness).

## Global Constraints

(Copied from `docs/superpowers/specs/2026-06-28-no-qt-engine-design.md` + prior milestone constraints — every task implicitly includes these.)

- No Qt of any kind. `wkhtmltox-core` and `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`; `unsafe`/FFI only in `wkhtmltox-pdf-sys` and `wkhtmltox-capi`.
- Every C ABI export wraps its body in `catch_unwind`; `const char*`/byte returns borrow per-converter owned storage; settings ownership transfers on `create_converter`. (Not directly touched here, but do not regress.)
- LGPL-3.0-or-later header comment on every new source file: `// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.`
- `cargo clippy --all-targets` clean for crates you touch (`-D warnings` is the goal; pre-existing toolchain-drift lints in untouched files — `pdfread.rs`, the PDF assembly tests — are out of scope, do not fix them, do not regress them).
- Oracle is the gold standard. Default render behavior stays permissive (drop-in compat); `--safe` stays hardened (do not weaken M4).
- Commit per task with a conventional-commit subject and the trailer:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- Chrome-requiring tests are gated with `#[ignore = "requires a real Chrome; run with: cargo test ... -- --ignored"]`. Pure-logic tests run in plain `cargo test`. The oracle binaries live at `$WKHTMLTOX_ORACLE` / `$WKHTMLTOX_IMAGE_ORACLE` (default `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmlto{pdf,image}`).
- Working dir for all `cargo`/`python3` commands below: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs`.

---

## File Structure

- `tests/compat/compare.py` — MODIFY: add `outline_tree_ratio`, `extract_body_text`, `gate_check`; use ordered outline + body text; add `--gate`.
- `tests/compat/test_compare.py` — CREATE: pure-Python unit tests for the three new functions (no Chrome/oracle).
- `tests/compat/thresholds.json` — CREATE: the CI gate floors.
- `crates/wkhtmltoimage-cli/src/main.rs` — MODIFY: local-input existence check (exit-code parity).
- `crates/wkhtmltoimage-cli/tests/exit_codes.rs` — CREATE: spawn the built binary, assert exit codes.
- `crates/wkhtmltox-core/src/render.rs` — MODIFY: add `DeviceMetrics`, extend `LoadSettings` (`device_metrics`, `load_images`).
- `crates/wkhtmltox-render-chromium/src/renderer.rs` — MODIFY: apply `Emulation.setDeviceMetricsOverride`; Fetch image-block; thread `load_images` into the pump.
- `crates/wkhtmltox-core/src/image.rs` — MODIFY: composite alpha over white on the JPEG path.
- `crates/wkhtmltox-core/src/settings.rs` + `registry.rs` — MODIFY: forward `zoom`/`screenWidth`/`screenHeight`/`smartWidth`/`web.loadImages` into `LoadSettings`; map `--enable-forms`/`produceForms`.
- `crates/wkhtmltox-core/src/forms.rs` — CREATE: `FORM_PROBE_JS`, `FormField`, `parse_form_fields`.
- `crates/wkhtmltox-pdf-sys/cpp/shim.cpp` + `shim.h` + `src/lib.rs` — MODIFY: a safe bytes-based `add_text_fields` wrapper over the existing path-based `wkx_pdf_add_text_field`.
- `crates/wkhtmltox-core/src/assembly.rs` — MODIFY: probe + place form fields (reuse the link transform) when `produce_forms` is set.
- `tests/compat/legacy/` — CREATE: realistic legacy-style HTML corpus + `results-legacy.{md,json}`.

---

### Task 1 — Compat gate: ordered outline + body-only text + threshold

**Goal:** The harness compares outlines as an *ordered* tree and text on *body content only* (excluding TOC pages + header/footer margin chrome), and can fail CI when a metric drops below a floor.

**Files:**
- Modify: `tests/compat/compare.py`
- Create: `tests/compat/test_compare.py`
- Create: `tests/compat/thresholds.json`

**Interfaces:**
- Produces: `outline_tree_ratio(ref_toc, new_toc) -> float` (ordered LCS ratio over `(level, title)` sequences); `extract_body_text(doc, top_pt=36.0, bottom_pt=36.0, skip_toc=False) -> str`; `gate_check(metrics: dict, thresholds: dict) -> list[str]` (returns list of failure strings; empty = pass).
- Consumes: existing `compare.py` helpers `extract_text`, `text_similarity`, `outline_match`, `measure_file` (unchanged signatures); PyMuPDF `fitz` `Document`/`Page`.

- [ ] **Step 1: Write the failing test file**

Create `tests/compat/test_compare.py`:

```python
# wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
"""Pure-Python unit tests for the new compat-gate helpers. No Chrome/oracle.

Run: python3 -m unittest tests.compat.test_compare   (from wkhtmltox-rs/)
  or: cd tests/compat && python3 -m unittest test_compare
"""
import unittest
import compare


class OutlineTreeRatio(unittest.TestCase):
    def _toc(self, *items):
        # fitz TOC rows are [level, title, page]; page is ignored by the metric.
        return [[lvl, title, 1] for (lvl, title) in items]

    def test_identical_order_is_one(self):
        a = self._toc((1, "Intro"), (2, "Background"), (1, "Method"))
        self.assertEqual(compare.outline_tree_ratio(a, a), 1.0)

    def test_reordered_scores_below_one(self):
        a = self._toc((1, "Intro"), (1, "Method"))
        b = self._toc((1, "Method"), (1, "Intro"))
        # Set-intersection would call these identical; ordered must not.
        self.assertLess(compare.outline_tree_ratio(a, b), 1.0)

    def test_empty_both_is_one(self):
        self.assertEqual(compare.outline_tree_ratio([], []), 1.0)

    def test_empty_one_side_is_zero(self):
        self.assertEqual(compare.outline_tree_ratio(self._toc((1, "X")), []), 0.0)


class GateCheck(unittest.TestCase):
    def test_pass_when_all_meet_floor(self):
        m = {"mean_ssim": 0.80, "outline_ratio": 1.0, "abs_delta_pages": 0}
        t = {"mean_ssim": 0.70, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        self.assertEqual(compare.gate_check(m, t), [])

    def test_fail_lists_offending_metric(self):
        m = {"mean_ssim": 0.50, "outline_ratio": 1.0, "abs_delta_pages": 0}
        t = {"mean_ssim": 0.70, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        fails = compare.gate_check(m, t)
        self.assertEqual(len(fails), 1)
        self.assertIn("mean_ssim", fails[0])

    def test_fail_on_page_drift(self):
        m = {"mean_ssim": 0.9, "outline_ratio": 1.0, "abs_delta_pages": 3}
        t = {"mean_ssim": 0.7, "outline_ratio": 1.0, "max_abs_delta_pages": 1}
        self.assertTrue(any("pages" in f for f in compare.gate_check(m, t)))


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat && python3 -m unittest test_compare -v`
Expected: FAIL / ERROR — `AttributeError: module 'compare' has no attribute 'outline_tree_ratio'`.

- [ ] **Step 3: Implement the three functions in `compare.py`**

Add near the existing `outline_match` (compare.py ~line 220). Use only the stdlib `difflib` (already imported) — no new deps:

```python
def outline_tree_ratio(ref_toc, new_toc) -> float:
    """Ordered similarity of two outlines as (level, title) sequences.

    Unlike outline_match (set intersection), this respects order and nesting:
    a reordered or re-nested outline scores below 1.0.  Uses difflib's
    longest-contiguous-matching-blocks ratio over the (level, title) tuples.
    """
    ref_seq = [(lvl, (title or "").strip()) for lvl, title, *_ in ref_toc]
    new_seq = [(lvl, (title or "").strip()) for lvl, title, *_ in new_toc]
    if not ref_seq and not new_seq:
        return 1.0
    if not ref_seq or not new_seq:
        return 0.0
    return difflib.SequenceMatcher(None, ref_seq, new_seq).ratio()


def extract_body_text(doc, top_pt: float = 36.0, bottom_pt: float = 36.0,
                      skip_toc: bool = False) -> str:
    """Concatenated page text EXCLUDING the top/bottom margin bands (where
    running headers/footers live) and, optionally, a leading TOC page.

    Header/footer chrome and TOC formatting were the dominant source of the
    low M2b text_sim despite matching body content; clipping the margin bands
    and the TOC page isolates the body for a fair comparison.
    """
    parts = []
    start = 1 if (skip_toc and doc.page_count > 1) else 0
    for i in range(start, doc.page_count):
        page = doc[i]
        r = page.rect
        body = fitz.Rect(r.x0, r.y0 + top_pt, r.x1, r.y1 - bottom_pt)
        parts.append(page.get_text("text", clip=body))
    return " ".join(parts)


def gate_check(metrics: dict, thresholds: dict) -> list:
    """Return a list of human-readable failure strings; empty list == pass.

    Recognised threshold keys: mean_ssim (floor), outline_ratio (floor),
    text_sim (floor), max_abs_delta_pages (ceiling on |pages_new-pages_ref|).
    Only keys present in `thresholds` are enforced.
    """
    fails = []
    for key in ("mean_ssim", "outline_ratio", "text_sim"):
        if key in thresholds and key in metrics and metrics[key] < thresholds[key]:
            fails.append(f"{key}={metrics[key]:.4f} < floor {thresholds[key]:.4f}")
    if "max_abs_delta_pages" in thresholds and "abs_delta_pages" in metrics:
        if metrics["abs_delta_pages"] > thresholds["max_abs_delta_pages"]:
            fails.append(
                f"abs_delta_pages={metrics['abs_delta_pages']} > "
                f"ceiling {thresholds['max_abs_delta_pages']}")
    return fails
```

Confirm `import difflib` and `import fitz` are already at the top of `compare.py` (they are — `text_similarity` uses difflib, the doc loaders use fitz). If `json` is not already imported at module top, add `import json` (needed in Step 5).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat && python3 -m unittest test_compare -v`
Expected: PASS (7 tests OK).

- [ ] **Step 5: Wire ordered-outline + body-text into the corpus sweep and add `--gate`**

In `measure_file()` (compare.py), where `outline_match` is currently computed, ALSO compute and record `outline_ratio = outline_tree_ratio(ref_toc, new_toc)` and `body_text_sim = text_similarity(extract_body_text(ref_doc, skip_toc=skip_toc), extract_body_text(new_doc, skip_toc=skip_toc))` into the per-file result dict, alongside (do not remove) the existing `text_sim`/`outline` fields. Thread a `skip_toc` argument through `measure_file` (default `False`; the `--m2b` path passes `True` because it prepends a TOC). Add both new fields to the markdown table and JSON output.

Add a `--gate` argparse flag and a `--thresholds PATH` flag (default `tests/compat/thresholds.json`). After the sweep, build a `metrics` dict from the AGGREGATE numbers (`mean_ssim` over all docs, the minimum `outline_ratio` across docs, the maximum `abs_delta_pages` across docs as `abs_delta_pages`, mean `body_text_sim` as `text_sim`), call `gate_check`, print each failure, and `sys.exit(1)` if any — but ONLY when `--gate` was passed (default run stays informational, exit 0). Load thresholds with `json.load`.

- [ ] **Step 6: Create the thresholds file**

Create `tests/compat/thresholds.json` with floors set just below current measured baselines so the gate catches regressions without flapping (baselines from logbook: corpus mean_ssim 0.734, outline 13/13 ordered, page drift ≤2):

```json
{
  "mean_ssim": 0.68,
  "outline_ratio": 0.95,
  "text_sim": 0.90,
  "max_abs_delta_pages": 3
}
```

- [ ] **Step 7: Smoke-run the gate against the corpus (informational; needs Chrome+oracle)**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs && python3 tests/compat/compare.py --gate 2>&1 | tail -20`
Expected: prints the table, then `GATE: PASS` (exit 0). If the environment lacks Chrome/oracle this step is skipped — note that in the report; the unit tests in Step 4 are the gating evidence for review.

- [ ] **Step 8: Commit**

```bash
git add tests/compat/compare.py tests/compat/test_compare.py tests/compat/thresholds.json
git commit -m "feat(compat): ordered-outline + body-only-text metrics + --gate threshold check

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2 — Exit-code parity with the oracle

**Goal:** `wkhtmltoimage` returns a nonzero exit code for a missing local input file (matching the oracle), closing the known M5 divergence (Chrome silently rendered an error page and we returned 0). A spawn-the-binary integration test pins the parity.

**Files:**
- Modify: `crates/wkhtmltoimage-cli/src/main.rs`
- Create: `crates/wkhtmltoimage-cli/tests/exit_codes.rs`

**Interfaces:**
- Consumes: the built binary via `env!("CARGO_BIN_EXE_wkhtmltoimage")` (Cargo sets this for integration tests of a bin crate).
- Produces: no library surface; behavior change only.

- [ ] **Step 1: Write the failing integration test**

Create `crates/wkhtmltoimage-cli/tests/exit_codes.rs`:

```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Exit-code parity tests. These spawn the built binary; the missing-input
//! and bad-flag cases return BEFORE any Chrome spawn, so they need no browser.
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wkhtmltoimage"))
}

#[test]
fn missing_local_input_is_nonzero() {
    // A local path that does not exist must fail (oracle returns nonzero).
    let status = bin()
        .args(["/no/such/input/file_xyz.html", "/tmp/out_should_not_exist.png"])
        .status()
        .expect("spawn");
    assert!(!status.success(), "missing input must be nonzero, got {status:?}");
}

#[test]
fn unknown_flag_is_nonzero() {
    let status = bin()
        .args(["--definitely-not-a-flag", "in.html", "out.png"])
        .status()
        .expect("spawn");
    assert!(!status.success());
}

#[test]
fn version_is_zero() {
    let status = bin().arg("--version").status().expect("spawn");
    assert!(status.success());
}
```

- [ ] **Step 2: Run the test to verify the missing-input case fails**

Run: `cargo test -p wkhtmltoimage-cli --test exit_codes`
Expected: `missing_local_input_is_nonzero` FAILS (currently returns 0). `unknown_flag_is_nonzero` and `version_is_zero` pass.

- [ ] **Step 3: Add the local-input existence check**

In `crates/wkhtmltoimage-cli/src/main.rs`, in `run()`, AFTER argument parsing and BEFORE the input is turned into a `file://` URL / Chrome is spawned, add the same guard `wkhtmltopdf-cli` uses (mirror its lines 76–89). Only check when the input is a real local path — not `-` (stdin) and not an `http(s)://`/`file://` URL:

```rust
// Exit-code parity with the oracle: a non-existent local input must fail
// here, not silently render Chrome's error page (returns 0).  Skip the
// check for stdin ("-") and for explicit URLs.
let is_url = input.starts_with("http://")
    || input.starts_with("https://")
    || input.starts_with("file://");
if input != "-" && !is_url && !std::path::Path::new(input).exists() {
    eprintln!("wkhtmltoimage: error: input file not found: {input}");
    return 1;
}
```

Use the actual variable name for the positional input as it exists in `main.rs` (the recon names it the first positional; confirm and match it).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p wkhtmltoimage-cli --test exit_codes`
Expected: all three PASS.

- [ ] **Step 5: Confirm the no-regression on existing CLI tests**

Run: `cargo test -p wkhtmltoimage-cli`
Expected: all existing unit tests still pass (34/34 from M5) plus the 3 new integration tests.

- [ ] **Step 6: Commit**

```bash
git add crates/wkhtmltoimage-cli/src/main.rs crates/wkhtmltoimage-cli/tests/exit_codes.rs
git commit -m "fix(cli): wkhtmltoimage exits nonzero on missing local input (oracle exit-code parity)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3 — Image device-metrics forwarding (zoom / screen size / no-images / JPEG alpha)

**Goal:** Wire the M5 stored-but-not-forwarded settings to CDP: `--zoom` → `deviceScaleFactor`; `screenWidth`/`screenHeight`/`smartWidth` → `Emulation.setDeviceMetricsOverride`; `web.loadImages=false`/`--no-images` → Fetch-blocked `Image` requests; and composite alpha over white on the JPEG encode path (was a raw channel drop).

**Files:**
- Modify: `crates/wkhtmltox-core/src/render.rs` (add `DeviceMetrics`, extend `LoadSettings`)
- Modify: `crates/wkhtmltox-render-chromium/src/renderer.rs` (apply metrics; block images)
- Modify: `crates/wkhtmltox-core/src/image.rs` (JPEG composite)
- Modify: `crates/wkhtmltox-core/src/settings.rs` + `registry.rs` (forward the settings)

**Interfaces:**
- Produces: `pub struct DeviceMetrics { pub width: u32, pub height: u32, pub device_scale_factor: f64, pub smart_width: bool }`; `LoadSettings.device_metrics: Option<DeviceMetrics>`; `LoadSettings.load_images: bool` (default `true`).
- Consumes: existing `LoadSettings` (M4 fields: cookies, headers, auth, policy, `compat_ua_css`, `enable_javascript`); the M4 Fetch pump (`call_pumping`, `handle_fetch_event`, `fetch_action`); `ImageGlobalSettings.{zoom, screen_width, screen_height, smart_width}` and `to_load_settings()`.

- [ ] **Step 1: Write the failing unit tests (core, no Chrome)**

In `crates/wkhtmltox-core/src/image.rs` tests module, add a JPEG-alpha test; in `crates/wkhtmltox-core/src/settings.rs` tests module, add a forwarding test. JPEG test:

```rust
#[test]
fn jpeg_flattens_alpha_over_white_not_black() {
    // A fully transparent RGBA pixel must encode as white (255), not black,
    // on the JPEG path (JPEG has no alpha; raw to_rgb8 would drop to 0,0,0).
    use image as img;
    let mut rgba = img::RgbaImage::new(2, 2);
    for px in rgba.pixels_mut() { *px = img::Rgba([0, 0, 0, 0]); } // transparent black
    let raw = RawImage { bytes: encode_png(&img::DynamicImage::ImageRgba8(rgba)), format: ImageFormat::Png };
    let opts = ImageOpts { format: ImageFormat::Jpeg, transparent: false, quality: 90,
                           width: None, height: None, crop: None, zoom: 1.0, screen_width: None };
    let out = produce(&raw, &opts).unwrap();
    let decoded = img::load_from_memory(&out).unwrap().to_rgb8();
    let p = decoded.get_pixel(0, 0);
    assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "transparent→white, got {p:?}");
}
```

If a `encode_png` test helper does not already exist in `image.rs`, inline the encoding with the `image` crate's `DynamicImage::write_to(&mut Cursor, ImageFormat::Png)` instead. Settings test:

```rust
#[test]
fn image_settings_forward_zoom_and_screen_width_to_device_metrics() {
    let mut g = ImageGlobalSettings::default();
    g.zoom = 2.0;
    g.screen_width = Some(800);
    g.smart_width = false;
    let ls = g.to_load_settings();
    let dm = ls.device_metrics.expect("device_metrics set when zoom/width given");
    assert_eq!(dm.width, 800);
    assert!((dm.device_scale_factor - 2.0).abs() < 1e-9);
    assert!(!dm.smart_width);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wkhtmltox-core image:: && cargo test -p wkhtmltox-core settings::`
Expected: both new tests FAIL (`device_metrics` field missing; JPEG returns black `0,0,0`).

- [ ] **Step 3: Add `DeviceMetrics` + `LoadSettings` fields**

In `crates/wkhtmltox-core/src/render.rs`:

```rust
/// Viewport/scale overrides applied via CDP `Emulation.setDeviceMetricsOverride`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceMetrics {
    pub width: u32,             // 0 = let Chrome choose
    pub height: u32,            // 0 = let Chrome choose
    pub device_scale_factor: f64, // 1.0 = no zoom
    pub smart_width: bool,      // expand width to content after load
}
```

Add to `LoadSettings`: `pub device_metrics: Option<DeviceMetrics>,` and `pub load_images: bool,`. In `LoadSettings`'s `Default`, set `device_metrics: None` and `load_images: true` (permissive default). Update any struct-literal constructions of `LoadSettings` in the workspace that don't use `..Default::default()` (search `rg "LoadSettings \{" crates`).

- [ ] **Step 4: JPEG composite in `image.rs`**

Replace the JPEG branch (image.rs ~line 122). Reuse the same over-white blend already used by the PNG non-transparent branch — flatten RGBA over white BEFORE encoding instead of `to_rgb8()`:

```rust
// JPEG has no alpha channel; composite over white (matches the PNG
// non-transparent path) so transparency flattens to white, never black.
let rgba = dynimg.to_rgba8();
let mut flat = img::RgbImage::new(rgba.width(), rgba.height());
for (x, y, px) in rgba.enumerate_pixels() {
    let a = px[3] as f32 / 255.0;
    let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
    flat.put_pixel(x, y, img::Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
}
let rgb = img::DynamicImage::ImageRgb8(flat);
```

(If a shared `composite_over_white(&RgbaImage) -> RgbImage` helper makes the PNG and JPEG paths DRY, extract it and call it from both — preferred.)

- [ ] **Step 5: Forward settings in `settings.rs` / `registry.rs`**

In `ImageGlobalSettings::to_load_settings()`: when `zoom != 1.0` OR `screen_width.is_some()` OR `screen_height.is_some()`, populate `device_metrics: Some(DeviceMetrics { width: screen_width.unwrap_or(0), height: screen_height.unwrap_or(0), device_scale_factor: if zoom > 0.0 { zoom } else { 1.0 }, smart_width })`. In `registry.rs` `set_image_global`, move `"web.loadImages"` out of the recognised-but-unimplemented block: parse a bool and store it (add a `load_images: bool` field to `ImageGlobalSettings`, default `true`), and set `LoadSettings.load_images` from it in `to_load_settings()`. Map the CLI `--no-images` flag (in `wkhtmltoimage-cli`) to `web.loadImages=false` (it likely already maps to `web.loadImages`; ensure the registry now honors it). Keep `screenWidth`/`screenHeight`/`smartWidth`/`zoom` parsing as-is (they are already stored) — only the forwarding is new.

- [ ] **Step 6: Run core tests to verify pass**

Run: `cargo test -p wkhtmltox-core`
Expected: the two new tests PASS; all prior core tests still pass.

- [ ] **Step 7: Apply the metrics + image-block in the renderer**

In `crates/wkhtmltox-render-chromium/src/renderer.rs`:

(a) In `open()`, AFTER the page target/session is set up and BEFORE `Page.navigate`, if `load.device_metrics` is `Some(dm)` and any of `dm.width`/`dm.height`/`dm.device_scale_factor != 1.0` is meaningful, call:

```rust
if let Some(dm) = &load.device_metrics {
    let w = if dm.width > 0 { dm.width } else { 1024 }; // upstream screenWidth default
    let h = if dm.height > 0 { dm.height } else { 0 };
    self.cdp.call("Emulation.setDeviceMetricsOverride", json!({
        "width": w, "height": h,
        "deviceScaleFactor": if dm.device_scale_factor > 0.0 { dm.device_scale_factor } else { 1.0 },
        "mobile": false
    }))?;
}
```

(b) smart_width expansion: after `wait_ready`, if `dm.smart_width` and `dm.width > 0`, measure `document.documentElement.scrollWidth` via `eval_json`, and if it exceeds `w`, re-issue `setDeviceMetricsOverride` with the larger width (content-fit). Keep it best-effort; document as approximate.

(c) Image blocking: thread `load_images` into the Fetch pump. In the `fetch_state` struct (the one holding `nav_url`/`policy`/`extra_headers`), add `load_images: bool`. In `handle_fetch_event` / `fetch_action`, BEFORE the policy decision, if `!load_images` and the event's `resourceType == "Image"`, return a Fail action (`Fetch.failRequest` `"BlockedByClient"`), so images never load. Set `fetch_state.load_images = load.load_images` where `fetch_state` is constructed in `open()`.

- [ ] **Step 8: Add a gated e2e test (real Chrome)**

In `crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`:

```rust
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn snapshot_honors_screen_width_via_device_metrics() {
    use wkhtmltox_core::render::*;
    use wkhtmltox_core::image;
    let mut r = ChromiumRenderer::spawn(&Default::default()).unwrap();
    let mut load = LoadSettings::default();
    load.device_metrics = Some(DeviceMetrics { width: 800, height: 0, device_scale_factor: 1.0, smart_width: false });
    let p = r.open(&Source::Html("<html><body style='margin:0'><div style='width:100%'>x</div></body></html>".into()), &load).unwrap();
    r.wait_ready(p, &ReadyPolicy::default()).unwrap();
    let raw = r.snapshot(p, &SnapshotOpts { format: ImageFormat::Png, crop: None, scale: 1.0, quality: 90 }).unwrap();
    let img = image::decode_dims(&raw.bytes).unwrap(); // (w,h)
    assert!((img.0 as i64 - 800).abs() <= 2, "width ~800, got {}", img.0);
}
```

If a `decode_dims` helper is not present, decode inline with `image::load_from_memory(&raw.bytes).unwrap().dimensions()`. Match the actual `ChromiumRenderer::spawn` signature.

- [ ] **Step 9: Run unit tests + build the e2e (do not require Chrome in CI step)**

Run: `cargo test -p wkhtmltox-core && cargo build -p wkhtmltox-render-chromium --tests && cargo clippy -p wkhtmltox-core -p wkhtmltox-render-chromium`
Expected: core tests pass; e2e compiles; clippy clean on touched crates. If Chrome is available, also run `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1 snapshot_honors_screen_width_via_device_metrics` and report the result.

- [ ] **Step 10: Commit**

```bash
git add crates/wkhtmltox-core/src/render.rs crates/wkhtmltox-core/src/image.rs \
        crates/wkhtmltox-core/src/settings.rs crates/wkhtmltox-core/src/registry.rs \
        crates/wkhtmltox-render-chromium/src/renderer.rs \
        crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs
git commit -m "feat(image): forward zoom/screen-size via Emulation, block images via Fetch, JPEG alpha-over-white

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4 — Forms → AcroForm end-to-end (text inputs)

**Goal:** When forms are enabled, HTML `<input type=text>` / `<textarea>` fields become interactive AcroForm `/Tx` fields in the output PDF, placed using the same px→PDF-point transform that M2b uses for `/Link` synthesis. Scope: text fields only (checkbox/radio/select → noted post-v1).

**Files:**
- Create: `crates/wkhtmltox-core/src/forms.rs`
- Modify: `crates/wkhtmltox-pdf-sys/cpp/shim.cpp`, `shim.h`, `src/lib.rs` (safe bytes wrapper)
- Modify: `crates/wkhtmltox-core/src/assembly.rs` (probe + place)
- Modify: `crates/wkhtmltox-core/src/settings.rs` + `registry.rs` + `lib.rs` (module decl) + `crates/wkhtmltopdf-cli/src/main.rs` (flag)

**Interfaces:**
- Produces: `forms::FORM_PROBE_JS: &str`; `forms::FormField { name: String, kind: FormFieldKind, top: f64, rect: [f64;4] }` (`kind` = `Text`); `forms::parse_form_fields(&serde_json::Value) -> Vec<FormField>`; `wkhtmltox_pdf_sys::add_text_fields(pdf: &[u8], fields: &[TextFieldSpec]) -> Result<Vec<u8>, ...>` where `TextFieldSpec { name: String, page_index: u32, rect: [f64;4] }`.
- Consumes: `outline::PROBE_JS` shape (model the probe on it); the link transform in `assembly.rs:151–222` (`page_height_pt`, `page_height_px`, `×0.75`, bottom-up y-flip, per-object `global_*_page` accounting); the existing path-based shim `wkx_pdf_add_text_field(in,out,name,page_index,x,y,w,h)`; `AssembleOpts`.

- [ ] **Step 1: Write the failing probe-parse test (core, no Chrome)**

Create `crates/wkhtmltox-core/src/forms.rs`:

```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! HTML form-field extraction → interactive PDF AcroForm fields.
//! v1 scope: text inputs and textareas. Checkbox/radio/select are deferred.
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum FormFieldKind { Text }

#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    pub name: String,
    pub kind: FormFieldKind,
    /// Document-absolute top Y in CSS px (for source-page derivation).
    pub top: f64,
    /// Bounding rect [x0, y0, x1, y1] in document-absolute CSS px.
    pub rect: [f64; 4],
}

/// JS injected via `Renderer::eval_json`. Mirrors `outline::PROBE_JS`'s
/// coordinate handling: viewport-relative rects shifted by scrollY.
pub const FORM_PROBE_JS: &str = r#"(() => {
  const sy = (typeof window !== 'undefined' && window.scrollY) || 0;
  const sel = 'input[type=text], input:not([type]), textarea';
  const fs = [...document.querySelectorAll(sel)].map(e => {
    const r = e.getBoundingClientRect();
    return {
      name: e.getAttribute('name') || e.id || '',
      kind: 'text',
      top: r.top + sy,
      rect: [r.left, r.top + sy, r.right, r.bottom + sy]
    };
  }).filter(f => f.name && f.rect[2] > f.rect[0] && f.rect[3] > f.rect[1]);
  return { fields: fs };
})()"#;

pub fn parse_form_fields(v: &Value) -> Vec<FormField> {
    v.get("fields")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let rect = f.get("rect")?.as_array()?;
                    if rect.len() != 4 { return None; }
                    let r = |i: usize| rect[i].as_f64();
                    Some(FormField {
                        name: f.get("name")?.as_str()?.to_string(),
                        kind: FormFieldKind::Text,
                        top: f.get("top")?.as_f64()?,
                        rect: [r(0)?, r(1)?, r(2)?, r(3)?],
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_named_text_fields() {
        let v = serde_json::json!({ "fields": [
            { "name": "email", "kind": "text", "top": 100.0, "rect": [10.0, 100.0, 210.0, 124.0] }
        ]});
        let fs = parse_form_fields(&v);
        assert_eq!(fs.len(), 1);
        assert_eq!(fs[0].name, "email");
        assert_eq!(fs[0].kind, FormFieldKind::Text);
        assert_eq!(fs[0].rect, [10.0, 100.0, 210.0, 124.0]);
    }
    #[test]
    fn skips_unnamed_and_degenerate() {
        let v = serde_json::json!({ "fields": [
            { "name": "", "kind": "text", "top": 0.0, "rect": [0.0,0.0,10.0,10.0] }
        ]});
        assert!(parse_form_fields(&v).is_empty());
    }
}
```

Declare the module in `crates/wkhtmltox-core/src/lib.rs`: `pub mod forms;`.

- [ ] **Step 2: Run to verify the module compiles and tests pass (parse layer first)**

Run: `cargo test -p wkhtmltox-core forms::`
Expected: PASS (2 tests). (This is the pure layer; it should pass once the module is declared.)

- [ ] **Step 3: Write the failing pdf-sys wrapper test**

In `crates/wkhtmltox-pdf-sys/tests/` add `forms.rs`:

```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_pdf_sys::{add_text_fields, TextFieldSpec};

#[test]
fn adds_two_text_fields_to_acroform() {
    // Minimal one-page PDF from the existing roundtrip fixture helper, or
    // build via the engine in an integration test. Here use the crate's
    // existing test PDF bytes helper (see tests/roundtrip.rs for the source).
    let pdf = wkhtmltox_pdf_sys::test_support::one_page_pdf();
    let out = add_text_fields(&pdf, &[
        TextFieldSpec { name: "a".into(), page_index: 0, rect: [72.0, 700.0, 272.0, 724.0] },
        TextFieldSpec { name: "b".into(), page_index: 0, rect: [72.0, 660.0, 272.0, 684.0] },
    ]).expect("add fields");
    // Verify via lopdf: catalog /AcroForm exists and has 2 /Fields.
    let doc = lopdf::Document::load_mem(&out).unwrap();
    let cat = doc.catalog().unwrap();
    let acro = cat.get(b"AcroForm").unwrap();
    let acro = doc.dereference(acro).unwrap().1.as_dict().unwrap();
    let fields = acro.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2);
}
```

If `test_support::one_page_pdf()` does not exist, reuse exactly the PDF-bytes source already used by `crates/wkhtmltox-pdf-sys/tests/roundtrip.rs` / `acroform.rs` (the existing AcroForm spike test already builds/loads a one-page PDF — copy that setup). Add `lopdf` as a `[dev-dependencies]` of `wkhtmltox-pdf-sys` if not already present (it is used by the existing acroform test, so it is).

- [ ] **Step 4: Run to verify failure**

Run: `cargo test -p wkhtmltox-pdf-sys --test forms`
Expected: FAIL — `add_text_fields` / `TextFieldSpec` not found.

- [ ] **Step 5: Implement the safe bytes wrapper**

In `crates/wkhtmltox-pdf-sys/src/lib.rs` add (no new C++ — loop the existing path-based shim through temp files, threading in→out):

```rust
/// One interactive text field to add: name, 0-based page, PDF-point rect
/// [x0, y0, x1, y1] (bottom-up origin).
#[derive(Debug, Clone)]
pub struct TextFieldSpec { pub name: String, pub page_index: u32, pub rect: [f64; 4] }

/// Add interactive `/Tx` AcroForm fields to `pdf`, returning new bytes.
/// Reuses the tested single-field shim; each call rewrites the file, so this
/// is O(n) in field count — fine for the handful of fields a page carries.
pub fn add_text_fields(pdf: &[u8], fields: &[TextFieldSpec]) -> Result<Vec<u8>, PdfError> {
    if fields.is_empty() { return Ok(pdf.to_vec()); }
    let dir = tempfile::TempDir::new().map_err(PdfError::Io)?;
    let mut cur = dir.path().join("cur.pdf");
    std::fs::write(&cur, pdf).map_err(PdfError::Io)?;
    for (i, f) in fields.iter().enumerate() {
        let out = dir.path().join(format!("o{i}.pdf"));
        let in_c = path_cstring(&cur)?;
        let out_c = path_cstring(&out)?;
        let name_c = std::ffi::CString::new(f.name.as_str()).map_err(|_| PdfError::BadArg)?;
        let rc = unsafe {
            wkx_pdf_add_text_field(
                in_c.as_ptr(), out_c.as_ptr(), name_c.as_ptr(),
                f.page_index as c_int,
                f.rect[0], f.rect[1], f.rect[2] - f.rect[0], f.rect[3] - f.rect[1],
            )
        };
        if rc != 0 { return Err(PdfError::Shim(rc)); }
        cur = out;
    }
    std::fs::read(&cur).map_err(PdfError::Io)
}
```

Match the crate's existing error type and helpers: use the same `PdfError` variants the other wrappers (`set_outline`, `add_links`) return, and the same path→`CString` helper if one exists (else add `path_cstring`). Note the shim takes `(x, y, w, h)` — pass width/height computed from the rect. The `tempfile` crate is already a dependency (used by assembly temp dirs).

- [ ] **Step 6: Run to verify pass**

Run: `cargo test -p wkhtmltox-pdf-sys --test forms`
Expected: PASS (catalog `/AcroForm` has 2 `/Fields`).

- [ ] **Step 7: Wire into assembly + settings + registry + CLI flag**

(a) `settings.rs`: add `produce_forms: bool` to the PDF object/global settings struct that flows into `AssembleOpts` (follow how an existing per-object bool like `number`/cover flows). Add `pub produce_forms: bool` to `AssembleOpts` (default `false`).

(b) `registry.rs`: in `set_object`, move `"produceForms"` out of the recognised-but-unimplemented block and store it as a bool into `produce_forms`. In `wkhtmltopdf-cli/src/main.rs`, map the `--enable-forms` flag to `produceForms=true`.

(c) `assembly.rs`: inside the existing per-object loop (the one that already runs `PROBE_JS` and accumulates `link_annots`, ~lines 160–222), when `opts.produce_forms`, ALSO `eval_json(p, forms::FORM_PROBE_JS)`, `forms::parse_form_fields(&v)`, and for each field reuse the IDENTICAL transform already used for links to compute `global_src_page` and the PDF-point rect, accumulating into `Vec<wkhtmltox_pdf_sys::TextFieldSpec>`. After the merged PDF is built (alongside where `add_links` is applied), call `wkhtmltox_pdf_sys::add_text_fields(&pdf_bytes, &field_specs)` when non-empty. Keep this on the SAME non-TOC path as links; document the TOC-path deferral with a `NOTE(forms)` comment matching the existing link `NOTE(T5)` at assembly.rs:125.

- [ ] **Step 8: Add a gated end-to-end test (real Chrome)**

In `crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`:

```rust
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn forms_become_acroform_fields() {
    use wkhtmltox_core::{assembly::*, render::*};
    let mut r = ChromiumRenderer::spawn(&Default::default()).unwrap();
    let html = "<html><body><form><input type='text' name='email'><textarea name='note'></textarea></form></body></html>";
    let opts = AssembleOpts { produce_forms: true, ..Default::default() };
    let pdf = assemble_pdf(&mut r, &PageGeometry::default(),
                           &[Source::Html(html.into())], &opts).unwrap();
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    let acro = doc.dereference(cat.get(b"AcroForm").unwrap()).unwrap().1;
    let fields = acro.as_dict().unwrap().get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2, "email + note → 2 AcroForm fields");
}
```

Match the real `assemble_pdf` signature (the recon shows `assemble_pdf(r, geom, objects, opts)` — confirm argument order in `assembly.rs`). Add `lopdf` to `wkhtmltox-render-chromium` `[dev-dependencies]` if needed.

- [ ] **Step 9: Run the layered tests**

Run: `cargo test -p wkhtmltox-core forms:: && cargo test -p wkhtmltox-pdf-sys --test forms && cargo build -p wkhtmltox-render-chromium --tests && cargo clippy -p wkhtmltox-core -p wkhtmltox-pdf-sys`
Expected: core + pdf-sys form tests pass; e2e compiles; clippy clean. If Chrome present: `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1 forms_become_acroform_fields` and report.

- [ ] **Step 10: Commit**

```bash
git add crates/wkhtmltox-core/src/forms.rs crates/wkhtmltox-core/src/lib.rs \
        crates/wkhtmltox-core/src/assembly.rs crates/wkhtmltox-core/src/settings.rs \
        crates/wkhtmltox-core/src/registry.rs \
        crates/wkhtmltox-pdf-sys/src/lib.rs crates/wkhtmltox-pdf-sys/tests/forms.rs \
        crates/wkhtmltopdf-cli/src/main.rs \
        crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs
git commit -m "feat(forms): HTML text inputs -> interactive AcroForm fields (--enable-forms)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5 — Legacy visual corpus + recorded results

**Goal:** Add a small corpus of realistic legacy-style documents (the kind wkhtmltopdf was used for: invoices/reports), run them through the fidelity harness, and check in the measured results so regressions are visible.

**Files:**
- Create: `tests/compat/legacy/invoice.html`, `tests/compat/legacy/report.html`, `tests/compat/legacy/article.html`
- Create: `tests/compat/results-legacy.md`, `tests/compat/results-legacy.json` (generated)

**Interfaces:**
- Consumes: the corpus sweep in `compare.py` (`--dir`/`--out-prefix`).
- Produces: data + recorded metrics only.

- [ ] **Step 1: Create three realistic legacy HTML documents**

Create `tests/compat/legacy/invoice.html` (a table-driven invoice with a header block, line-item table with borders, totals, and a footer note — pure HTML+inline CSS, self-contained, no external resources), `tests/compat/legacy/report.html` (multi-section report with h1/h2 headings, paragraphs, an ordered list, and a bordered data table spanning > 1 page), and `tests/compat/legacy/article.html` (long single-column prose with a couple of `<h2>` and a `<blockquote>`). Each MUST be fully self-contained (no network — keeps `--safe`/offline runs deterministic). Keep each ~1–3 printed pages.

- [ ] **Step 2: Run the harness over the legacy corpus**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs && python3 tests/compat/compare.py --dir tests/compat/legacy --out-prefix results-legacy 2>&1 | tail -25`
Expected: a table with one row per doc and aggregate `mean_ssim`, `outline_ratio`, `body_text_sim`, page deltas; `results-legacy.md` + `results-legacy.json` written. (Requires Chrome + oracle. If unavailable in the environment, note it in the report and commit just the corpus HTML; the milestone gate run will populate results.)

- [ ] **Step 3: Sanity-check the numbers**

Confirm every doc produced a valid PDF on both engines (status OK, no NEW_CRASH/ORACLE_CRASH). Record the aggregate `mean_ssim` and worst diverging doc in the commit message.

- [ ] **Step 4: Commit**

```bash
git add tests/compat/legacy tests/compat/results-legacy.md tests/compat/results-legacy.json
git commit -m "test(compat): realistic legacy-doc visual corpus (invoice/report/article) + recorded results

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Milestone Gate (controller-run, not an implementer task)

After Tasks 1–5 review clean:

1. **Whole-milestone Opus review** over the full M6 diff (`scripts/review-package <M6-base> HEAD`), with an explicit, dedicated **security re-audit of the M4 `call_pumping` whole-load Fetch pump** (renderer.rs:421–445 + the `open()` event loop 583–665): confirm no residual fail-open now that Task 3 adds an image-block branch and Task 4 adds a second `eval_json` probe through `call_pumping` — verify the new probe and the smart_width re-measure cannot bypass policy, and that the image-block branch cannot be used to *allow* a normally-blocked request. Plus the standard image/forms correctness + FFI safety pass.
2. **Static audit:** `scripts/security-audit.sh` (expect HIGH=0).
3. **Fix wave** for any Critical/Important findings (one fix subagent with the complete list).
4. **Push** `git push fork codex/macos-arm64-wkhtmltopdf`.
5. Update `TODO.md` (M6 → Done, M7 → In progress) + `logbook.md`.

---

## Self-Review

**1. Spec / TODO coverage:**
- "diff-vs-oracle as CI gate (ordered outline-tree comparison; body-only text metric)" → Task 1 ✅
- "exit-code derivation from the reference binary" → Task 2 ✅
- "forms→AcroForm wiring end-to-end" → Task 4 ✅ (text inputs; checkbox/radio/select deferred — noted)
- "visual/pixel corpus on real legacy docs" → Task 5 ✅
- "re-audit the M4 call_pumping pump" → Milestone Gate step 1 ✅
- Deferred M5 image settings (zoom/no-images/screen dims/JPEG alpha) → Task 3 ✅

**2. Placeholder scan:** No "TBD"/"handle edge cases"/"similar to Task N". Each code step carries real code. Two intentional "match the actual signature" instructions (Task 2 input var name; Task 3/4 `spawn`/`assemble_pdf` arg order) point the implementer at existing code to read, not at undefined symbols — acceptable since those symbols exist today.

**3. Type consistency:** `DeviceMetrics` fields (`width/height/device_scale_factor/smart_width`) are used identically in render.rs (Task 3 Step 3), settings forwarding (Step 5), and the renderer (Step 7) and tests. `FormField`/`FormFieldKind`/`parse_form_fields` (Task 4 Step 1) match their use in assembly wiring (Step 7) and the e2e (Step 8). `TextFieldSpec { name, page_index, rect:[f64;4] }` matches between the pdf-sys wrapper (Step 5), its test (Step 3), and the assembly accumulation (Step 7). `gate_check`/`outline_tree_ratio`/`extract_body_text` signatures match between the test (Task 1 Step 1) and impl (Step 3).

**4. Risk notes:** Task 4 coordinate accuracy inherits the same "APPROXIMATE" caveat as link rects (assembly.rs:151) — the e2e asserts field *count* and presence, not pixel-exact placement, consistent with the link tests and the deferred "link-rect coordinate accuracy" post-v1 item. Task 3 `setDeviceMetricsOverride` height=0 behavior and smart_width re-measure are best-effort; the gated test asserts width parity (the M5-observed 756-vs-800 gap), the concrete regression it targets.
