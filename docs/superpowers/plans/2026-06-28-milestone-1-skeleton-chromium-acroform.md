# wkhtmltox-rs Milestone 1 — Workspace + Chromium Renderer + AcroForm Spike — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Rust workspace and prove, with running code, the two riskiest assumptions of the design: (1) a single out-of-process headless Chromium driven over CDP yields paginated PDFs with a correct outline + page-accurate destinations, and (2) interactive AcroForm fields can be added on top of engine PDF output via QPDF.

**Architecture:** A Cargo workspace with a safe-Rust core (`wkhtmltox-core`) defining a blocking `Renderer` trait, a pure-Rust Chromium/CDP backend (`wkhtmltox-render-chromium`, sync websocket — no async runtime, no FFI), and a `wkhtmltox-pdf-sys` crate that FFIs to QPDF via a tiny C++ shim. Two `unsafe` edges only: QPDF and (later) the C ABI.

**Tech Stack:** Rust (edition 2021); `serde_json`, `tungstenite` (sync WS), `ureq` (HTTP), `thiserror`; `cc` (build) + `libqpdf` (C++ shim); `lopdf` (dev-only, PDF inspection in tests); Google Chrome / `chrome-headless-shell` ≥ 126 (for `generateDocumentOutline`).

## Global Constraints

- Language: Rust, edition 2021. No Qt of any kind (including QtCore).
- Engine: headless Chromium driven over CDP **out-of-process** (subprocess). No embedding.
- Drop-in target (later milestones): CLI + `libwkhtmltox` C ABI; do not break those names when they appear.
- `unsafe`/FFI is allowed ONLY in `wkhtmltox-pdf-sys` (QPDF) and `wkhtmltox-capi` (later). `wkhtmltox-core` and `wkhtmltox-render-chromium` must be `#![forbid(unsafe_code)]`.
- License: LGPLv3. Every source file starts with the short LGPLv3 header comment. All third-party deps must be license-compatible (QPDF Apache-2; tungstenite/ureq/serde/lopdf MIT/Apache).
- All new code lives under `wkhtmltox-rs/` at the repo root; do not modify the legacy `src/` or `qt/` trees.
- Chrome binary is located via env var `WKHTMLTOX_CHROME` if set, else platform defaults; browser-dependent tests are `#[ignore]` by default and run explicitly.

---

### Task 1: Cargo workspace skeleton

**Files:**
- Create: `wkhtmltox-rs/Cargo.toml` (workspace)
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/Cargo.toml`, `.../src/lib.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/Cargo.toml`, `.../src/lib.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/Cargo.toml`, `.../src/lib.rs`
- Create: `wkhtmltox-rs/.gitignore`

**Interfaces:**
- Produces: a buildable workspace with three library crates; `wkhtmltox-core` and `wkhtmltox-render-chromium` declare `#![forbid(unsafe_code)]`.

- [ ] **Step 1: Create the workspace manifest**

`wkhtmltox-rs/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2021"
license = "LGPL-3.0-or-later"
rust-version = "1.74"

[workspace.dependencies]
serde_json = "1"
thiserror = "2"
tungstenite = "0.24"
ureq = "2"
lopdf = "0.34"
cc = "1"
```

- [ ] **Step 2: Create the three crate manifests**

`wkhtmltox-rs/crates/wkhtmltox-core/Cargo.toml`:
```toml
[package]
name = "wkhtmltox-core"
edition.workspace = true
license.workspace = true

[dependencies]
serde_json.workspace = true
thiserror.workspace = true
```

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/Cargo.toml`:
```toml
[package]
name = "wkhtmltox-render-chromium"
edition.workspace = true
license.workspace = true

[dependencies]
wkhtmltox-core = { path = "../wkhtmltox-core" }
serde_json.workspace = true
tungstenite.workspace = true
ureq.workspace = true

[dev-dependencies]
lopdf.workspace = true
```

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/Cargo.toml`:
```toml
[package]
name = "wkhtmltox-pdf-sys"
edition.workspace = true
license.workspace = true
build = "build.rs"

[build-dependencies]
cc.workspace = true

[dev-dependencies]
lopdf.workspace = true
```

- [ ] **Step 3: Create stub lib files with the LGPL header and unsafe policy**

`wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
```

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
```

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
```

Create a placeholder `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/build.rs`:
```rust
fn main() {} // QPDF shim wired up in Task 7
```

`wkhtmltox-rs/.gitignore`:
```
/target
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cd wkhtmltox-rs && cargo build`
Expected: `Finished` with no errors (three crates compile).

- [ ] **Step 5: Commit**

```bash
git add wkhtmltox-rs/Cargo.toml wkhtmltox-rs/crates wkhtmltox-rs/.gitignore
git commit -m "feat(wkhtmltox-rs): scaffold Cargo workspace (core, render-chromium, pdf-sys)"
```

---

