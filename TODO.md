# wkhtmltox-rs — TODO (v1 milestone backlog)

The autonomous milestone loop drives this list: each milestone runs
plan → SDD execution → security audit → push → check off here + log in `logbook.md`.
**Loop terminates when every item under "v1 critical path" is checked.**
Each milestone is built test-first, validated against the wkhtmltopdf 0.12.6 oracle harness,
and security-audited (`scripts/security-audit.sh` + review) before push.

## In progress
- [ ] **M5 — Image pipeline**: `wkhtmltoimage` — snapshot via CDP `Page.captureScreenshot` (full page or `--crop-*` region), scale/`--width`/`--height`/`--zoom`, `--quality`, `--transparent`, PNG/JPEG encode; image C ABI (`wkhtmltoimage_*`, `image.h` already vendored) with the same catch_unwind + string-cache pattern; `wkhtmltoimage` CLI.

## v1 critical path (queued)
- [ ] **M6 — Compat hardening**: diff-vs-oracle harness as a CI gate (ordered outline-tree comparison; body-only text metric excluding TOC/footer chrome), exit-code derivation from the reference binary, forms→AcroForm wiring end-to-end, visual/pixel corpus on real legacy docs. **Also: re-audit the M4 `call_pumping` whole-load Fetch pump for any residual fail-open.**
- [ ] **M7 — Packaging + CI**: bundle `chrome-headless-shell`; per-platform artifacts (Linux/macOS); GitHub Actions workflow (cargo test/clippy/fmt + `cargo-deny`/`cargo-audit` + compat gate + the security-audit script).

## Post-v1 (NOT required for loop termination)
- [ ] Windows backend (WebView2 or WebKit-WinCairo)
- [ ] Font-substitution fidelity spike
- [ ] `--xsl-style-sheet` custom-XSLT TOC (libxslt) + HTML headers/footers (v1 is text cells only)
- [ ] TOC-entry clickable links; content link-rect coordinate accuracy; named-`/Dest` resolution; Latin-1 titles
- [ ] Security hardening: DNS-rebinding (hostname→private IP) resolution; percent-encoded host decode; finer http_error
- [ ] C-ABI: warning-callback surfacing; per-row header/footer font size. CLI: `--quiet`; full ~100-flag parity; manpage; `--read-args-from-stdin`; per-page layout flags
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
