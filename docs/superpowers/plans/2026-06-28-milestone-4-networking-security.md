# wkhtmltox-rs Milestone 4 — Networking + Security policy

> SDD execution. The security-critical milestone: wkhtmltopdf's historical CVEs were SSRF + local-file disclosure. Design basis: spec §6.3 (ResourcePolicy enforced at the backend resource hook; compatibility-vs-secure defaults; opt-in `--safe`).

## Global Constraints
- Rust 2021; no Qt. `wkhtmltox-core` (where ResourcePolicy lives) stays `#![forbid(unsafe_code)]`. LGPL headers; `cargo clippy -D warnings` clean. Commit per task + trailer.
- **Default = match wkhtmltopdf's permissive behavior** (local file access ALLOWED by default — drop-in compat). **`--safe` profile = hardened** (deny `file://` + SSRF private-range block). The choice is explicit, never silent (spec §6.3).
- ResourcePolicy logic lives in core (pure, browser-free testable); the Chromium backend ENFORCES it via CDP `Fetch` interception. Same logic, testable without a browser.

### Task 1 — ResourcePolicy (core, pure, security-tested)
- `wkhtmltox-core/src/policy.rs`: `pub struct ResourcePolicy { pub allow_local_file: bool, pub allowed_paths: Vec<String>, pub block_private_ips: bool, pub allow_external_links: bool, pub allow_internal_links: bool }` + `Default` (permissive: allow_local_file=true, block_private_ips=false). `pub fn safe_profile() -> ResourcePolicy` (allow_local_file=false, block_private_ips=true). `pub enum Decision { Allow, Block(String) }`. `pub fn decide(&self, url: &str, is_redirect: bool) -> Decision`:
  - Parse scheme/host. `file:`/`local` → Block unless `allow_local_file` OR the path is under an `allowed_paths` entry.
  - `http(s):` host that resolves to / is a literal private/link-local/loopback IP (`127/8`,`10/8`,`172.16/12`,`192.168/16`,`169.254/16`,`::1`,`fc00::/7`) → Block if `block_private_ips`. (Match on literal IP host; DNS-rebinding note for later.)
  - Redirect targets (`is_redirect=true`) are re-checked with the SAME rules (the redirect-to-`file://` hole).
- Unit tests (NO browser): default allows file:// + http; safe_profile blocks file:// (and an `--allow`ed path is permitted); safe_profile blocks `http://169.254.169.254/` and `http://127.0.0.1/` and `http://10.0.0.5/`; a redirect target to `file://` is blocked under safe; public host allowed.
- Commit `feat(core): ResourcePolicy (file:// + SSRF private-range gating, redirect re-validation)`.

### Task 2 — Wire LoadSettings networking into ChromiumRenderer (CDP)
- In `ChromiumRenderer::open`, apply `LoadSettings`: cookies (`Network.enable` + `Network.setCookie`/`setCookies`), custom headers (`Network.setExtraHTTPHeaders`), basic auth (username/password → `Fetch`/`Network` auth or an `Authorization` header), `no_check_certificate` (`Security.setIgnoreCertificateErrors{ignore:true}`), proxy (launch arg `--proxy-server=` — needs it at spawn, so thread proxy from settings into `spawn`). Map the registry `load.*` settings (cookie, customHeaders, username/password, proxy) into LoadSettings so the CLI/C-ABI reach them.
- Tests: pure unit tests for the CDP param building (cookie/header JSON); a gated (Chrome) test that a custom header + cookie are accepted without error (and, if feasible, echoed by a tiny local HTTP responder — else assert no error + page loads).
- Commit `feat(chromium): apply LoadSettings networking via CDP (cookies/headers/auth/proxy/cert)`.

### Task 3 — Enforce ResourcePolicy via CDP Fetch interception + `--safe`
- ChromiumRenderer: `Fetch.enable` (patterns = all), handle `Fetch.requestPaused` events during load: call `policy.decide(url, is_redirect)` → `Fetch.continueRequest` or `Fetch.failRequest{errorReason:"Aborted"}`. Re-check redirect responses. Thread a `ResourcePolicy` into the renderer (via LoadSettings or a new field).
- Wire `--safe` (CLI) + registry: `disable-local-file-access`/`enable-local-file-access`/`--allow`/`--safe` set the policy; default permissive.
- Tests (gated, real Chrome): a page with `<img src="file:///etc/hosts">` or an XHR to file:// → under DEFAULT it may load (permissive), under `--safe` the file:// subresource is BLOCKED (assert via console/network that it failed); an `http://127.0.0.1:<n>/` subresource blocked under safe. Keep assertions on the policy decision (e.g. a callback/log), robust to environment.
- Commit `feat: enforce ResourcePolicy via CDP Fetch interception + --safe profile`.

### Task 4 — Renderer rapid-respawn robustness
- Fix the M3 finding (back-to-back convert in one process → WebSocket reset). Options: retry the CDP websocket connect a few times with backoff in `spawn`; ensure the prior Chrome is fully reaped before the next spawn; OR reuse a single Chrome process across `open` calls (a renderer that can render multiple pages). Pick the simplest that makes back-to-back conversions reliable.
- Test (gated): two `assemble_pdf`/convert calls in one process succeed in a row (the exact M3 failure).
- Commit `fix(chromium): robust rapid re-spawn (retry CDP connect / reap) for back-to-back conversions`.

### Task 5 — Security validation + milestone gate
- A small SSRF/file-disclosure test corpus (HTML that tries file:// + private-IP fetches); assert blocked under `--safe`, allowed under default (documented). Then whole-milestone Opus SECURITY review (this is the highest-stakes security milestone), static audit, fix wave, push.

## Self-Review
Covers spec §6.3 (ResourcePolicy, file://+SSRF gating, redirect re-validation, --safe, permissive default) + the M3 respawn-robustness item. DNS-rebinding (host resolves public then private) is noted as a known limitation for post-v1 (we check literal-IP hosts + can resolve at decide time best-effort). Security is the gate: T1 pure tests + T3 gated tests + the Opus security review.
