// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Monotonically increasing counter so each `ChromiumRenderer::spawn` within
/// the same process gets a unique `user-data-dir` path.  Without this, rapid
/// back-to-back spawns reuse the same path; the second Chrome can pick up stale
/// lock files from the still-terminating first instance.
static SPAWN_COUNTER: AtomicU64 = AtomicU64::new(0);
use crate::cdp::{connect, Cdp};
use crate::launch::{find_chrome, launch_args};
use base64::Engine as _; // see note in Step 4 about the base64 dep
use serde_json::json;
use wkhtmltox_core::policy::ResourcePolicy;
use wkhtmltox_core::render::*;
use wkhtmltox_core::{Result, WkError};

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
        Self {
            inner: Some((child, udd)),
        }
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

/// State maintained while Fetch interception is active for the current page.
///
/// Set by `open()`, cleared (and Fetch disabled) after `print_pdf()` or at
/// the start of the next `open()`.  When `fetch_state` is `Some`, every CDP
/// round-trip that could trigger subresource loads MUST go through
/// `call_pumping` so that `Fetch.requestPaused` events are serviced inline.
struct ActiveFetchState {
    /// The URL we navigated to.  Top-level document requests whose URL matches
    /// this value are allowed unconditionally (C1b).
    nav_url: String,
    /// Resource policy governing subresource decisions.
    policy: ResourcePolicy,
    /// Extra HTTP headers (custom + Basic-auth) to inject on `continueRequest`
    /// ONLY when the request's origin matches `nav_url`'s origin (I3).
    extra_headers: Vec<(String, String)>,
    /// When `false`, all Image-type resources are blocked before policy
    /// evaluation (fail-safe: never promotes a policy Block to an Allow).
    load_images: bool,
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
    /// Active Fetch interception state.  `None` when Fetch is disabled.
    fetch_state: Option<ActiveFetchState>,
    /// Device metrics stored at `open()` time so `wait_ready()` can apply
    /// smart-width expansion after the page has loaded.
    device_metrics: Option<wkhtmltox_core::render::DeviceMetrics>,
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

fn mm_to_in(mm: f64) -> f64 {
    mm / 25.4
}

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
///
/// NOTE (I3): This function is kept for tests and future callers but is no
/// longer used to issue a global `Network.setExtraHTTPHeaders` call.  Headers
/// are now injected per-request via `Fetch.continueRequest` only for same-origin
/// requests, preventing cross-origin credential leaks.
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

// ---------------------------------------------------------------------------
// Per-origin header injection (I3)
// ---------------------------------------------------------------------------

/// Extract `scheme://authority` from a URL for same-origin comparison.
///
/// Returns `None` for URLs without a `//` authority (e.g. `file:///path`
/// where authority is empty, `data:`, or malformed URLs).
fn extract_origin(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let scheme = url[..colon].to_ascii_lowercase();
    let rest = &url[colon + 1..];
    let after_slashes = rest.strip_prefix("//")?;
    let auth_end = after_slashes
        .find(['/', '?', '#'])
        .unwrap_or(after_slashes.len());
    Some(format!("{}://{}", scheme, &after_slashes[..auth_end]))
}

/// Return `true` when both URLs share the same scheme + authority.
fn same_origin(url1: &str, url2: &str) -> bool {
    match (extract_origin(url1), extract_origin(url2)) {
        (Some(o1), Some(o2)) => o1 == o2,
        _ => false,
    }
}

/// Build `Fetch.continueRequest` params, injecting `extra_headers` when the
/// request is same-origin with `nav_url`.
///
/// When extra headers are injected the original request headers (from the
/// `Fetch.requestPaused` event) are preserved and the extra headers are merged
/// in, so Chrome sends all headers the page would have sent plus ours.
fn build_continue_params(
    request_id: &str,
    req_url: &str,
    original_headers: &serde_json::Value,
    nav_url: &str,
    extra_headers: &[(String, String)],
) -> serde_json::Value {
    if !extra_headers.is_empty() && same_origin(req_url, nav_url) {
        let mut headers = original_headers.as_object().cloned().unwrap_or_default();
        for (name, value) in extra_headers {
            headers.insert(name.clone(), serde_json::Value::String(value.clone()));
        }
        json!({
            "requestId": request_id,
            "headers": serde_json::Value::Object(headers),
        })
    } else {
        json!({ "requestId": request_id })
    }
}

// ---------------------------------------------------------------------------
// Fetch event handler (C1, C1b, I3)
// ---------------------------------------------------------------------------

/// Process a single `Fetch.requestPaused` event and return the CDP commands
/// to execute in response (`Fetch.continueRequest` or `Fetch.failRequest`).
///
/// * C1b: The top-level document request (identified by `resourceType ==
///   "Document"` AND `url == nav_url`) is always allowed, so that
///   `--safe <file.html>` and `--safe --toc`/`--cover` work.
/// * Image block: When `!load_images` and `resourceType == "Image"`, the
///   request is failed BEFORE policy evaluation (fail-safe: never promotes a
///   policy Block to an Allow).
/// * Policy: All other requests (subresources, sub-frame documents) are
///   evaluated via `fetch_action`.
/// * I3: Allowed same-origin requests receive `extra_headers` via
///   `continueRequest`.
///
/// Blocked URLs are appended to `blocked`.
fn handle_fetch_event(
    msg: &serde_json::Value,
    policy: &ResourcePolicy,
    nav_url: &str,
    extra_headers: &[(String, String)],
    load_images: bool,
    blocked: &mut Vec<String>,
) -> Result<Vec<(String, serde_json::Value)>> {
    let params = &msg["params"];
    let request_id = params["requestId"].as_str().unwrap_or("").to_string();
    let req_url = params["request"]["url"].as_str().unwrap_or("").to_string();
    let resource_type = params["resourceType"].as_str().unwrap_or("");
    let original_headers = &params["request"]["headers"];

    // C1b: top-level document — allow unconditionally.
    if resource_type == "Document" && req_url == nav_url {
        let cp = build_continue_params(
            &request_id,
            &req_url,
            original_headers,
            nav_url,
            extra_headers,
        );
        return Ok(vec![("Fetch.continueRequest".into(), cp)]);
    }

    // Image block: checked BEFORE policy so it can only ADD blocks, never remove them.
    if !load_images && resource_type == "Image" {
        blocked.push(req_url);
        return Ok(vec![(
            "Fetch.failRequest".into(),
            json!({
                "requestId": request_id,
                "errorReason": "BlockedByClient",
            }),
        )]);
    }

    let is_redirect = params
        .get("responseStatusCode")
        .and_then(|c| c.as_u64())
        .map(|code| (300..=399).contains(&code))
        .unwrap_or(false);

    match fetch_action(policy, &req_url, is_redirect) {
        FetchDecision::Continue => {
            let cp = build_continue_params(
                &request_id,
                &req_url,
                original_headers,
                nav_url,
                extra_headers,
            );
            Ok(vec![("Fetch.continueRequest".into(), cp)])
        }
        FetchDecision::Fail => {
            blocked.push(req_url);
            Ok(vec![(
                "Fetch.failRequest".into(),
                json!({
                    "requestId": request_id,
                    "errorReason": "Aborted",
                }),
            )])
        }
    }
}

impl ChromiumRenderer {
    pub fn spawn() -> Result<Self> {
        Self::spawn_opts(SpawnOpts::default())
    }

