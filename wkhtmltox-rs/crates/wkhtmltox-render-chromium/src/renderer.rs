// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use base64::Engine as _; // see note in Step 4 about the base64 dep
use serde_json::json;
use wkhtmltox_core::policy::ResourcePolicy;
use wkhtmltox_core::render::*;
use wkhtmltox_core::{Result, WkError};
use crate::cdp::{connect, Cdp};
use crate::launch::{find_chrome, launch_args};

/// Options for spawning a `ChromiumRenderer`.
///
/// Use `SpawnOpts::default()` for a default (no-proxy) renderer, or build
/// `SpawnOpts { proxy: Some("http://proxy:8080".into()), ..Default::default() }`
/// to route all renderer traffic through a proxy.
#[derive(Debug, Default)]
pub struct SpawnOpts {
    /// HTTP/SOCKS proxy URL passed to Chrome as `--proxy-server=<proxy>`.
    /// `None` = no proxy (system default).
    pub proxy: Option<String>,
}

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
    /// Compat CSS stored at `open()` time from `LoadSettings::compat_ua_css`.
    compat_ua_css: Option<String>,
    /// Keeps the temp HTML file alive for the duration of the current page.
    ///
    /// When [`Source::Html`] is passed to [`open`], the HTML is written to a
    /// `NamedTempFile` so Chrome can load it via a `file://` URL.  The file
    /// must not be deleted until Chrome has finished reading it (i.e. after
    /// [`print_pdf`] returns), so we hold the guard here.  It is replaced on
    /// every subsequent call to `open()` and cleared when a [`Source::Url`]
    /// is opened.
    _html_temp: Option<tempfile::NamedTempFile>,
    /// URLs that were blocked by the [`ResourcePolicy`] during the most recent
    /// `open()` call.  Cleared at the start of each `open()`.
    blocked_urls: Vec<String>,
}

impl ChromiumRenderer {
    /// Returns the list of URLs blocked by the `ResourcePolicy` during the
    /// most recent [`open()`] call.
    ///
    /// The list is cleared at the start of each new `open()` call.
    pub fn blocked_urls(&self) -> &[String] {
        &self.blocked_urls
    }
}

fn mm_to_in(mm: f64) -> f64 { mm / 25.4 }

// ---------------------------------------------------------------------------
// ResourcePolicy enforcement helpers
// ---------------------------------------------------------------------------

/// Decision for a single CDP `Fetch.requestPaused` event.
///
/// Factored out of the event pump so it can be unit-tested without a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchDecision {
    /// Allow the request to continue via `Fetch.continueRequest`.
    Continue,
    /// Abort the request via `Fetch.failRequest { errorReason: "Aborted" }`.
    Fail,
}

/// Map a `ResourcePolicy` decision to a `FetchDecision`.
///
/// Pure helper — no I/O, no CDP calls.  Tests call this directly to verify
/// the policy-to-action mapping without needing a real browser.
pub fn fetch_action(policy: &ResourcePolicy, url: &str, is_redirect: bool) -> FetchDecision {
    use wkhtmltox_core::policy::Decision;
    match policy.decide(url, is_redirect) {
        Decision::Allow => FetchDecision::Continue,
        Decision::Block(_) => FetchDecision::Fail,
    }
}

/// Build `Network.setCookies` params with the target URL as the cookie scope.
///
/// Pure function — no CDP calls; used by `open` and testable without a browser.
pub fn build_cookies_params(cookies: &[(String, String)], url: &str) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = cookies
        .iter()
        .map(|(n, v)| json!({ "name": n, "value": v, "url": url }))
        .collect();
    json!({ "cookies": arr })
}