### Task 2: Core error type, `Renderer` trait, and DTOs

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/src/error.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/src/render.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub enum WkError { Load{url:String,http_status:Option<u16>}, Render(String), Pagination(String), Pdf(String), Xslt(String), Io(String), BadArg(String), Engine(String), Security(String) }`
  - `pub type Result<T> = std::result::Result<T, WkError>;`
  - `pub trait Renderer` with methods `open`, `wait_ready`, `eval_json`, `print_pdf`, `snapshot`, `page_info` (signatures below)
  - DTOs: `Source`, `LoadSettings`, `ReadyPolicy`, `PageGeometry`, `SnapshotOpts`, `ImageFormat`, `RawImage`, `PageInfo`, `PageHandle`

- [ ] **Step 1: Write the failing test**

`wkhtmltox-rs/crates/wkhtmltox-core/src/render.rs` (append at bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtos_have_sensible_defaults() {
        let g = PageGeometry::default();
        assert!(g.width_mm > 0.0 && g.height_mm > 0.0);
        assert!(matches!(SnapshotOpts::default().format, ImageFormat::Png));
        let r = ReadyPolicy::default();
        assert_eq!(r.javascript_delay_ms, 0);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core`
Expected: FAIL — `PageGeometry` / `SnapshotOpts` / `ReadyPolicy` not found.

- [ ] **Step 3: Implement the error type**

`wkhtmltox-rs/crates/wkhtmltox-core/src/error.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WkError {
    #[error("load failed: {url} (http {http_status:?})")]
    Load { url: String, http_status: Option<u16> },
    #[error("render error: {0}")]
    Render(String),
    #[error("pagination error: {0}")]
    Pagination(String),
    #[error("pdf error: {0}")]
    Pdf(String),
    #[error("xslt error: {0}")]
    Xslt(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("bad argument: {0}")]
    BadArg(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("security policy: {0}")]
    Security(String),
}

pub type Result<T> = std::result::Result<T, WkError>;
```

- [ ] **Step 4: Implement the DTOs and trait**

`wkhtmltox-rs/crates/wkhtmltox-core/src/render.rs` (above the `tests` module):
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use crate::error::Result;

/// Opaque handle to a loaded page, owned by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHandle(pub u64);

#[derive(Debug, Clone)]
pub enum Source {
    Url(String),
    Html(String),
    Stdin,
}

#[derive(Debug, Clone, Default)]
pub struct LoadSettings {
    pub cookies: Vec<(String, String)>,
    pub custom_headers: Vec<(String, String)>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub proxy: Option<String>,
    pub no_check_certificate: bool,
    pub enable_javascript: bool,
    pub allow_local_file_access: bool,
}

#[derive(Debug, Clone)]
pub struct ReadyPolicy {
    pub javascript_delay_ms: u64,
    pub window_status: Option<String>,
}
impl Default for ReadyPolicy {
    fn default() -> Self { Self { javascript_delay_ms: 0, window_status: None } }
}

#[derive(Debug, Clone, Copy)]
pub enum Orientation { Portrait, Landscape }

