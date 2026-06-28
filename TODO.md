# wkhtmltox-rs — TODO (v1 milestone backlog)

The autonomous milestone loop drives this list: each milestone runs
plan → SDD execution → security audit → push → check off here + log in `logbook.md`.
**Loop terminates when every item under "v1 critical path" is checked.**
Each milestone is built test-first, validated against the wkhtmltopdf 0.12.6 oracle harness,
and security-audited (`scripts/security-audit.sh` + review) before push.

## In progress
- [ ] **M4 — Networking + security policy**: cookies/custom-headers/proxy/auth via CDP; `ResourcePolicy` (scheme allowlist, `--allow`, redirect re-validation, SSRF private-range block) enforced at CDP `Fetch` interception; opt-in `--safe` profile. Plus **renderer rapid-respawn robustness** (retry CDP connect / reuse Chrome across conversions — from M3's back-to-back flakiness).

## v1 critical path (queued)
- [ ] **M5 — Image pipeline**: `wkhtmltoimage` (snapshot via `Page.captureScreenshot`, crop/scale/quality/transparent, PNG/JPEG encode) + image C ABI (`wkhtmltoimage_*`, image.h vendored) + `wkhtmltoimage` CLI.
- [ ] **M6 — Compat hardening**: diff-vs-oracle harness as a CI gate (ordered outline-tree comparison; body-only text metric excluding TOC/footer chrome), exit-code derivation from the reference binary, forms→AcroForm wiring end-to-end, visual/pixel corpus on real legacy docs.
- [ ] **M7 — Packaging + CI**: bundle `chrome-headless-shell`; per-platform artifacts (Linux/macOS); GitHub Actions workflow (cargo test/clippy/fmt + `cargo-deny`/`cargo-audit` + compat gate + the security-audit script).

## Post-v1 (NOT required for loop termination)
- [ ] Windows backend (WebView2 or WebKit-WinCairo)
- [ ] Font-substitution fidelity spike
- [ ] `--xsl-style-sheet` custom-XSLT TOC (libxslt) + HTML headers/footers (v1 is text cells only)
- [ ] TOC-entry clickable links; content link-rect coordinate accuracy; named-`/Dest` string resolution; Latin-1/PDFDocEncoding titles
- [ ] C-ABI: warning-callback surfacing; finer http_error codes; per-row header/footer font size
- [ ] CLI: `--quiet` honored; full ~100-flag parity; manpage exactness; `--read-args-from-stdin`; per-page layout flags; double-stdin reject
- [ ] Deferred M1 Minors: `Orientation` Default, `Renderer`/DTO crate-root re-export, `find_chrome` test, `compile_commands.json` for IDE
- [ ] SVG image output (legacy QtSvg feature)

## Done
- [x] **M1 — Skeleton + Chromium renderer + AcroForm spike**.
- [x] **Security-audit hook** (caught + fixed RUSTSEC-2026-0187).
- [x] **Compat-profile v1** (`--compat` UA-reset).
- [x] **M2a — Document assembly core** (merge, bookmarks, page numbers; oracle 12/12). `74a331c..5088a8d`.
- [x] **M2b — TOC + chrome** (exact pages, TOC+fixed-point, headers/footers, cover, links, `Source::Html`; oracle 13/13). `938b1f4..5adb9e9`.
- [x] **M3 — libwkhtmltox C ABI (PDF)** (registry, vendored headers, 27 exports w/ catch_unwind + string-cache + ownership; C consumer PASS, leaks=0). `992d3b0..095f961`.
- [x] **M3b — wkhtmltopdf CLI** (page-object grammar, 54 flags, executable; oracle: outline 13/13, exit codes match success/bad-flag/missing-input). `42b1158..b101c60`.