/// Build `Network.setExtraHTTPHeaders` params from custom headers + optional Basic auth.
///
/// Basic auth is injected as an `Authorization: Basic <b64>` header when both
/// `username` and `password` are `Some`.  Returns `None` when both lists are
/// empty (caller skips the CDP call).
///
/// Pure function — no CDP calls; used by `open` and testable without a browser.
pub fn build_extra_headers_params(
    custom_headers: &[(String, String)],
    username: Option<&str>,
    password: Option<&str>,
) -> Option<serde_json::Value> {
    let mut map: serde_json::Map<String, serde_json::Value> = custom_headers
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    if let (Some(u), Some(p)) = (username, password) {
        let creds = format!("{u}:{p}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
        map.insert(
            "Authorization".into(),
            serde_json::Value::String(format!("Basic {encoded}")),
        );
    }
    if map.is_empty() {
        None
    } else {
        Some(json!({ "headers": serde_json::Value::Object(map) }))
    }
}

impl ChromiumRenderer {
    pub fn spawn() -> Result<Self> {
        Self::spawn_opts(SpawnOpts::default())
    }


    pub fn spawn_opts(opts: SpawnOpts) -> Result<Self> {
        let chrome = find_chrome().ok_or_else(|| WkError::Engine("no chrome found".into()))?;
        let udd = std::env::temp_dir().join(format!("wkx-cdp-{}", std::process::id()));
        let udd_str = udd.to_string_lossy().to_string();

        // Use port=0 so the OS assigns a free ephemeral port.
        // Chrome writes the actual bound port to <user_data_dir>/DevToolsActivePort.
        let child = Command::new(chrome)
            .args(launch_args(0, &udd_str, opts.proxy.as_deref()))
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
        Ok(Self { child, user_data_dir, cdp, compat_ua_css: None, _html_temp: None, blocked_urls: Vec::new() })
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
    fn open(&mut self, src: &Source, load: &LoadSettings) -> Result<PageHandle> {
        let url = match src {
            Source::Url(u) => {
                // Release any previously held HTML temp file — it is no longer
                // needed now that we are navigating to a URL.
                self._html_temp = None;
                u.clone()
            }
            Source::Html(html) => {
                // Write the HTML to a NamedTempFile with a `.html` extension so
                // that Chrome applies the correct MIME type when loading it via
                // the `file://` scheme.  The file guard is stored in `_html_temp`
                // and lives until the next call to `open()` (or until the
                // renderer is dropped), ensuring Chrome can finish reading it.
                let mut tmp = tempfile::Builder::new()
                    .prefix("wkx-html-")
                    .suffix(".html")
                    .tempfile()
                    .map_err(|e| WkError::Io(format!("create html temp file: {e}")))?;
                tmp.write_all(html.as_bytes())
                    .map_err(|e| WkError::Io(format!("write html temp file: {e}")))?;
                tmp.flush()
                    .map_err(|e| WkError::Io(format!("flush html temp file: {e}")))?;
                let path = tmp.path().to_path_buf();
                let file_url = format!("file://{}", path.display());
                // Keep the guard alive so the file is not deleted before Chrome loads it.
                self._html_temp = Some(tmp);
                file_url
            }
            Source::Stdin =>
                return Err(WkError::Engine("Stdin source not supported by ChromiumRenderer".into())),
        };

        // Clear blocked-URLs from any previous open() call.
        self.blocked_urls.clear();

        // Persist compat CSS so wait_ready() can inject it after the page loads.
        self.compat_ua_css = load.compat_ua_css.clone();

        // ── Apply networking settings via CDP ─────────────────────────────────
        self.cdp.call("Network.enable", json!({}))?;

        // Cookies require an http(s) origin; skip for file://, data:, etc.
        if !load.cookies.is_empty()
            && (url.starts_with("http://") || url.starts_with("https://"))
        {
            let params = build_cookies_params(&load.cookies, &url);
            self.cdp.call("Network.setCookies", params)?;
        }

        if let Some(headers_params) = build_extra_headers_params(
            &load.custom_headers,
            load.username.as_deref(),
            load.password.as_deref(),
        ) {
            self.cdp.call("Network.setExtraHTTPHeaders", headers_params)?;
        }

        if load.no_check_certificate {
            self.cdp.call("Security.enable", json!({}))?;
            self.cdp.call("Security.setIgnoreCertificateErrors", json!({ "ignore": true }))?;
        }

        if !load.enable_javascript {
            self.cdp.call("Emulation.setScriptExecutionDisabled", json!({ "value": true }))?;
        }

        // ── Enable Fetch interception for ResourcePolicy enforcement ──────────
        //
        // `Fetch.enable` pauses every matching request until the debugger
        // responds with continueRequest or failRequest.  We enable it
        // unconditionally so the enforcement path is always exercised (simplifies
        // reasoning about correctness) and disable it again after loadEventFired.
        //
        // IMPORTANT: Because Fetch.enable pauses requests, the old "poll
        // document.readyState" approach no longer works for detecting load
        // completion — the page can never reach readyState=complete while its
        // requests are paused.  We instead wait for Page.loadEventFired in the
        // event pump below and then disable Fetch before handing off to
        // wait_ready().
        self.cdp.call("Fetch.enable", json!({
            "patterns": [{ "urlPattern": "*" }],
            "handleAuthRequests": false
        }))?;

        // CRITICAL: Use send_only for Page.navigate, NOT call().
        //
        // When Fetch.enable is active, Chrome intercepts the navigation request
        // itself via Fetch.requestPaused BEFORE sending the Page.navigate
        // response.  Using call() here causes a deadlock:
        //   - call() blocks waiting for the Page.navigate response
        //   - Chrome waits for us to send continueRequest/failRequest first
        //   - Neither can proceed → 60-second timeout
        //
        // send_only() fires the command and returns immediately.  The navigate
        // response arrives later in the event pump as a regular message with
        // an `id` field (not a `method` field) and is silently ignored — we
        // use Page.loadEventFired as the completion signal instead.
        self.cdp.send_only("Page.navigate", json!({ "url": url }))?;

        // ── Event pump ────────────────────────────────────────────────────────
        //
        // Read CDP messages until Page.loadEventFired or the 60 s deadline.
        // For each Fetch.requestPaused event:
        //   Allow → Fetch.continueRequest (fire-and-forget via send_only)
        //   Block → Fetch.failRequest + record URL in blocked_urls
        //
        // After Page.loadEventFired we continue the loop for up to 200 ms to
        // drain any Fetch.requestPaused events that Chrome had already queued.
        // The drain exits when there are no messages for 200 ms; command
        // responses to our continueRequest calls (with `id` but no `method`)
        // are read and silently discarded, allowing the idle window to fill.
        let load_deadline = Instant::now() + Duration::from_secs(60);
        let mut page_loaded = false;
        'pump: loop {
            if Instant::now() >= load_deadline {
                // Disable Fetch before returning so subsequent calls aren't stuck.
                let _ = self.cdp.send_only("Fetch.disable", json!({}));
                return Err(WkError::Engine(
                    "timeout waiting for Page.loadEventFired".into()
                ));
            }

            // After the page has loaded, use a short drain timeout so we exit
            // promptly once there are no more pending Fetch events.
            let poll_ms = if page_loaded { 200 } else { 250 };
            let Some(msg) = self.cdp.read_message(Duration::from_millis(poll_ms)) else {
                if page_loaded {
                    // No message for 200 ms after loadEventFired — all pending
                    // Fetch.requestPaused events have been handled.
                    break 'pump;
                }
                continue 'pump;
            };

            match msg.get("method").and_then(|m| m.as_str()) {
                Some("Fetch.requestPaused") => {
                    let params = &msg["params"];
                    let request_id = params["requestId"].as_str().unwrap_or("").to_string();
                    let req_url = params["request"]["url"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    // A paused response (with responseStatusCode) is a redirect.
                    let is_redirect = params
                        .get("responseStatusCode")
                        .and_then(|c| c.as_u64())
                        .map(|code| (300..=399).contains(&code))
                        .unwrap_or(false);

                    match fetch_action(&load.policy, &req_url, is_redirect) {
                        FetchDecision::Continue => {
                            self.cdp.send_only(
                                "Fetch.continueRequest",
                                json!({ "requestId": request_id }),
                            )?;
                        }
                        FetchDecision::Fail => {
                            self.blocked_urls.push(req_url);
                            self.cdp.send_only(
                                "Fetch.failRequest",
                                json!({
                                    "requestId": request_id,
                                    "errorReason": "Aborted"
                                }),
                            )?;
                        }
                    }
                }
                Some("Page.loadEventFired") => {
                    page_loaded = true;
                    // Continue the loop to drain any remaining buffered
                    // Fetch.requestPaused events (see comment above).
                }
                _ => {} // Ignore command responses (id-keyed) and other events.
            }
        }

        // Disable Fetch interception so wait_ready()'s Runtime.evaluate calls
        // are not blocked by paused requests triggered by page JavaScript.
        // call() is used here (not send_only) so we wait for Chrome to confirm
        // the disable — any Fetch.requestPaused events arriving during this
        // call are buffered in msg_buf and handled in the post-disable drain.
        let _ = self.cdp.call("Fetch.disable", json!({}));

        // Post-disable drain: respond to any Fetch.requestPaused events that
        // Chrome sent between loadEventFired and our Fetch.disable command.
        // Chrome may not auto-resume these; ignoring them risks a hang.
        {
            let buffered = self.cdp.drain_buffer();
            for msg in buffered {
                if msg.get("method").and_then(|m| m.as_str()) != Some("Fetch.requestPaused") {
                    continue;
                }
                let params = &msg["params"];
                let request_id = params["requestId"].as_str().unwrap_or("").to_string();
                let req_url = params["request"]["url"].as_str().unwrap_or("").to_string();
                let is_redirect = params
                    .get("responseStatusCode")
                    .and_then(|c| c.as_u64())
                    .map(|code| (300..=399).contains(&code))
                    .unwrap_or(false);
                match fetch_action(&load.policy, &req_url, is_redirect) {
                    FetchDecision::Continue => {
                        let _ = self.cdp.send_only(
                            "Fetch.continueRequest",
                            json!({ "requestId": request_id }),
                        );
                    }
                    FetchDecision::Fail => {
                        self.blocked_urls.push(req_url);
                        let _ = self.cdp.send_only(
                            "Fetch.failRequest",
                            json!({ "requestId": request_id, "errorReason": "Aborted" }),
                        );
                    }
                }
            }
        }

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

        // Inject compat UA-reset stylesheet if configured.
        if let Some(css) = &self.compat_ua_css.clone() {
            // JSON-encode the CSS string so any quotes/backticks are safely escaped.
            let css_json = serde_json::to_string(css)
                .map_err(|e| WkError::Engine(format!("compat css json encode: {e}")))?;
            let js = format!(
                r#"(function(){{
  var existing = document.getElementById('__wkx_compat');
  if (existing) {{ existing.parentNode.removeChild(existing); }}
  var s = document.createElement('style');
  s.id = '__wkx_compat';
  s.textContent = {css_json};
  (document.head || document.documentElement).appendChild(s);
}})();"#
            );
            self.cdp.call(
                "Runtime.evaluate",
                json!({ "expression": js, "returnByValue": true }),
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use wkhtmltox_core::policy::ResourcePolicy;

    // ── fetch_action pure unit tests ─────────────────────────────────────────

    #[test]
    fn fetch_action_allow_maps_to_continue_default() {
        let policy = ResourcePolicy::default();
        // Default is permissive: everything is Continue.
        assert_eq!(
            fetch_action(&policy, "https://example.com/page.js", false),
            FetchDecision::Continue,
        );
        assert_eq!(
            fetch_action(&policy, "file:///etc/hosts", false),
            FetchDecision::Continue,
            "default policy must not block file://"
        );
        assert_eq!(
            fetch_action(&policy, "http://127.0.0.1/internal", false),
            FetchDecision::Continue,
            "default policy must not block private IPs"
        );
    }

    #[test]
    fn fetch_action_block_file_under_safe() {
        let policy = ResourcePolicy::safe_profile();
        assert_eq!(
            fetch_action(&policy, "file:///etc/hosts", false),
            FetchDecision::Fail,
            "safe profile must block file://"
        );
        assert_eq!(
            fetch_action(&policy, "file:///etc/passwd", false),
            FetchDecision::Fail,
        );
    }

    #[test]
    fn fetch_action_block_private_ip_under_safe() {
        let policy = ResourcePolicy::safe_profile();
        assert_eq!(
            fetch_action(&policy, "http://127.0.0.1/", false),
            FetchDecision::Fail,
            "safe profile must block loopback IP"
        );
        assert_eq!(
            fetch_action(&policy, "http://169.254.169.254/latest/meta-data/", false),
            FetchDecision::Fail,
            "safe profile must block link-local IMDS"
        );
        assert_eq!(
            fetch_action(&policy, "http://10.0.0.5/internal", false),
            FetchDecision::Fail,
            "safe profile must block 10/8"
        );
    }

    #[test]
    fn fetch_action_safe_allows_public_https() {
        let policy = ResourcePolicy::safe_profile();
        assert_eq!(
            fetch_action(&policy, "https://example.com/style.css", false),
            FetchDecision::Continue,
            "safe profile must allow public HTTPS"
        );
    }

    #[test]
    fn fetch_action_redirect_target_re_validated_under_safe() {
        let policy = ResourcePolicy::safe_profile();
        // Redirect to file:// must be blocked even with is_redirect=true.
        assert_eq!(
            fetch_action(&policy, "file:///etc/passwd", true),
            FetchDecision::Fail,
            "redirect to file:// must be blocked under safe"
        );
        // Redirect to private IP is also blocked.
        assert_eq!(
            fetch_action(&policy, "http://127.0.0.1/secret", true),
            FetchDecision::Fail,
            "redirect to loopback must be blocked under safe"
        );
    }

    #[test]
    fn fetch_action_data_uri_always_allowed() {
        let policy = ResourcePolicy::safe_profile();
        assert_eq!(
            fetch_action(&policy, "data:text/html,<h1>hi</h1>", false),
            FetchDecision::Continue,
            "data: URIs are always allowed"
        );
    }

    // ── Existing CDP / builder tests ─────────────────────────────────────────

    #[test]
    fn cookies_params_json_shape() {
        let cookies = vec![
            ("session".to_string(), "abc123".to_string()),
            ("lang".to_string(), "en".to_string()),
        ];
        let p = build_cookies_params(&cookies, "https://example.com/");
        let arr = p["cookies"].as_array().expect("cookies must be an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "session");
        assert_eq!(arr[0]["value"], "abc123");
        assert_eq!(arr[0]["url"], "https://example.com/");
        assert_eq!(arr[1]["name"], "lang");
    }

    #[test]
    fn extra_headers_params_custom_and_auth() {
        let headers = vec![("X-Custom".to_string(), "header-val".to_string())];
        let p = build_extra_headers_params(&headers, Some("alice"), Some("s3cr3t"))
            .expect("should produce Some when headers+auth present");
        let hdrs = &p["headers"];
        assert_eq!(hdrs["X-Custom"].as_str().unwrap(), "header-val");
        let auth = hdrs["Authorization"].as_str().expect("Authorization must be present");
        assert!(auth.starts_with("Basic "), "auth header must start with 'Basic '");
        let b64 = auth.strip_prefix("Basic ").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("must be valid base64");
        assert_eq!(decoded, b"alice:s3cr3t");
    }

    #[test]
    fn extra_headers_params_none_when_empty() {
        let result = build_extra_headers_params(&[], None, None);
        assert!(result.is_none(), "must return None when there is nothing to set");
    }

    #[test]
    fn extra_headers_params_no_auth_when_credentials_missing() {
        let headers = vec![("X-Foo".to_string(), "bar".to_string())];
        let p = build_extra_headers_params(&headers, Some("user"), None)
            .expect("should be Some with custom header present");
        assert!(
            p["headers"]["Authorization"].is_null(),
            "Authorization must be absent when password is None"
        );
        assert_eq!(p["headers"]["X-Foo"].as_str().unwrap(), "bar");
    }

    #[test]
    fn extra_headers_params_auth_only() {
        let p = build_extra_headers_params(&[], Some("u"), Some("p"))
            .expect("should be Some when auth is present");
        let auth = p["headers"]["Authorization"].as_str().unwrap();
        assert!(auth.starts_with("Basic "));
    }
}