    pub fn spawn_opts(opts: SpawnOpts) -> Result<Self> {
        let chrome = find_chrome().ok_or_else(|| WkError::Engine("no chrome found".into()))?;

        // Each spawn within the same process gets a unique user-data-dir so that
        // rapid back-to-back spawns never share or race over the same directory.
        let spawn_n = SPAWN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let udd = std::env::temp_dir().join(format!("wkx-cdp-{}-{}", std::process::id(), spawn_n));
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

        // ── Step 1: Poll DevToolsActivePort until Chrome has bound its debug socket.
        // Budget: 150 × 100 ms = 15 s.
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
        let port =
            port.ok_or_else(|| WkError::Engine("DevToolsActivePort never appeared".into()))?;

        // ── Step 2: Discover a page target's websocket URL via the /json endpoint.
        // Budget: 60 × 200 ms = 12 s.
        let mut ws_url = None;
        for _ in 0..60 {
            if let Ok(resp) = ureq::get(&format!("http://127.0.0.1:{port}/json")).call() {
                if let Ok(list) =
                    serde_json::from_reader::<_, serde_json::Value>(resp.into_reader())
                {
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
        let ws_url =
            ws_url.ok_or_else(|| WkError::Engine("devtools endpoint never appeared".into()))?;

        // ── Step 3: Open the CDP WebSocket with bounded retry + backoff.
        //
        // Chrome may respond to /json before its WebSocket server is fully bound
        // (e.g. on rapid re-spawn when the OS is reusing ephemeral ports).
        // Retry up to 40 times with 150 ms sleep between attempts (~6 s budget).
        // Connection-refused / reset are both retryable; any other error is fatal.
        let mut cdp = {
            const MAX_CONNECT_ATTEMPTS: u32 = 40;
            let mut last_err: Option<wkhtmltox_core::WkError> = None;
            let mut connected = None;
            for attempt in 0..MAX_CONNECT_ATTEMPTS {
                match connect(&ws_url) {
                    Ok(c) => {
                        connected = Some(c);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        if attempt + 1 < MAX_CONNECT_ATTEMPTS {
                            std::thread::sleep(Duration::from_millis(150));
                        }
                    }
                }
            }
            connected.ok_or_else(|| {
                last_err.unwrap_or_else(|| WkError::Engine("cdp connect: no attempts made".into()))
            })?
        };
        cdp.call("Page.enable", json!({}))?;

        // All succeeded — disarm the guard and hand ownership to ChromiumRenderer.
        let (child, user_data_dir) = guard.disarm();
        Ok(Self {
            child,
            user_data_dir,
            cdp,
            compat_ua_css: None,
            _html_temp: None,
            blocked_urls: Vec::new(),
            fetch_state: None,
            device_metrics: None,
        })
    }

    /// Send a CDP command while servicing `Fetch.requestPaused` events inline.
    ///
    /// When Fetch interception is active (`fetch_state.is_some()`), every CDP
    /// round-trip that might trigger subresource loads (Runtime.evaluate,
    /// Page.printToPDF) MUST go through this method instead of `cdp.call()`.
    /// This ensures that post-load JS network requests (e.g. via `setTimeout`)
    /// are intercepted and policy-evaluated even during `wait_ready` delays and
    /// PDF printing (C1 fix).
    ///
    /// Falls back to `cdp.call()` when Fetch is not active.
    fn call_pumping(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if self.fetch_state.is_none() {
            return self.cdp.call(method, params);
        }

        // Clone the state fields we need; the borrow of `self.fetch_state`
        // ends here so we can mutably borrow `self.cdp` and `self.blocked_urls`
        // independently (split borrow).
        let nav_url = self.fetch_state.as_ref().unwrap().nav_url.clone();
        let policy = self.fetch_state.as_ref().unwrap().policy.clone();
        let extra_headers = self.fetch_state.as_ref().unwrap().extra_headers.clone();
        let load_images = self.fetch_state.as_ref().unwrap().load_images;

        let mut new_blocked: Vec<String> = Vec::new();

        let result = self.cdp.call_pumping(method, params, |msg| {
            handle_fetch_event(
                msg,
                &policy,
                &nav_url,
                &extra_headers,
                load_images,
                &mut new_blocked,
            )
        })?;

        self.blocked_urls.extend(new_blocked);
        Ok(result)
    }
}

impl Drop for ChromiumRenderer {
    fn drop(&mut self) {
        // Best-effort: disable Fetch if it is still active for the current page.
        if self.fetch_state.is_some() {
            let _ = self.cdp.send_only("Fetch.disable", json!({}));
        }
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
            Source::Stdin => {
                return Err(WkError::Engine(
                    "Stdin source not supported by ChromiumRenderer".into(),
                ))
            }
        };

        // ── Cleanup from previous page ────────────────────────────────────────
        //
        // If Fetch was still enabled from the previous page (e.g. print_pdf
        // errored), disable it now before re-enabling for the new page.
        if self.fetch_state.is_some() {
            let _ = self.cdp.send_only("Fetch.disable", json!({}));
            self.fetch_state = None;
        }

        // Clear blocked-URLs from any previous open() call.
        self.blocked_urls.clear();

        // Persist compat CSS so wait_ready() can inject it after the page loads.
        self.compat_ua_css = load.compat_ua_css.clone();
        // Persist device_metrics so wait_ready() can apply smart-width expansion.
        self.device_metrics = load.device_metrics.clone();

        // ── Build per-origin extra headers (I3) ───────────────────────────────
        //
        // Custom headers and Basic-auth credentials are injected via
        // Fetch.continueRequest ONLY for same-origin requests, preventing
        // cross-origin credential leaks.  We intentionally do NOT call
        // Network.setExtraHTTPHeaders, which would send them to all hosts.
        let extra_headers: Vec<(String, String)> = {
            let mut h = load.custom_headers.clone();
            if let (Some(u), Some(p)) = (load.username.as_deref(), load.password.as_deref()) {
                let creds = format!("{u}:{p}");
                let encoded = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
                h.push(("Authorization".into(), format!("Basic {encoded}")));
            }
            h
        };

        // ── Apply networking settings via CDP ─────────────────────────────────
        self.cdp.call("Network.enable", json!({}))?;

        // Cookies require an http(s) origin; skip for file://, data:, etc.
        if !load.cookies.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            let params = build_cookies_params(&load.cookies, &url);
            self.cdp.call("Network.setCookies", params)?;
        }

        // NOTE: Network.setExtraHTTPHeaders is intentionally NOT called here.
        // Auth + custom headers are injected per-request via the Fetch pump (I3).

        if load.no_check_certificate {
            self.cdp.call("Security.enable", json!({}))?;
            self.cdp.call(
                "Security.setIgnoreCertificateErrors",
                json!({ "ignore": true }),
            )?;
        }

        if !load.enable_javascript {
            self.cdp.call(
                "Emulation.setScriptExecutionDisabled",
                json!({ "value": true }),
            )?;
        }

        // ── Viewport / device-scale override (Emulation.setDeviceMetricsOverride) ─
        //
        // Applied BEFORE Page.navigate so that Chrome lays out the page at the
        // requested viewport width from the very first paint.
        if let Some(dm) = &load.device_metrics {
            let w = if dm.width > 0 { dm.width } else { 1024 }; // upstream screenWidth default
            let h = if dm.height > 0 { dm.height } else { 0 };
            self.cdp.call("Emulation.setDeviceMetricsOverride", json!({
                "width": w, "height": h,
                "deviceScaleFactor": if dm.device_scale_factor > 0.0 { dm.device_scale_factor } else { 1.0 },
                "mobile": false
            }))?;
        }

        // ── Enable Fetch interception (whole-load, C1) ────────────────────────
        //
        // `Fetch.enable` pauses every matching request until the debugger
        // responds with continueRequest or failRequest.  Fetch stays enabled
        // from here until `print_pdf` completes (or the next `open()` / Drop).
        // This ensures post-load JS requests (e.g. via setTimeout) are still
        // intercepted during wait_ready() delays and PDF printing (C1 fix).
        //
        // IMPORTANT: Use send_only for Page.navigate (not call()).
        // When Fetch.enable is active, Chrome intercepts the navigation request
        // itself via Fetch.requestPaused BEFORE sending the Page.navigate
        // response.  Using call() here causes a deadlock:
        //   - call() blocks waiting for the Page.navigate response
        //   - Chrome waits for us to send continueRequest/failRequest first
        //   - Neither can proceed → 60-second timeout
        self.cdp.call(
            "Fetch.enable",
            json!({
                "patterns": [{ "urlPattern": "*" }],
                "handleAuthRequests": false
            }),
        )?;

        self.cdp.send_only("Page.navigate", json!({ "url": url }))?;

        // ── Event pump ────────────────────────────────────────────────────────
        //
        // Read CDP messages until Page.loadEventFired or the 60 s deadline.
        // For each Fetch.requestPaused event, apply C1b + policy + I3 logic
        // (see handle_fetch_event).
        //
        // After Page.loadEventFired we continue the loop for up to 200 ms to
        // drain any Fetch.requestPaused events Chrome had already queued.
        //
        // IMPORTANT: We do NOT disable Fetch after this pump (C1 fix).  Fetch
        // stays active during wait_ready() and print_pdf() so that post-load
        // JavaScript requests are still intercepted.
        let load_deadline = Instant::now() + Duration::from_secs(60);
        let mut page_loaded = false;
        'pump: loop {
            if Instant::now() >= load_deadline {
                // Best-effort cleanup before returning the error.
                let _ = self.cdp.send_only("Fetch.disable", json!({}));
                self.fetch_state = None;
                return Err(WkError::Engine(
                    "timeout waiting for Page.loadEventFired".into(),
                ));
            }

            let poll_ms = if page_loaded { 200 } else { 250 };
            let Some(msg) = self.cdp.read_message(Duration::from_millis(poll_ms)) else {
                if page_loaded {
                    break 'pump;
                }
                continue 'pump;
            };

            match msg.get("method").and_then(|m| m.as_str()) {
                Some("Fetch.requestPaused") => {
                    let params = &msg["params"];
                    let request_id = params["requestId"].as_str().unwrap_or("").to_string();
                    let req_url = params["request"]["url"].as_str().unwrap_or("").to_string();
                    let resource_type = params["resourceType"].as_str().unwrap_or("");
                    let original_headers = &params["request"]["headers"];
                    let is_redirect = params
                        .get("responseStatusCode")
                        .and_then(|c| c.as_u64())
                        .map(|code| (300..=399).contains(&code))
                        .unwrap_or(false);

                    // C1b: allow top-level document unconditionally.
                    if resource_type == "Document" && req_url == url {
                        let cp = build_continue_params(
                            &request_id,
                            &req_url,
                            original_headers,
                            &url,
                            &extra_headers,
                        );
                        self.cdp.send_only("Fetch.continueRequest", cp)?;
                    } else if !load.load_images && resource_type == "Image" {
                        // Image block: checked BEFORE policy (fail-safe).
                        self.blocked_urls.push(req_url);
                        self.cdp.send_only(
                            "Fetch.failRequest",
                            json!({
                                "requestId": request_id,
                                "errorReason": "BlockedByClient",
                            }),
                        )?;
                    } else {
                        match fetch_action(&load.policy, &req_url, is_redirect) {
                            FetchDecision::Continue => {
                                let cp = build_continue_params(
                                    &request_id,
                                    &req_url,
                                    original_headers,
                                    &url,
                                    &extra_headers,
                                );
                                self.cdp.send_only("Fetch.continueRequest", cp)?;
                            }
                            FetchDecision::Fail => {
                                self.blocked_urls.push(req_url);
                                self.cdp.send_only(
                                    "Fetch.failRequest",
                                    json!({
                                        "requestId": request_id,
                                        "errorReason": "Aborted",
                                    }),
                                )?;
                            }
                        }
                    }
                }
                Some("Page.loadEventFired") => {
                    page_loaded = true;
                }
                _ => {}
            }
        }

        // Fetch remains ENABLED.  Store state so call_pumping() can service
        // Fetch.requestPaused events during wait_ready() and print_pdf().
        self.fetch_state = Some(ActiveFetchState {
            nav_url: url,
            policy: load.policy.clone(),
            extra_headers,
            load_images: load.load_images,
        });

        Ok(PageHandle(1))
    }

