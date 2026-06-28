// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use base64::Engine as _; // see note in Step 4 about the base64 dep
use serde_json::json;
use wkhtmltox_core::render::*;
use wkhtmltox_core::{Result, WkError};
use crate::cdp::{connect, Cdp};
use crate::launch::{find_chrome, launch_args};

/// Cleanup guard held during Chrome discovery/connect.
/// Kills+waits the child and removes the user-data-dir on drop.
/// Call `disarm()` on success to transfer ownership to `ChromiumRenderer`.
struct SpawnGuard {
    inner: Option<(Child, PathBuf)>,
}

impl SpawnGuard {
    fn new(child: Child, udd: PathBuf) -> Self {
        Self { inner: Some((child, udd)) }
    }
    /// Consume the guard without triggering cleanup; returns the owned fields.
    fn disarm(mut self) -> (Child, PathBuf) {
        self.inner.take().unwrap()
        // self drops here; inner is None so Drop is a no-op
    }
}

impl Drop for SpawnGuard {
    fn drop(&mut self) {
        if let Some((mut c, udd)) = self.inner.take() {
            let _ = c.kill();
            let _ = c.wait();
            let _ = std::fs::remove_dir_all(&udd);
        }
    }
}

pub struct ChromiumRenderer {
    child: Child,
    user_data_dir: PathBuf,
    cdp: Cdp,
}

fn mm_to_in(mm: f64) -> f64 { mm / 25.4 }

impl ChromiumRenderer {
    pub fn spawn() -> Result<Self> {
        let chrome = find_chrome().ok_or_else(|| WkError::Engine("no chrome found".into()))?;
        let udd = std::env::temp_dir().join(format!("wkx-cdp-{}", std::process::id()));
        let udd_str = udd.to_string_lossy().to_string();

        // Use port=0 so the OS assigns a free ephemeral port.
        // Chrome writes the actual bound port to <user_data_dir>/DevToolsActivePort.
        let child = Command::new(chrome)
            .args(launch_args(0, &udd_str))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| WkError::Engine(format!("spawn chrome: {e}")))?;

        // Guard ensures kill+wait+udd removal if anything below fails.
        let guard = SpawnGuard::new(child, udd.clone());

        // Poll DevToolsActivePort until Chrome has bound its debugging socket.
        let dap_file = udd.join("DevToolsActivePort");
        let mut port: Option<u16> = None;
        for _ in 0..150 {
            if let Ok(contents) = std::fs::read_to_string(&dap_file) {
                let line = contents.lines().next().unwrap_or("").trim();
                if let Ok(p) = line.parse::<u16>() {
                    port = Some(p);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let port = port.ok_or_else(|| WkError::Engine("DevToolsActivePort never appeared".into()))?;

        // Discover a page target's websocket URL via the /json endpoint.
        let mut ws_url = None;
        for _ in 0..60 {
            if let Ok(resp) = ureq::get(&format!("http://127.0.0.1:{port}/json")).call() {
                if let Ok(list) = serde_json::from_reader::<_, serde_json::Value>(resp.into_reader()) {
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

        // All succeeded — disarm the guard and hand ownership to ChromiumRenderer.
        let (child, user_data_dir) = guard.disarm();
        Ok(Self { child, user_data_dir, cdp })
    }
}

impl Drop for ChromiumRenderer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
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
        // Poll document.readyState until "complete", bounded by a 30 s deadline.
        // This avoids the race where Page.loadEventFired fires during navigate's
        // call() loop and is silently discarded as a non-matching id.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let val = self.cdp.call(
                "Runtime.evaluate",
                json!({ "expression": "document.readyState", "returnByValue": true }),
            )?;
            if val["result"]["value"].as_str() == Some("complete") {
                break;
            }
            if Instant::now() >= deadline {
                return Err(WkError::Engine("timeout waiting for readyState=complete".into()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        if ready.javascript_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(ready.javascript_delay_ms));
        }

        if let Some(wanted) = &ready.window_status {
            let deadline2 = Instant::now() + Duration::from_secs(30);
            loop {
                let val = self.cdp.call(
                    "Runtime.evaluate",
                    json!({ "expression": "window.status", "returnByValue": true }),
                )?;
                if val["result"]["value"].as_str() == Some(wanted.as_str()) {
                    break;
                }
                if Instant::now() >= deadline2 {
                    return Err(WkError::Engine(
                        format!("timeout waiting for window.status={wanted:?}")
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
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
