# wkhtmltox-rs — agent handoff & working conventions

A no-Qt reimplementation of **wkhtmltopdf / wkhtmltoimage** built on **headless Chromium (CDP)** + **QPDF**, as a Rust workspace under `wkhtmltox-rs/`. The legacy Qt/C++ source remains under `src/` as the behavioral reference.

## Read these FIRST (current state lives here, not in chat)
- **`TODO.md`** — the milestone backlog (what's done, what's queued, post-v1 items).
- **`logbook.md`** — chronological record of every milestone with commit ranges + decisions. This is the source of truth for project history.
- **`docs/superpowers/specs/`** — the design spec. **`docs/superpowers/plans/`** — per-milestone implementation plans.
- **`wkhtmltox-rs/BUILD.md`** — build-from-source, bundled-Chrome model, packaging, local CI checks.

**Status (2026-06-29):** v1 critical path complete (M1–M7); post-v1 M8 (SSRF hardening), M9 (custom-XSLT TOC), M10 (HTML headers/footers) complete. CI green. Work branch: `codex/macos-arm64-wkhtmltopdf`.

## How work is executed (the milestone loop)
Each milestone: **plan** (writing-plans → `docs/superpowers/plans/`) → **SDD** (a fresh implementer subagent per task + a reviewer subagent; TDD) → **whole-milestone review** (most-capable model) → **static security audit** (`scripts/security-audit.sh --full`, expect `HIGH=0`) → **push** → update `TODO.md` + `logbook.md`. Gated Chrome e2e + the oracle compat harness validate fidelity.

## Commit policy (IMPORTANT)
**Default: do NOT commit or push automatically — ask first.** (An earlier local session had explicit standing authorization to commit/push autonomously; that was session-scoped. Do not assume it.) Always end commit messages with the project's `Co-Authored-By` trailer.

## Code conventions
- `wkhtmltox-core` and `wkhtmltox-render-chromium` are `#![forbid(unsafe_code)]`. All `unsafe`/FFI lives only in `wkhtmltox-pdf-sys` (QPDF C++ shim) and `wkhtmltox-capi` (the `libwkhtmltox` C ABI). Every C export wraps its body in `catch_unwind`; C++ shim functions have `catch(...)` backstops.
- **Run `cargo fmt --all` before every commit** (CI's first gate is `cargo fmt --all --check`; missing this is what broke CI between M8–M10).
- The toolchain is **pinned to `1.96.0`** (`rust-toolchain.toml` + the `dtolnay/rust-toolchain` workflow refs) so local `fmt`/`clippy` == CI exactly. Don't bump it casually.
- **Fix red CI before starting new feature work.** Check run status after pushing: `gh run list/view/watch --repo jihlenburg/wkhtmltopdf`.

## Build / test (from `wkhtmltox-rs/`)
- Build deps: **`libqpdf-dev` + a C++17 compiler** (`wkhtmltox-pdf-sys` links libqpdf via pkg-config). On a fresh Linux box run **`scripts/setup-dev.sh`** (`--full` also installs a Chrome engine + the 0.12.6 oracle).
- `cargo build --workspace` / `cargo test --workspace` (non-gated; the Chrome e2e tests are `#[ignore]`).
- Gated Chrome e2e: `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1` (needs a Chrome; set `WKHTMLTOX_CHROME`).
- **Local CI gate (run before pushing):** `cargo fmt --all --check` · `cargo clippy --workspace --lib -- -D warnings` · `cargo test --workspace` · `cargo deny check --config ../deny.toml` · `cargo audit` · `bash ../scripts/security-audit.sh --full`.
- Compat harness vs the oracle: `python3 tests/compat/compare.py --gate` (needs `WKHTMLTOX_ORACLE` = wkhtmltopdf 0.12.6, a Chrome, and `pip install pymupdf numpy scikit-image`).

## Environment variables
- `WKHTMLTOX_CHROME` — path to a Chrome/Chromium binary (overrides discovery; required where no system/bundled Chrome exists).
- `WKHTMLTOX_ORACLE` / `WKHTMLTOX_IMAGE_ORACLE` — the reference wkhtmltopdf/wkhtmltoimage 0.12.6 binaries for the compat harness.

## Platforms
Supported: **Linux x86_64**, **Linux arm64**, **macOS arm64** (Windows is post-v1). The renderer finds Chrome in this order: `WKHTMLTOX_CHROME` → a `chrome-headless-shell` bundled next to the executable → system Chrome (`/usr/bin/google-chrome`, `/usr/bin/chromium`, …).
- **arm64 Linux caveat:** Chrome for Testing publishes **no** `linux-arm64` `chrome-headless-shell`, so the arm64 Linux package does **not** bundle the engine — install system **chromium** (auto-discovered) or set `WKHTMLTOX_CHROME`. x86_64 Linux + macOS bundle the engine normally (`scripts/package.sh`).

## CI / release
- `.github/workflows/ci.yml` — on push/PR: a `lint + test` matrix (Linux x86_64, Linux arm64, macOS arm64) + a Linux compat-gate vs the 0.12.6 oracle.
- `.github/workflows/release.yml` — on tag `v*`: per-platform `scripts/package.sh` → tarball upload.
- `.devcontainer/` — runs `scripts/setup-dev.sh` so the Claude Code Web sandbox is build-ready.

## Note for Claude Code Web
This repo is the single source of truth — a fresh session resumes from `TODO.md` + `logbook.md` + the plans. Local-only context that does NOT transfer: the prior CLI session's chat, the machine's `~/.claude` auto-memory, and the locally-installed 0.12.6 oracle (re-provision via `scripts/setup-dev.sh --full`).
