# wkhtmltox-rs — TODO (v1 milestone backlog)

The autonomous milestone loop drives this list: each milestone runs
plan → SDD execution → security audit → push → check off here + log in `logbook.md`.
**Loop terminates when every item under "v1 critical path" is checked.**
Each milestone is built test-first, validated against the wkhtmltopdf 0.12.6 oracle harness,
and security-audited (`scripts/security-audit.sh` + review) before push.

## In progress
- [ ] **M3 — C ABI**: vendored `pdf.h`/`image.h` verbatim, `wkhtmltox-capi` cdylib (`extern "C"` exports for `wkhtmltopdf_*`, per-converter UTF-8 string cache, `catch_unwind` on every export), header-diff test, C consumer test under ASan. (Bridges the C ABI to the existing settings/assembly core.)

## v1 critical path (queued)
- [ ] **M3b — CLI**: wkhtmltopdf-compatible flag grammar (page-object positional model, repeatable two-arg flags), settings registry (names/defaults ported from upstream `reflect`), `--help`/`--extended-help`/`--manpage` generation.
- [ ] **M4 — Networking + security policy**: cookies/custom-headers/proxy/auth via CDP; `ResourcePolicy` (scheme allowlist, `--allow`, redirect re-validation, SSRF private-range block) enforced at CDP `Fetch` interception; opt-in `--safe` profile.
- [ ] **M5 — Image pipeline**: `wkhtmltoimage` (snapshot via `Page.captureScreenshot`, crop/scale/quality/transparent, PNG/JPEG encode) + image C ABI.
- [ ] **M6 — Compat hardening**: diff-vs-oracle harness as a CI gate (ordered outline-tree comparison; body-only text metric excluding TOC/footer chrome), exit-code derivation from the reference binary, forms→AcroForm wiring end-to-end, visual/pixel corpus on real legacy docs.
- [ ] **M7 — Packaging + CI**: bundle `chrome-headless-shell`; per-platform artifacts (Linux/macOS); GitHub Actions workflow (cargo test/clippy/fmt + `cargo-deny`/`cargo-audit` + compat gate + the security-audit script).

## Post-v1 (NOT required for loop termination)
- [ ] Windows backend (WebView2 or WebKit-WinCairo)
- [ ] Font-substitution fidelity spike
- [ ] `--xsl-style-sheet` custom-XSLT TOC (libxslt) + HTML headers/footers (v1 is text cells only)
- [ ] TOC-entry clickable links; content link-rect coordinate accuracy; named-`/Dest` string resolution; Latin-1/PDFDocEncoding title decoding
- [ ] Deferred M1 Minors: `Orientation` Default, `Renderer`/DTO crate-root re-export, `find_chrome` test, `wait_event`/`call` JSON consistency, `compile_commands.json` for IDE
- [ ] SVG image output (legacy QtSvg feature)
- [ ] `examples/assemble.rs`: reject/​document `--number` + `--cover` combo (cover-aware numbering via `footer.center`)

## Done
- [x] **M1 — Skeleton + Chromium renderer + AcroForm spike** (workspace, `Renderer` trait + `MockRenderer`, Chromium/CDP renderer w/ e2e, QPDF FFI, AcroForm risk retired, fidelity harness).
- [x] **Security-audit hook** (`scripts/security-audit.sh` + PostToolUse hook; caught + fixed RUSTSEC-2026-0187).
- [x] **Compat-profile v1** (`--compat` UA-reset; SSIM 0.717→0.734, page-drift font-metric-bound).
- [x] **M2a — Document assembly core** (probe/outline, QPDF merge, nested bookmarks, page-number footer, `assemble_pdf`; oracle 12/12 titles; temp-dir CWE-377 hardened). `74a331c..5088a8d`.
- [x] **M2b — TOC + chrome** (exact per-heading pages via lopdf, TOC + fixed-point, variable headers/footers, cover page, clickable-link synthesis, `Source::Html` on real Chrome; oracle 13/13 titles, TOC present; cycle-guard hardened). `938b1f4..5adb9e9`.
