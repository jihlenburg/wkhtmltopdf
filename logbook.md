# wkhtmltox-rs — logbook

Chronological record of the autonomous build. Newest at bottom. Branch: `codex/macos-arm64-wkhtmltopdf` → `fork`.

## 2026-06-28

- **Decision (engine):** brainstorm → `/devils-advocate` (4 disjoint challengers) → 2 spikes reversed per-platform-WebKit → **single headless Chromium over CDP**. SPIKE 1: Chromium `printToPDF{generateDocumentOutline}` gives correct bookmarks + page-accurate destinations. SPIKE 2: `WKWebView.createPDF` can't paginate → native WebKit dropped. Spec: `docs/superpowers/specs/2026-06-28-no-qt-engine-design.md`.
- **M1 complete** (15 commits `c7e14a0..5248f62`, final-review fixes `bb8feae`): Cargo workspace (core/render-chromium/pdf-sys), `Renderer` trait + `MockRenderer`, Chromium/CDP renderer (gated e2e: paginated PDF + `/Outlines`), QPDF FFI shim, AcroForm spike (top risk retired), fidelity harness vs 0.12.6 oracle. 2 unsafe edges only.
- **Fidelity decision (data-driven):** oracle = gold standard for all tests; no-Qt + opt-in legacy-compat profile. Text+outline already identical; visual SSIM 0.72→tunable; pagination font-metric-bound.
- **Security hook:** `scripts/security-audit.sh` + PostToolUse hook (`feat:`-gated, asyncRewake on HIGH). Caught + fixed **RUSTSEC-2026-0187** (lopdf 0.34 stack overflow → 0.42).
- **Compat-profile v1** (`b168286`): `--compat` UA-reset injection. SSIM 0.717→0.734 (baseline); page-drift unchanged (font-bound). Security audit clean.
- Pushed through `7279083`.
- **M2a (document assembly) started** (BASE `5b6f523`). Execution model: light controller check on low-risk pure-Rust tasks; full reviewer subagent on FFI/integration tasks; whole-milestone review + security audit + oracle validation before each push.
  - Task 1 ✅ `a3ec621` — outline model (probe JS + heading tree), 2/2 tests, clippy clean.
