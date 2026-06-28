// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use wkhtmltox_core::render::*;
use wkhtmltox_render_chromium::renderer::{ChromiumRenderer, FetchDecision, fetch_action};

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

/// Verify that the `FetchDecision` enum and `fetch_action` helper are exported
/// and work correctly — a compile-time smoke test.
#[test]
fn fetch_action_exported_and_correct() {
    use wkhtmltox_core::policy::ResourcePolicy;
    // Default policy: permissive → Continue.
    assert_eq!(
        fetch_action(&ResourcePolicy::default(), "file:///etc/hosts", false),
        FetchDecision::Continue
    );
    // Safe profile: local file → Fail.
    assert_eq!(
        fetch_action(&ResourcePolicy::safe_profile(), "file:///etc/hosts", false),
        FetchDecision::Fail
    );
    // Safe profile: private IP → Fail.
    assert_eq!(
        fetch_action(&ResourcePolicy::safe_profile(), "http://127.0.0.1:1/", false),
        FetchDecision::Fail
    );
    // Safe profile: public HTTPS → Continue.
    assert_eq!(
        fetch_action(&ResourcePolicy::safe_profile(), "https://example.com/", false),
        FetchDecision::Continue
    );
}

#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored"]
fn networking_settings_accepted_headers_and_auth() {
    // Uses a data: URL so no network is needed; exercises the CDP networking
    // calls (Network.enable, Network.setExtraHTTPHeaders with Basic auth header)
    // and verifies the page still renders to a valid PDF.
    // Note: cookies require http/https origins, so they are tested separately
    // with real HTTP targets; this test focuses on custom-headers + auth + JS.
    let mut r = ChromiumRenderer::spawn().expect("spawn chrome");
    let load = LoadSettings {
        custom_headers: vec![("X-Test-Header".to_string(), "wkhtmltox".to_string())],
        username: Some("testuser".to_string()),
        password: Some("testpass".to_string()),
        enable_javascript: true,
        allow_local_file_access: true,
        ..Default::default()
    };
    // data: URL — no network required; the CDP networking calls are the point.
    let p = r.open(
        &Source::Url("data:text/html,<html><body><h1>Networking Test</h1></body></html>".into()),
        &load,
    ).expect("open must succeed with networking settings applied");
    r.wait_ready(p, &ReadyPolicy::default()).expect("wait_ready must succeed");
    let pdf = r.print_pdf(p, &PageGeometry::default()).expect("print_pdf must succeed");
    assert!(pdf.starts_with(b"%PDF"), "output must be a valid PDF");
}

/// Verify that back-to-back ChromiumRenderer spawns in the same process both
/// succeed — this is the exact scenario that triggered the M3 WebSocket-reset
/// failure.
///
/// The test spawns a renderer, renders a small page to PDF, drops the renderer,
/// then immediately spawns a SECOND renderer and renders again.  Both PDFs must
/// begin with `%PDF`.  The loop runs 3 times to surface intermittent races.
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn respawn_back_to_back() {
    const HTML: &str =
        "data:text/html,<html><body><h1>Respawn test</h1></body></html>";
    let load = LoadSettings { enable_javascript: false, ..Default::default() };

    for round in 0..3 {
        // ── First renderer ────────────────────────────────────────────────────
        let pdf_a = {
            let mut r = ChromiumRenderer::spawn()
                .unwrap_or_else(|e| panic!("round {round}: first spawn failed: {e}"));
            let p = r
                .open(&Source::Url(HTML.into()), &load)
                .unwrap_or_else(|e| panic!("round {round}: first open failed: {e}"));
            r.wait_ready(p, &ReadyPolicy::default())
                .unwrap_or_else(|e| panic!("round {round}: first wait_ready failed: {e}"));
            let pdf = r
                .print_pdf(p, &PageGeometry::default())
                .unwrap_or_else(|e| panic!("round {round}: first print_pdf failed: {e}"));
            // `r` is dropped here — kills+waits Chrome and removes user-data-dir.
            pdf
        };
        assert!(
            pdf_a.starts_with(b"%PDF"),
            "round {round}: first PDF is invalid (got {} bytes)",
            pdf_a.len()
        );

        // ── Second renderer (rapid re-spawn — this is the M3 failure case) ───
        let pdf_b = {
            let mut r = ChromiumRenderer::spawn()
                .unwrap_or_else(|e| panic!("round {round}: second spawn failed: {e}"));
            let p = r
                .open(&Source::Url(HTML.into()), &load)
                .unwrap_or_else(|e| panic!("round {round}: second open failed: {e}"));
            r.wait_ready(p, &ReadyPolicy::default())
                .unwrap_or_else(|e| panic!("round {round}: second wait_ready failed: {e}"));
            r.print_pdf(p, &PageGeometry::default())
                .unwrap_or_else(|e| panic!("round {round}: second print_pdf failed: {e}"))
        };
        assert!(
            pdf_b.starts_with(b"%PDF"),
            "round {round}: second PDF is invalid (got {} bytes)",
            pdf_b.len()
        );
    }
}