    fn wait_ready(&mut self, _p: PageHandle, ready: &ReadyPolicy) -> Result<()> {
        // Poll document.readyState until "complete".
        //
        // IMPORTANT: Use call_pumping (not cdp.call) so that Fetch.requestPaused
        // events are serviced while we wait for the Runtime.evaluate response.
        // This keeps policy enforcement active during the readyState polling loop.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let val = self.call_pumping(
                "Runtime.evaluate",
                json!({ "expression": "document.readyState", "returnByValue": true }),
            )?;
            if val["result"]["value"].as_str() == Some("complete") {
                break;
            }
            if Instant::now() >= deadline {
                return Err(WkError::Engine(
                    "timeout waiting for readyState=complete".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // Inject compat UA-reset stylesheet if configured.
        if let Some(css) = &self.compat_ua_css.clone() {
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
            self.call_pumping(
                "Runtime.evaluate",
                json!({ "expression": js, "returnByValue": true }),
            )?;
        }

        // JS delay — sleep, then any Fetch events queued during the sleep will
        // be drained by the next call_pumping call (window_status or print_pdf).
        if ready.javascript_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(ready.javascript_delay_ms));
        }

        if let Some(wanted) = &ready.window_status {
            let deadline2 = Instant::now() + Duration::from_secs(30);
            loop {
                let val = self.call_pumping(
                    "Runtime.evaluate",
                    json!({ "expression": "window.status", "returnByValue": true }),
                )?;
                if val["result"]["value"].as_str() == Some(wanted.as_str()) {
                    break;
                }
                if Instant::now() >= deadline2 {
                    return Err(WkError::Engine(format!(
                        "timeout waiting for window.status={wanted:?}"
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // ── smart_width expansion ─────────────────────────────────────────────
        //
        // Best-effort: if smart_width is enabled and a width was set, measure
        // the page's scroll width and re-issue setDeviceMetricsOverride with the
        // larger of the two values so content is not clipped.  Approximate only.
        if let Some(dm) = self.device_metrics.clone() {
            if dm.smart_width && dm.width > 0 {
                let configured_w = dm.width;
                let scroll_val = self.call_pumping(
                    "Runtime.evaluate",
                    json!({ "expression": "document.documentElement.scrollWidth", "returnByValue": true }),
                )?;
                if let Some(scroll_w) = scroll_val["result"]["value"].as_u64() {
                    let scroll_w = scroll_w as u32;
                    if scroll_w > configured_w {
                        let h = if dm.height > 0 { dm.height } else { 0 };
                        let dsf = if dm.device_scale_factor > 0.0 {
                            dm.device_scale_factor
                        } else {
                            1.0
                        };
                        // Best-effort: ignore errors (viewport expansion is non-critical).
                        let _ = self.call_pumping(
                            "Emulation.setDeviceMetricsOverride",
                            json!({
                                "width": scroll_w, "height": h,
                                "deviceScaleFactor": dsf,
                                "mobile": false
                            }),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    fn eval_json(&mut self, _p: PageHandle, script: &str) -> Result<serde_json::Value> {
        let r = self.cdp.call(
            "Runtime.evaluate",
            json!({ "expression": script, "returnByValue": true }),
        )?;
        Ok(r["result"]["value"].clone())
    }

    fn print_pdf(&mut self, _p: PageHandle, g: &PageGeometry) -> Result<Vec<u8>> {
        let (w, h) = match g.orientation {
            Orientation::Portrait => (mm_to_in(g.width_mm), mm_to_in(g.height_mm)),
            Orientation::Landscape => (mm_to_in(g.height_mm), mm_to_in(g.width_mm)),
        };
        // Use call_pumping so that any Fetch.requestPaused events Chrome fires
        // during PDF rendering (e.g. lazy-loaded images) are serviced inline.
        let r = self.call_pumping(
            "Page.printToPDF",
            json!({
                "printBackground": g.print_background,
                "preferCSSPageSize": g.prefer_css_page_size,
                "generateDocumentOutline": g.generate_document_outline,
                "paperWidth": w, "paperHeight": h,
                "marginTop": mm_to_in(g.margin_top_mm),
                "marginBottom": mm_to_in(g.margin_bottom_mm),
                "marginLeft": mm_to_in(g.margin_left_mm),
                "marginRight": mm_to_in(g.margin_right_mm),
                "scale": g.scale,
                "transferMode": "ReturnAsBase64",
            }),
        )?;

        let b64 = r["data"]
            .as_str()
            .ok_or_else(|| WkError::Pdf("printToPDF: no data".into()))?;
        let pdf = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| WkError::Pdf(format!("b64 decode: {e}")))?;

        // Page is done — disable Fetch interception.
        let _ = self.cdp.send_only("Fetch.disable", json!({}));
        self.fetch_state = None;

        Ok(pdf)
    }

    fn snapshot(&mut self, _p: PageHandle, o: &SnapshotOpts) -> Result<RawImage> {
        let fmt_str = match o.format {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
        };

        let mut params = json!({
            "format": fmt_str,
            "captureBeyondViewport": true,
            "fromSurface": true,
        });

        // JPEG quality (ignored by Chrome for PNG).
        if o.format == ImageFormat::Jpeg {
            params["quality"] = json!(o.quality);
        }

        // CDP clip: applied when the caller requested a crop region.
        // `scale` here maps logical CSS pixels to the physical device pixels
        // captured by captureScreenshot.
        if let Some((x, y, w, h)) = o.crop {
            params["clip"] = json!({
                "x": x,
                "y": y,
                "width": w,
                "height": h,
                "scale": o.scale,
            });
        }

        // Use call_pumping so that any Fetch.requestPaused events Chrome may
        // emit during the capture (e.g. from running scripts) are serviced
        // inline and policy enforcement remains active.
        let r = self.call_pumping("Page.captureScreenshot", params)?;

        let b64 = r["data"]
            .as_str()
            .ok_or_else(|| WkError::Render("captureScreenshot: no data field".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| WkError::Render(format!("captureScreenshot b64 decode: {e}")))?;

        // Page is done — disable Fetch interception (mirrors print_pdf).
        let _ = self.cdp.send_only("Fetch.disable", json!({}));
        self.fetch_state = None;

        Ok(RawImage {
            bytes,
            format: o.format,
        })
    }
    fn page_info(&self, _p: PageHandle) -> Result<PageInfo> {
        Err(WkError::Engine(
            "page_info lands in a later milestone".into(),
        ))
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
        let auth = hdrs["Authorization"]
            .as_str()
            .expect("Authorization must be present");
        assert!(
            auth.starts_with("Basic "),
            "auth header must start with 'Basic '"
        );
        let b64 = auth.strip_prefix("Basic ").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("must be valid base64");
        assert_eq!(decoded, b"alice:s3cr3t");
    }

    #[test]
    fn extra_headers_params_none_when_empty() {
        let result = build_extra_headers_params(&[], None, None);
        assert!(
            result.is_none(),
            "must return None when there is nothing to set"
        );
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

    // ── same_origin / extract_origin unit tests ──────────────────────────────

    #[test]
    fn same_origin_http_match() {
        assert!(same_origin("http://example.com/a", "http://example.com/b"));
    }

    #[test]
    fn same_origin_different_hosts() {
        assert!(!same_origin("http://example.com/", "http://evil.com/"));
    }

    #[test]
    fn same_origin_different_schemes() {
        assert!(!same_origin("http://example.com/", "https://example.com/"));
    }

    #[test]
    fn same_origin_file_urls() {
        // Both file:// → same origin (empty authority).
        assert!(same_origin("file:///tmp/a.html", "file:///tmp/b.html"));
    }

    // ── build_continue_params unit tests ─────────────────────────────────────

    #[test]
    fn build_continue_params_no_extra_headers() {
        let p = build_continue_params(
            "req-1",
            "https://example.com/img.png",
            &json!({}),
            "https://example.com/page.html",
            &[],
        );
        assert_eq!(p["requestId"].as_str().unwrap(), "req-1");
        assert!(p.get("headers").is_none() || p["headers"].is_null());
    }

    #[test]
    fn build_continue_params_injects_headers_same_origin() {
        let extra = vec![("X-Auth".to_string(), "token".to_string())];
        let orig = json!({ "Accept": "image/*" });
        let p = build_continue_params(
            "req-2",
            "https://example.com/api",
            &orig,
            "https://example.com/page.html",
            &extra,
        );
        assert_eq!(p["headers"]["X-Auth"].as_str().unwrap(), "token");
        assert_eq!(p["headers"]["Accept"].as_str().unwrap(), "image/*");
    }

    #[test]
    fn build_continue_params_no_injection_cross_origin() {
        let extra = vec![("Authorization".to_string(), "Basic xxx".to_string())];
        let p = build_continue_params(
            "req-3",
            "https://cdn.evil.com/img.png",
            &json!({}),
            "https://example.com/page.html",
            &extra,
        );
        // Cross-origin: headers must NOT be injected.
        assert!(p.get("headers").is_none() || p["headers"].is_null());
    }
}