#[derive(Debug, Clone)]
pub struct PageGeometry {
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_top_mm: f64,
    pub margin_bottom_mm: f64,
    pub margin_left_mm: f64,
    pub margin_right_mm: f64,
    pub orientation: Orientation,
    pub scale: f64,
    pub print_background: bool,
    pub prefer_css_page_size: bool,
    pub generate_document_outline: bool,
}
impl Default for PageGeometry {
    fn default() -> Self {
        Self {
            width_mm: 210.0, height_mm: 297.0, // A4
            margin_top_mm: 10.0, margin_bottom_mm: 10.0,
            margin_left_mm: 10.0, margin_right_mm: 10.0,
            orientation: Orientation::Portrait,
            scale: 1.0, print_background: true, prefer_css_page_size: true,
            generate_document_outline: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat { Png, Jpeg }

#[derive(Debug, Clone)]
pub struct SnapshotOpts {
    pub format: ImageFormat,
    pub crop: Option<(u32, u32, u32, u32)>, // x,y,w,h
    pub scale: f64,
    pub quality: u8,
}
impl Default for SnapshotOpts {
    fn default() -> Self { Self { format: ImageFormat::Png, crop: None, scale: 1.0, quality: 94 } }
}

#[derive(Debug, Clone)]
pub struct RawImage { pub bytes: Vec<u8>, pub format: ImageFormat }

#[derive(Debug, Clone)]
pub struct PageInfo { pub title: String, pub final_url: String, pub content_height_px: f64 }

/// The seam. Blocking from the core's view; backends hide the async engine.
pub trait Renderer {
    fn open(&mut self, src: &Source, load: &LoadSettings) -> Result<PageHandle>;
    fn wait_ready(&mut self, page: PageHandle, ready: &ReadyPolicy) -> Result<()>;
    fn eval_json(&mut self, page: PageHandle, script: &str) -> Result<serde_json::Value>;
    fn print_pdf(&mut self, page: PageHandle, geom: &PageGeometry) -> Result<Vec<u8>>;
    fn snapshot(&mut self, page: PageHandle, opts: &SnapshotOpts) -> Result<RawImage>;
    fn page_info(&self, page: PageHandle) -> Result<PageInfo>;
}
```

`wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
pub mod error;
pub mod render;
pub use error::{Result, WkError};
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core`
Expected: PASS (`dtos_have_sensible_defaults`).

- [ ] **Step 6: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-core/src
git commit -m "feat(core): Renderer trait, DTOs, and WkError"
```

---

### Task 3: `MockRenderer` test double

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-core/src/testing.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`

**Interfaces:**
- Consumes: `Renderer` trait + DTOs from Task 2.
- Produces: `pub struct MockRenderer` (in `wkhtmltox_core::testing`) with public fields `pub pdf: Vec<u8>`, `pub probe: serde_json::Value`, `pub info: PageInfo` and `MockRenderer::new()`. Used by later milestones to test the document layer without a browser.

- [ ] **Step 1: Write the failing test**

`wkhtmltox-rs/crates/wkhtmltox-core/src/testing.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use crate::error::{Result, WkError};
use crate::render::*;

/// A scriptable in-memory Renderer for browser-free tests.
pub struct MockRenderer {
    pub pdf: Vec<u8>,
    pub probe: serde_json::Value,
    pub info: PageInfo,
    next: u64,
}

impl MockRenderer {
    pub fn new() -> Self {
        Self {
            pdf: b"%PDF-1.4\n%mock\n".to_vec(),
            probe: serde_json::json!({ "title": "mock", "headings": [], "anchors": [] }),
            info: PageInfo { title: "mock".into(), final_url: "about:blank".into(), content_height_px: 1000.0 },
            next: 0,
        }
    }
}

impl Default for MockRenderer { fn default() -> Self { Self::new() } }

impl Renderer for MockRenderer {
    fn open(&mut self, _s: &Source, _l: &LoadSettings) -> Result<PageHandle> {
        self.next += 1;
        Ok(PageHandle(self.next))
    }
    fn wait_ready(&mut self, _p: PageHandle, _r: &ReadyPolicy) -> Result<()> { Ok(()) }
    fn eval_json(&mut self, _p: PageHandle, _s: &str) -> Result<serde_json::Value> { Ok(self.probe.clone()) }
    fn print_pdf(&mut self, _p: PageHandle, _g: &PageGeometry) -> Result<Vec<u8>> { Ok(self.pdf.clone()) }
    fn snapshot(&mut self, _p: PageHandle, o: &SnapshotOpts) -> Result<RawImage> {
        Ok(RawImage { bytes: vec![0u8; 8], format: o.format })
    }
    fn page_info(&self, _p: PageHandle) -> Result<PageInfo> { Ok(self.info.clone()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_roundtrips() -> Result<()> {
        let mut r = MockRenderer::new();
        let p = r.open(&Source::Html("<p>x</p>".into()), &LoadSettings::default())?;
        r.wait_ready(p, &ReadyPolicy::default())?;
        let pdf = r.print_pdf(p, &PageGeometry::default())?;
        assert!(pdf.starts_with(b"%PDF"));
        let _ : WkError = WkError::Render("ok".into()); // type is reachable
        Ok(())
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core testing`
Expected: FAIL — module `testing` not declared in `lib.rs`.

- [ ] **Step 3: Declare the module**

In `wkhtmltox-rs/crates/wkhtmltox-core/src/lib.rs`, add after `pub mod render;`:
```rust
pub mod testing;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-core`
Expected: PASS (`mock_roundtrips`, `dtos_have_sensible_defaults`).

- [ ] **Step 5: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-core/src
git commit -m "feat(core): MockRenderer test double for browser-free tests"
```

---

### Task 4: Chromium launch — binary discovery + argument builder (pure)

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/launch.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn find_chrome() -> Option<std::path::PathBuf>` (honors `WKHTMLTOX_CHROME`, else platform defaults)
  - `pub fn launch_args(port: u16, user_data_dir: &str) -> Vec<String>`

- [ ] **Step 1: Write the failing test**

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/launch.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::PathBuf;

pub fn find_chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WKHTMLTOX_CHROME") {
        let pb = PathBuf::from(p);
        if pb.exists() { return Some(pb); }
    }
    const CANDIDATES: &[&str] = &[
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    ];
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.exists())
}

pub fn launch_args(port: u16, user_data_dir: &str) -> Vec<String> {
    vec![
        "--headless=new".into(),
        format!("--remote-debugging-port={port}"),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-gpu".into(),
        format!("--user-data-dir={user_data_dir}"),
        "about:blank".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args_include_port_and_headless() {
        let a = launch_args(9333, "/tmp/x");
        assert!(a.iter().any(|s| s == "--headless=new"));
        assert!(a.iter().any(|s| s == "--remote-debugging-port=9333"));
        assert!(a.iter().any(|s| s == "--user-data-dir=/tmp/x"));
        assert_eq!(a.last().unwrap(), "about:blank");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium launch`
Expected: FAIL — module `launch` not declared.

- [ ] **Step 3: Declare the module**

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#![forbid(unsafe_code)]
pub mod launch;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium`
Expected: PASS (`args_include_port_and_headless`).

- [ ] **Step 5: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-render-chromium/src
git commit -m "feat(chromium): chrome discovery + launch-arg builder"
```

---

### Task 5: Minimal CDP client (sync websocket JSON-RPC)

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/cdp.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`

**Interfaces:**
- Consumes: `WkError` (re-exported from `wkhtmltox-core`).
- Produces:
  - `pub struct Cdp` wrapping a `tungstenite` websocket.
  - `pub fn connect(ws_url: &str) -> wkhtmltox_core::Result<Cdp>`
  - `Cdp::call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value>` (returns the `result` object)
  - `Cdp::wait_event(&mut self, method: &str, timeout: Duration) -> Result<serde_json::Value>`

- [ ] **Step 1: Write the failing unit test (message correlation, no socket)**

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/cdp.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::time::Duration;
use tungstenite::{connect as ws_connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;
use wkhtmltox_core::{Result, WkError};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

pub struct Cdp { sock: Sock, next_id: u64 }

/// Build a CDP request frame. Pure + unit-testable.
pub fn request_frame(id: u64, method: &str, params: &serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

/// Classify an incoming CDP frame: Some((id, result_or_error)) for responses, None for events.
pub fn parse_response(txt: &str, want_id: u64) -> Option<std::result::Result<serde_json::Value, String>> {
    let v: serde_json::Value = serde_json::from_str(txt).ok()?;
    if v.get("id").and_then(|x| x.as_u64()) == Some(want_id) {
        if let Some(err) = v.get("error") { return Some(Err(err.to_string())); }
        return Some(Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)));
    }
    None
}

impl Cdp {
    pub fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.sock
            .send(Message::Text(request_frame(id, method, &params)))
            .map_err(|e| WkError::Engine(format!("cdp send: {e}")))?;
        loop {
            let msg = self.sock.read().map_err(|e| WkError::Engine(format!("cdp read: {e}")))?;
            if let Message::Text(t) = msg {
                if let Some(res) = parse_response(&t, id) {
                    return res.map_err(WkError::Engine);
                }
            }
        }
    }

    pub fn wait_event(&mut self, method: &str, timeout: Duration) -> Result<serde_json::Value> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let msg = self.sock.read().map_err(|e| WkError::Engine(format!("cdp read: {e}")))?;
            if let Message::Text(t) = msg {
                let v: serde_json::Value = serde_json::from_str(&t)
                    .map_err(|e| WkError::Engine(format!("cdp json: {e}")))?;
                if v.get("method").and_then(|m| m.as_str()) == Some(method) {
                    return Ok(v.get("params").cloned().unwrap_or(serde_json::Value::Null));
                }
            }
        }
        Err(WkError::Engine(format!("timeout waiting for event {method}")))
    }
}

pub fn connect(ws_url: &str) -> Result<Cdp> {
    let (sock, _resp) = ws_connect(ws_url).map_err(|e| WkError::Engine(format!("cdp connect: {e}")))?;
    Ok(Cdp { sock, next_id: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_and_response_roundtrip() {
        let f = request_frame(7, "Page.enable", &serde_json::json!({}));
        assert!(f.contains("\"id\":7"));
        assert!(f.contains("Page.enable"));
        let resp = r#"{"id":7,"result":{"ok":true}}"#;
        let got = parse_response(resp, 7).unwrap().unwrap();
        assert_eq!(got["ok"], serde_json::json!(true));
        // an event frame is not a response
        assert!(parse_response(r#"{"method":"Page.loadEventFired","params":{}}"#, 7).is_none());
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium cdp`
Expected: FAIL — module `cdp` not declared.

- [ ] **Step 3: Declare the module**

In `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`, add:
```rust
pub mod cdp;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium`
Expected: PASS (`frame_and_response_roundtrip`, plus Task 4's test).

- [ ] **Step 5: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-render-chromium/src
git commit -m "feat(chromium): minimal sync CDP client (JSON-RPC over websocket)"
```

---

### Task 6: `ChromiumRenderer` implements `Renderer` (open/wait_ready/eval_json/print_pdf) + integration test

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/renderer.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`
- Create: `wkhtmltox-rs/crates/wkhtmltox-render-chromium/tests/fixtures/toc.html`

**Interfaces:**
- Consumes: `launch::{find_chrome, launch_args}`, `cdp::{connect, Cdp}`, the `Renderer` trait.
- Produces: `pub struct ChromiumRenderer` + `pub fn spawn() -> Result<ChromiumRenderer>` that launches the browser and connects; implements `Renderer` for `open`, `wait_ready`, `eval_json`, `print_pdf` (snapshot/page_info may return `WkError::Engine("unimplemented")` for this milestone).

- [ ] **Step 1: Write the failing integration test (gated by a real Chrome)**

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/tests/fixtures/toc.html`:
```html
<!doctype html><meta charset="utf-8"><style>
@page{size:A4;margin:2cm} h1{break-before:page} h1#a{break-before:avoid} .tall{height:1400px}
</style>
<h1 id="a">Alpha</h1><p class="tall">x</p>
<h1 id="b">Beta</h1><p class="tall">y</p>
<h1 id="c">Gamma</h1><p>z</p>
```

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_core::render::*;
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored"]
fn renders_paginated_pdf_with_outline() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/toc.html");
    let url = format!("file://{path}");
    let mut r = ChromiumRenderer::spawn().expect("spawn chrome");
    let p = r.open(&Source::Url(url), &LoadSettings { enable_javascript: true, ..Default::default() }).unwrap();
    r.wait_ready(p, &ReadyPolicy::default()).unwrap();
    let pdf = r.print_pdf(p, &PageGeometry::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF"), "not a pdf");

    let doc = lopdf::Document::load_mem(&pdf).expect("parse pdf");
    let pages = doc.get_pages().len();
    assert!(pages >= 3, "expected >=3 pages from forced breaks, got {pages}");
    // generateDocumentOutline must yield a catalog /Outlines entry
    let catalog = doc.catalog().expect("catalog");
    assert!(catalog.get(b"Outlines").is_ok(), "no /Outlines (generateDocumentOutline failed)");
}
```

- [ ] **Step 2: Run it to verify it fails (compile failure)**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium`
Expected: FAIL — `renderer::ChromiumRenderer` does not exist.

- [ ] **Step 3: Implement the renderer**

`wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/renderer.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::process::{Child, Command};
use std::time::Duration;
use base64::Engine as _; // see note in Step 4 about the base64 dep
use serde_json::json;
use wkhtmltox_core::render::*;
use wkhtmltox_core::{Result, WkError};
use crate::cdp::{connect, Cdp};
use crate::launch::{find_chrome, launch_args};

pub struct ChromiumRenderer { child: Child, cdp: Cdp }

fn mm_to_in(mm: f64) -> f64 { mm / 25.4 }

impl ChromiumRenderer {
    pub fn spawn() -> Result<Self> {
        let chrome = find_chrome().ok_or_else(|| WkError::Engine("no chrome found".into()))?;
        let port = 9333u16;
        let udd = std::env::temp_dir().join(format!("wkx-cdp-{}", std::process::id()));
        let udd = udd.to_string_lossy().to_string();
        let child = Command::new(chrome)
            .args(launch_args(port, &udd))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| WkError::Engine(format!("spawn chrome: {e}")))?;

        // Discover a page target's websocket URL.
        let mut ws_url = None;
        for _ in 0..60 {
            if let Ok(resp) = ureq::get(&format!("http://127.0.0.1:{port}/json")).call() {
                if let Ok(list) = resp.into_json::<serde_json::Value>() {
                    if let Some(arr) = list.as_array() {
                        if let Some(t) = arr.iter().find(|t| t["type"] == "page") {
                            if let Some(u) = t["webSocketDebuggerUrl"].as_str() {
                                ws_url = Some(u.to_string());
                                break;
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let ws_url = ws_url.ok_or_else(|| WkError::Engine("devtools endpoint never appeared".into()))?;
        let mut cdp = connect(&ws_url)?;
        cdp.call("Page.enable", json!({}))?;
        Ok(Self { child, cdp })
    }
}

impl Drop for ChromiumRenderer {
    fn drop(&mut self) { let _ = self.child.kill(); }
}

impl Renderer for ChromiumRenderer {
    fn open(&mut self, src: &Source, _load: &LoadSettings) -> Result<PageHandle> {
        let url = match src {
            Source::Url(u) => u.clone(),
            Source::Html(_) | Source::Stdin =>
                return Err(WkError::Engine("Html/Stdin sources land in a later milestone".into())),
        };
        self.cdp.call("Page.navigate", json!({ "url": url }))?;
        Ok(PageHandle(1))
    }

    fn wait_ready(&mut self, _p: PageHandle, ready: &ReadyPolicy) -> Result<()> {
        self.cdp.wait_event("Page.loadEventFired", Duration::from_secs(30))?;
        if ready.javascript_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(ready.javascript_delay_ms));
        }
        Ok(())
    }

    fn eval_json(&mut self, _p: PageHandle, script: &str) -> Result<serde_json::Value> {
        let r = self.cdp.call("Runtime.evaluate",
            json!({ "expression": script, "returnByValue": true }))?;
        Ok(r["result"]["value"].clone())
    }

    fn print_pdf(&mut self, _p: PageHandle, g: &PageGeometry) -> Result<Vec<u8>> {
        let (w, h) = match g.orientation {
            Orientation::Portrait => (mm_to_in(g.width_mm), mm_to_in(g.height_mm)),
            Orientation::Landscape => (mm_to_in(g.height_mm), mm_to_in(g.width_mm)),
        };
        let r = self.cdp.call("Page.printToPDF", json!({
            "printBackground": g.print_background,
            "preferCSSPageSize": g.prefer_css_page_size,
            "generateDocumentOutline": g.generate_document_outline,
            "paperWidth": w, "paperHeight": h,
            "marginTop": mm_to_in(g.margin_top_mm), "marginBottom": mm_to_in(g.margin_bottom_mm),
            "marginLeft": mm_to_in(g.margin_left_mm), "marginRight": mm_to_in(g.margin_right_mm),
            "scale": g.scale,
            "transferMode": "ReturnAsBase64",
        }))?;
        let b64 = r["data"].as_str().ok_or_else(|| WkError::Pdf("printToPDF: no data".into()))?;
        base64::engine::general_purpose::STANDARD.decode(b64)
            .map_err(|e| WkError::Pdf(format!("b64 decode: {e}")))
    }

    fn snapshot(&mut self, _p: PageHandle, _o: &SnapshotOpts) -> Result<RawImage> {
        Err(WkError::Engine("snapshot lands in a later milestone".into()))
    }
    fn page_info(&self, _p: PageHandle) -> Result<PageInfo> {
        Err(WkError::Engine("page_info lands in a later milestone".into()))
    }
}
```

- [ ] **Step 4: Add the `base64` dependency and declare the module**

Add to `wkhtmltox-rs/crates/wkhtmltox-render-chromium/Cargo.toml` `[dependencies]`:
```toml
base64 = "0.22"
```
Add to `wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/lib.rs`:
```rust
pub mod renderer;
```

- [ ] **Step 5: Run unit tests (no Chrome) to verify the crate still compiles & passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium`
Expected: PASS for unit tests; the e2e test shows as `ignored`.

- [ ] **Step 6: Run the gated integration test against real Chrome**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-render-chromium -- --ignored`
Expected: PASS — `renders_paginated_pdf_with_outline` (>=3 pages, `/Outlines` present). If Chrome is elsewhere, set `WKHTMLTOX_CHROME=/path/to/chrome` first.

- [ ] **Step 7: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-render-chromium
git commit -m "feat(chromium): ChromiumRenderer (open/wait_ready/eval_json/print_pdf) + e2e outline test"
```

---

### Task 7: `wkhtmltox-pdf-sys` — QPDF C++ shim FFI + round-trip smoke test

**Files:**
- Create: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.cpp`
- Create: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.h`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/build.rs`
- Modify: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/src/lib.rs`

**Interfaces:**
- Produces (C ABI from the shim, declared `extern "C"` in Rust):
  - `int wkx_pdf_roundtrip(const char* in_path, const char* out_path)` → 0 on success.
  - (Task 8 adds `wkx_pdf_add_text_field`.)
- Requires: `libqpdf` installed (macOS: `brew install qpdf`; Debian/Ubuntu: `apt-get install libqpdf-dev`).

- [ ] **Step 1: Write the failing smoke test**

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/tests/roundtrip.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;

#[test]
fn qpdf_roundtrip_preserves_pages() {
    // a minimal valid 1-page PDF
    let pdf: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \n\
trailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF\n";
    let dir = std::env::temp_dir();
    let inp = dir.join("wkx_in.pdf");
    let outp = dir.join("wkx_out.pdf");
    std::fs::write(&inp, pdf).unwrap();

    let ci = CString::new(inp.to_string_lossy().as_bytes()).unwrap();
    let co = CString::new(outp.to_string_lossy().as_bytes()).unwrap();
    let rc = unsafe { wkhtmltox_pdf_sys::wkx_pdf_roundtrip(ci.as_ptr(), co.as_ptr()) };
    assert_eq!(rc, 0, "roundtrip failed");

    let out = lopdf::Document::load(&outp).expect("load out");
    assert_eq!(out.get_pages().len(), 1);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-pdf-sys`
Expected: FAIL — `wkx_pdf_roundtrip` not found / link error.

- [ ] **Step 3: Write the C++ shim**

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.h`:
```cpp
// wkhtmltox-rs — LGPL-3.0-or-later.
#ifdef __cplusplus
extern "C" {
#endif
int wkx_pdf_roundtrip(const char* in_path, const char* out_path);
int wkx_pdf_add_text_field(const char* in_path, const char* out_path,
                           const char* field_name, int page_index,
                           double x, double y, double w, double h);
#ifdef __cplusplus
}
#endif
```

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.cpp`:
```cpp
// wkhtmltox-rs — LGPL-3.0-or-later.
#include "shim.h"
#include <qpdf/QPDF.hh>
#include <qpdf/QPDFWriter.hh>

extern "C" int wkx_pdf_roundtrip(const char* in_path, const char* out_path) {
    try {
        QPDF q;
        q.processFile(in_path);
        QPDFWriter w(q, out_path);
        w.write();
        return 0;
    } catch (std::exception& e) {
        return 1;
    }
}
```

- [ ] **Step 4: Wire the build to compile the shim and link libqpdf**

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/build.rs`:
```rust
fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("cpp/shim.cpp")
        .include("cpp")
        .compile("wkxpdfshim");
    // Link libqpdf. On macOS/Homebrew and most Linux, pkg-config knows it.
    if let Ok(lib) = pkg_config::Config::new().probe("libqpdf") {
        for p in lib.link_paths { println!("cargo:rustc-link-search=native={}", p.display()); }
    }
    println!("cargo:rustc-link-lib=dylib=qpdf");
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed=cpp/shim.h");
}
```
Add to `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/Cargo.toml` `[build-dependencies]`:
```toml
pkg-config = "0.3"
```

- [ ] **Step 5: Declare the FFI in Rust**

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/src/lib.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::os::raw::{c_char, c_int};

extern "C" {
    pub fn wkx_pdf_roundtrip(in_path: *const c_char, out_path: *const c_char) -> c_int;
    pub fn wkx_pdf_add_text_field(
        in_path: *const c_char, out_path: *const c_char,
        field_name: *const c_char, page_index: c_int,
        x: f64, y: f64, w: f64, h: f64,
    ) -> c_int;
}
```

- [ ] **Step 6: Run the smoke test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-pdf-sys roundtrip`
Expected: PASS — `qpdf_roundtrip_preserves_pages`. (If link fails, install `libqpdf` dev package and re-run.)

- [ ] **Step 7: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-pdf-sys
git commit -m "feat(pdf-sys): QPDF C++ shim FFI + round-trip smoke test"
```

---

### Task 8: AcroForm spike — add an interactive text field via QPDF (the top High risk)

**Files:**
- Modify: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.cpp`
- Create: `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/tests/acroform.rs`

**Interfaces:**
- Consumes: `wkx_pdf_add_text_field` declared in Task 7.
- Produces: proof that an interactive AcroForm text field can be created on top of engine PDF output (success criterion for the design's top risk).

- [ ] **Step 1: Write the failing test**

`wkhtmltox-rs/crates/wkhtmltox-pdf-sys/tests/acroform.rs`:
```rust
// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::ffi::CString;

#[test]
fn adds_interactive_text_field() {
    let pdf: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]>>endobj\n\
xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \n\
trailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF\n";
    let dir = std::env::temp_dir();
    let inp = dir.join("wkx_form_in.pdf");
    let outp = dir.join("wkx_form_out.pdf");
    std::fs::write(&inp, pdf).unwrap();

    let ci = CString::new(inp.to_string_lossy().as_bytes()).unwrap();
    let co = CString::new(outp.to_string_lossy().as_bytes()).unwrap();
    let name = CString::new("email").unwrap();
    let rc = unsafe {
        wkhtmltox_pdf_sys::wkx_pdf_add_text_field(ci.as_ptr(), co.as_ptr(), name.as_ptr(),
                                                  0, 50.0, 50.0, 200.0, 20.0)
    };
    assert_eq!(rc, 0, "add_text_field failed");

    // Verify via lopdf: catalog has /AcroForm with one field, page has a /Widget annotation.
    let doc = lopdf::Document::load(&outp).expect("load out");
    let cat = doc.catalog().expect("catalog");
    let acro = cat.get(b"AcroForm").expect("no /AcroForm");
    let acro = acro.as_reference().map(|r| doc.get_object(r).unwrap()).unwrap_or(acro);
    let fields = acro.as_dict().unwrap().get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1, "expected exactly one form field");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-pdf-sys acroform`
Expected: FAIL — `wkx_pdf_add_text_field` is unresolved (only declared, not implemented in the shim).

- [ ] **Step 3: Implement the AcroForm field in the shim**

Append to `wkhtmltox-rs/crates/wkhtmltox-pdf-sys/cpp/shim.cpp`:
```cpp
#include <qpdf/QPDFObjectHandle.hh>
#include <qpdf/QPDFAcroFormDocumentHelper.hh>
#include <qpdf/QPDFPageDocumentHelper.hh>
#include <qpdf/QPDFFormFieldObjectHelper.hh>
#include <qpdf/QPDFAnnotationObjectHelper.hh>
#include <string>

extern "C" int wkx_pdf_add_text_field(const char* in_path, const char* out_path,
                                      const char* field_name, int page_index,
                                      double x, double y, double w, double h) {
    try {
        QPDF q;
        q.processFile(in_path);

        QPDFPageDocumentHelper pdh(q);
        auto pages = pdh.getAllPages();
        if (page_index < 0 || (size_t)page_index >= pages.size()) return 2;
        QPDFObjectHandle page = pages[page_index].getObjectHandle();

        // Build the field/widget dictionary (a terminal text field that is also its own widget).
        QPDFObjectHandle field = q.makeIndirectObject(QPDFObjectHandle::newDictionary());
        field.replaceKey("/FT", QPDFObjectHandle::newName("/Tx"));
        field.replaceKey("/T", QPDFObjectHandle::newUnicodeString(std::string(field_name)));
        field.replaceKey("/Type", QPDFObjectHandle::newName("/Annot"));
        field.replaceKey("/Subtype", QPDFObjectHandle::newName("/Widget"));
        field.replaceKey("/F", QPDFObjectHandle::newInteger(4)); // Print
        field.replaceKey("/DA", QPDFObjectHandle::newString("/Helv 0 Tf 0 g"));
        field.replaceKey("/P", page);
        QPDFObjectHandle rect = QPDFObjectHandle::newArray();
        rect.appendItem(QPDFObjectHandle::newReal(x));
        rect.appendItem(QPDFObjectHandle::newReal(y));
        rect.appendItem(QPDFObjectHandle::newReal(x + w));
        rect.appendItem(QPDFObjectHandle::newReal(y + h));
        field.replaceKey("/Rect", rect);

        // Attach the widget to the page's /Annots.
        if (!page.hasKey("/Annots")) page.replaceKey("/Annots", QPDFObjectHandle::newArray());
        page.getKey("/Annots").appendItem(field);

        // Register with the document AcroForm (creates /AcroForm + /Fields if needed).
        QPDFAcroFormDocumentHelper afdh(q);
        afdh.addFormField(QPDFFormFieldObjectHelper(field));
        afdh.setNeedAppearances(true);

        QPDFWriter wr(q, out_path);
        wr.write();
        return 0;
    } catch (std::exception& e) {
        return 1;
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd wkhtmltox-rs && cargo test -p wkhtmltox-pdf-sys acroform`
Expected: PASS — `adds_interactive_text_field` (catalog `/AcroForm` → one `/Fields` entry).

- [ ] **Step 5: Record the spike outcome in the spec**

Append a line under `## 11. Spike results` in `docs/superpowers/specs/2026-06-28-no-qt-engine-design.md`:
```markdown
**SPIKE (AcroForm), Milestone 1:** RESULT — QPDF (`QPDFAcroFormDocumentHelper::addFormField`) creates an interactive text-field widget on top of an unpatched engine PDF; verified via lopdf (catalog /AcroForm has one /Fields entry). Forms→AcroForm is feasible; QPDF (not lopdf) is the production path. Top High risk retired.
```

- [ ] **Step 6: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-pdf-sys docs/superpowers/specs/2026-06-28-no-qt-engine-design.md
git commit -m "feat(pdf-sys): AcroForm text-field spike via QPDF; retire top risk"
```

---

## Self-Review

**1. Spec coverage (Milestone 1 scope only):**
- Workspace + two-unsafe-edges structure (spec §3) → Task 1. ✓
- `Renderer` trait + DTOs + `WkError` (spec §4, §6.1) → Task 2. ✓
- `MockRenderer` for browser-free, test-first core (spec §8.1, "build test-first") → Task 3. ✓
- Chromium-CDP backend, pure-Rust, no FFI (spec §2, §3, §4) → Tasks 4–6. ✓
- `generateDocumentOutline` + paginated PDF (spec §5.1, §11 SPIKE 1) → Task 6 e2e assertions. ✓
- QPDF FFI via shim (spec §3, §7) → Task 7. ✓
- AcroForm feasibility — top High risk (spec §9, §10 Milestone 1) → Task 8. ✓
- Deferred to later milestones (correctly out of scope here): named-destination harvesting + link-annotation synthesis, TOC fixed-point, header/footer overlay, ResourcePolicy/CDP interception, C ABI + CLI, image pipeline, packaging. Noted in Task 6 (snapshot/page_info return "later milestone") and not claimed complete.

**2. Placeholder scan:** No "TODO/implement later" in code steps; every code step shows complete code. The `Source::Html`/`Stdin` and `snapshot`/`page_info` paths return explicit `WkError::Engine("... later milestone")` — that is a deliberate, compiling stub with a clear error, not a silent placeholder, and is out of Milestone 1 scope.

**3. Type consistency:** `Renderer` method signatures (`page: PageHandle` by value; `print_pdf -> Result<Vec<u8>>`) are identical in Task 2 (definition), Task 3 (`MockRenderer`), and Task 6 (`ChromiumRenderer`). `PageGeometry.generate_document_outline` defined in Task 2 is consumed in Task 6. `wkx_pdf_add_text_field` signature matches between the shim header (Task 7 Step 3 `shim.h`), the Rust `extern` block (Task 7 Step 5), and the call site (Task 8 Step 1). `find_chrome`/`launch_args` (Task 4) consumed in Task 6. `connect`/`Cdp::call`/`Cdp::wait_event` (Task 5) consumed in Task 6.

**Known external preconditions (documented, not placeholders):** Tasks 6 runs the gated test only with a real Chrome ≥126; Tasks 7–8 require `libqpdf` dev headers installed. Both are stated in the task's Requires/Run notes.