/// C1 + C1b: verify that Fetch interception stays active for the WHOLE page
/// load — including post-load JS fired via setTimeout — and that the
/// top-level file:// document still renders under --safe.
///
/// The page fires two SSRF/local-file requests from a `setTimeout` callback
/// that runs AFTER `Page.loadEventFired`.  With the whole-load Fetch fix (C1)
/// both requests arrive during `wait_ready()`'s JS delay and are blocked.
/// Without the fix they would escape interception (the old code disabled Fetch
/// immediately after loadEventFired).
///
/// C1b: The top-level file:// document itself must still load (we allow it
/// unconditionally) and produce a valid `%PDF`.
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn post_load_ssrf_blocked_whole_load_fetch() {
    use std::io::Write as _;
    use wkhtmltox_core::policy::ResourcePolicy;

    // Write a temp HTML file whose setTimeout fires AFTER Page.loadEventFired
    // and tries to load two policy-blocked resources.
    let mut tmp = tempfile::Builder::new()
        .prefix("wkx-postload-ssrf-")
        .suffix(".html")
        .tempfile()
        .expect("create temp html");
    writeln!(
        tmp,
        r#"<!DOCTYPE html>
<html><head><title>Post-load SSRF</title></head>
<body><p>Post-load SSRF test (C1 + C1b)</p>
<script>
setTimeout(function() {{
  // These fire AFTER loadEventFired — must still be intercepted under --safe.
  new Image().src = 'http://169.254.169.254/x';
  new Image().src = 'file:///etc/hosts';
}}, 0);
</script></body></html>"#
    )
    .expect("write html");
    tmp.flush().expect("flush");
    let main_path = tmp.path().to_str().expect("utf8 path").to_string();
    let main_url = format!("file://{main_path}");

    let mut r = ChromiumRenderer::spawn().expect("spawn chrome");
    let policy = ResourcePolicy {
        allow_local_file: false,
        // Allow the main HTML file itself (C1b: top-level document).
        allowed_paths: vec![main_path.clone()],
        block_private_ips: true,
        allow_external_links: true,
        allow_internal_links: true,
    };
    let load = LoadSettings {
        enable_javascript: true,
        allow_local_file_access: false,
        policy,
        ..Default::default()
    };
    // JS delay gives the setTimeout callback time to fire and trigger requests.
    let ready = ReadyPolicy {
        javascript_delay_ms: 500,
        ..Default::default()
    };

    // C1b: top-level file:// doc must load and produce %PDF even under --safe.
    let p = r
        .open(&Source::Url(main_url.clone()), &load)
        .expect("open must succeed — C1b: top-level file:// doc is allowed");
    r.wait_ready(p, &ready).expect("wait_ready must succeed");
    let pdf = r
        .print_pdf(p, &PageGeometry::default())
        .expect("print_pdf must succeed — C1b: top-level renders");
    assert!(pdf.starts_with(b"%PDF"), "output must be a valid PDF (C1b)");

    let blocked = r.blocked_urls();
    assert!(
        blocked.iter().any(|u| u.contains("169.254.169.254")),
        "post-load SSRF to IMDS must be blocked (C1); blocked_urls={blocked:?}"
    );
    assert!(
        blocked.iter().any(|u| u.contains("/etc/hosts")),
        "post-load local-file request must be blocked (C1); blocked_urls={blocked:?}"
    );
}

