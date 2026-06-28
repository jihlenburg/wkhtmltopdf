# wkhtmltox-rs — TODO (v1 milestone backlog)

The autonomous milestone loop drives this list: each milestone runs
plan → SDD execution → security audit → push → check off here + log in `logbook.md`.
**Loop terminates when every item under "v1 critical path" is checked.**
Each milestone is built test-first, validated against the wkhtmltopdf 0.12.6 oracle harness,
and security-audited (`scripts/security-audit.sh` + review) before push.

## In progress
- [ ] **M2b — TOC + chrome**: per-heading EXACT page mapping (the §5.1 crux — parse each object's Chromium `generateDocumentOutline` destinations → global pages), TOC generation via libxslt (`--xsl-style-sheet`) + TOC fixed-point page-number loop, variable headers/footers (text `[section]`/`[title]`/`[date]` + HTML), cover page, clickable-link annotation synthesis (Chromium bug 347674894). Also address deferred M2a finding: inheritance-aware `/Resources` in `stamp_footer` when inputs broaden.

## v1 critical path (queued)
- [ ] **M3 — C ABI**: vendored `pdf.h`/`image.h` verbatim, `wkhtmltox-capi` cdylib (`extern "C"` exports, per-converter UTF-8 string cache, `catch_unwind` on every export), header-diff test, C consumer test under ASan.
- [ ] **M3b — CLI**: wkhtmltopdf-compatible flag grammar (page-object positional model, repeatable two-arg flags), settings registry (names/defaults ported from upstream `reflect`), `--help`/`--extended-help`/`--manpage` generation.
- [ ] **M4 — Networking + security policy**: cookies/custom-headers/proxy/auth via CDP; `ResourcePolicy` (scheme allowlist, `--allow`, redirect re-validation, SSRF private-range block) enforced at CDP `Fetch` interception; opt-in `--safe` profile.
- [ ] **M5 — Image pipeline**: `wkhtmltoimage` (snapshot via `Page.captureScreenshot`, crop/scale/quality/transparent, PNG/JPEG encode) + image C ABI.
- [ ] **M6 — Compat hardening**: diff-vs-oracle harness as a CI gate (ordered outline-tree comparison), exit-code derivation from the reference binary, forms→AcroForm wiring end-to-end, visual/pixel corpus on real legacy docs.
- [ ] **M7 — Packaging + CI**: bundle `chrome-headless-shell`; per-platform artifacts (Linux/macOS); GitHub Actions workflow (cargo test/clippy/fmt + `cargo-deny`/`cargo-audit` + compat gate + the security-audit script).

## Post-v1 (NOT required for loop termination)
- [ ] Windows backend (WebView2 or WebKit-WinCairo)
- [ ] Font-substitution fidelity spike (close residual compat gap; likely limited)
- [ ] Deferred M1 Minors: `Orientation` Default, `Renderer`/DTO crate-root re-export, `find_chrome` test, `wait_event`/`call` JSON-skip consistency, `compile_commands.json` for IDE
- [ ] SVG image output (legacy QtSvg feature)

## Done
- [x] **M1 — Skeleton + Chromium renderer + AcroForm spike** (workspace, `Renderer` trait + `MockRenderer`, Chromium/CDP renderer w/ e2e, QPDF FFI, AcroForm risk retired, fidelity harness).
- [x] **Security-audit hook** (`scripts/security-audit.sh` + PostToolUse hook; caught + fixed RUSTSEC-2026-0187).
- [x] **Compat-profile v1** (`--compat` UA-reset; SSIM 0.717→0.734, page-drift font-metric-bound).
- [x] **M2a — Document assembly core** (probe/outline model, QPDF multi-doc merge, nested bookmarks, page-number footer, `assemble_pdf` wiring; oracle: outline 12/12 titles match, text 0.9974, pages ±1; temp-dir CWE-377 hardened). Commits `74a331c..5088a8d`.
