# wkhtmltox-rs — TODO (v1 milestone backlog)

The autonomous milestone loop drives this list: each milestone runs
plan → SDD execution → security audit → push → check off here + log in `logbook.md`.
**Loop terminates when every item under "v1 critical path" is checked.**
Each milestone is built test-first, validated against the wkhtmltopdf 0.12.6 oracle harness,
and security-audited (`scripts/security-audit.sh` + review) before push.

## In progress
- [ ] **M7 — Packaging + CI**: bundle `chrome-headless-shell`; per-platform artifacts (Linux/macOS); GitHub Actions workflow (cargo test/clippy/fmt + `cargo-deny`/`cargo-audit` + compat gate + the security-audit script). **Final v1 milestone — completing it empties the v1 critical path and terminates the loop.**

## v1 critical path (queued)
- _(empty — M7 is the last v1 item, now in progress)_

## Post-v1 (NOT required for loop termination)
- [ ] Windows backend (WebView2 or WebKit-WinCairo)
- [ ] Font-substitution fidelity spike
- [ ] `--xsl-style-sheet` custom-XSLT TOC (libxslt) + HTML headers/footers (v1 is text cells only)
- [ ] TOC-entry clickable links; content link-rect coordinate accuracy; named-`/Dest` resolution; Latin-1 titles
- [ ] Security hardening: DNS-rebinding (hostname→private IP) resolution; percent-encoded host decode; finer http_error
- [ ] C-ABI: warning-callback surfacing; per-row header/footer font size. CLI: `--quiet`; full ~100-flag parity; manpage; `--read-args-from-stdin`; per-page layout flags
- [ ] `--no-images` for the **PDF** path (`wkhtmltopdf`): M6 wired it for images only; PDF `GlobalSettings.to_load_settings()` still hardcodes `load_images: true` and `web.loadImages` stays unimplemented in `set_global`, so the flag is a silent no-op for PDF (works for `wkhtmltoimage`). Also: M6 form probe runs after `print_pdf` (cleaner alongside the link probe); thresholds.json floors are default-corpus-specific (legacy corpus needs its own).
- [ ] Deferred M1 Minors + 2 `useless_vec` lints in pdfread test code; `compile_commands.json` for IDE
- [ ] SVG image output (legacy QtSvg feature)

## Done
- [x] **M1 — Skeleton + Chromium renderer + AcroForm spike**.
- [x] **Security-audit hook** (caught + fixed RUSTSEC-2026-0187).
- [x] **Compat-profile v1** (`--compat` UA-reset).
- [x] **M2a — Document assembly core** (merge, bookmarks, page numbers; oracle 12/12). `74a331c..5088a8d`.
- [x] **M2b — TOC + chrome** (exact pages, TOC+fixed-point, headers/footers, cover, links; oracle 13/13). `938b1f4..5adb9e9`.
- [x] **M3 — libwkhtmltox C ABI (PDF)** (registry, vendored headers, 27 exports; C consumer PASS, leaks=0). `992d3b0..095f961`.
- [x] **M3b — wkhtmltopdf CLI** (page-object grammar, 54 flags; oracle exit codes match). `42b1158..b101c60`.
- [x] **M4 — Networking + security** (CDP cookies/headers/auth/cert/proxy; `ResourcePolicy` enforced via whole-load Fetch interception; `--safe` VERIFIED fail-closed against post-load-JS SSRF + file://; userinfo/alt-IP/IPv4-mapped bypasses closed; per-origin auth; rapid-respawn fixed). `237f351..7b3de90`.
- [x] **M5 — Image pipeline** (`wkhtmltoimage`: CDP `captureScreenshot` snapshot through the M4 whole-load Fetch/policy pump; core `image::produce` decode/crop/resize/encode; `wkhtmltoimage_*` C ABI = 18 exports, catch_unwind + str_cache + ownership-transfer; `wkhtmltoimage` CLI). Opus review: READY-WITH-FIXES, 0 Critical, 0 new security findings (snapshot `--safe` enforcement + FFI safety confirmed). Fixed: double-crop (CDP clip was re-applied by `produce`), `file://` percent-encode in C ABI, clippy. Oracle: both binaries emit valid PNGs (SSIM/text low where the macOS-arm64 oracle renders black — oracle bug). `2e9e0bc..3699508`.
- [x] **M6 — Compat hardening** (oracle harness: ordered outline-tree + body-only text metrics + `--gate` threshold; `wkhtmltoimage` exit-code parity; image device-metrics forwarding — `--zoom`/screen-size via `Emulation.setDeviceMetricsOverride`, `--no-images` via Fetch image-block, JPEG alpha-over-white; **forms→AcroForm end-to-end** for text inputs reusing the link px→pt transform; realistic legacy visual corpus). **Opus milestone review: READY, 0 Critical/Important; `call_pumping` security re-audit PASS** — image-block is monotonic (only adds blocks, never converts a policy Block→Allow), smart_width round-trips keep the pump live + timeout-bounded, form probe runs with Fetch off; all M4 SSRF/redirect/per-origin/userinfo protections byte-for-byte intact. Static audit HIGH=0. Gate fix wave: FFI symbol privacy, test CWE-377→NamedTempFile, degenerate-rect test, clippy, thresholds note. `c59ceea..9b9088b`.