/// Verify that ResourcePolicy is enforced via CDP Fetch interception.
///
/// The test creates a temp HTML file that references two subresources:
///   1. `file:///etc/hosts`   — local file access (blocked by safe profile)
///   2. `http://127.0.0.1:1/nonexistent` — private/loopback IP (blocked by safe profile)
///
/// Under the DEFAULT policy both are allowed through (permissive) and do NOT
/// appear in `blocked_urls`.
///
/// Under a SAFE-EQUIVALENT policy (allow_local_file=false, block_private_ips=true)
/// both appear in `blocked_urls` and the page still renders to `%PDF`.
///
/// # Note on allowed_paths
/// The safe-equivalent policy includes the temp file's own path in
/// `allowed_paths` so the main page can load.  This mirrors real-world usage:
///   `wkhtmltopdf --safe --allow /path/to/my-report.html my-report.html out.pdf`
/// The subresources (`/etc/hosts`, `127.0.0.1`) are NOT in allowed_paths.
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored"]
fn policy_enforcement_blocks_ssrf_and_local_files() {
    use std::io::Write as _;
    use wkhtmltox_core::policy::ResourcePolicy;

    // Write a temp HTML page that tries to load two policy-blocked resources.
    let mut tmp = tempfile::Builder::new()
        .prefix("wkx-policy-test-")
        .suffix(".html")
        .tempfile()
        .expect("create temp html file");
    writeln!(
        tmp,
        r#"<!DOCTYPE html>
<html>
<head><title>Policy Enforcement Test</title></head>
<body>
<img src="file:///etc/hosts" alt="local-file">
<img src="http://127.0.0.1:1/nonexistent" alt="private-ip">
<p>ResourcePolicy enforcement test</p>
</body>
</html>"#
    ).expect("write html");
    tmp.flush().expect("flush");
    let main_path = tmp.path().to_str().expect("utf8 path").to_string();
    let main_url  = format!("file://{main_path}");

    // ── Default (permissive) policy ──────────────────────────────────────────
    {
        let mut r = ChromiumRenderer::spawn().expect("spawn chrome [default policy]");
        let load = LoadSettings {
            enable_javascript: true,
            allow_local_file_access: true,
            policy: ResourcePolicy::default(),
            ..Default::default()
        };
        let p = r.open(&Source::Url(main_url.clone()), &load)
            .expect("open [default policy]");
        r.wait_ready(p, &ReadyPolicy::default()).expect("wait_ready [default policy]");
        let pdf = r.print_pdf(p, &PageGeometry::default())
            .expect("print_pdf [default policy]");

        assert!(pdf.starts_with(b"%PDF"), "default policy: output must be PDF");

        let blocked = r.blocked_urls();
        assert!(
            !blocked.iter().any(|u| u.contains("/etc/hosts")),
            "default policy must NOT block file:///etc/hosts; blocked_urls={blocked:?}"
        );
        assert!(
            !blocked.iter().any(|u| u.contains("127.0.0.1")),
            "default policy must NOT block http://127.0.0.1:1/; blocked_urls={blocked:?}"
        );
    }

    // ── Safe-equivalent policy ────────────────────────────────────────────────
    // Allow only the main test file; block all other local files + private IPs.
    {
        let mut r = ChromiumRenderer::spawn().expect("spawn chrome [safe policy]");
        let policy = ResourcePolicy {
            allow_local_file: false,
            // The main HTML file is explicitly allowed; /etc/hosts is not.
            allowed_paths: vec![main_path.clone()],
            block_private_ips: true,
            allow_external_links: true,
            allow_internal_links: true,
        };
        let load = LoadSettings {
            enable_javascript: true,
            allow_local_file_access: false,
            policy,
            ..Default::default()
        };
        let p = r.open(&Source::Url(main_url.clone()), &load)
            .expect("open [safe policy]");
        r.wait_ready(p, &ReadyPolicy::default()).expect("wait_ready [safe policy]");
        let pdf = r.print_pdf(p, &PageGeometry::default())
            .expect("print_pdf [safe policy]");

        assert!(pdf.starts_with(b"%PDF"), "safe policy: output must still be PDF");

        let blocked = r.blocked_urls();
        assert!(
            blocked.iter().any(|u| u.contains("/etc/hosts")),
            "safe policy must block file:///etc/hosts; blocked_urls={blocked:?}"
        );
        assert!(
            blocked.iter().any(|u| u.contains("127.0.0.1")),
            "safe policy must block http://127.0.0.1:1/nonexistent; blocked_urls={blocked:?}"
        );
    }
}
