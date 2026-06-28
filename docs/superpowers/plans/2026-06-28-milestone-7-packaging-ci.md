# wkhtmltox-rs Milestone 7 — Packaging + CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make wkhtmltox-rs distributable — a self-contained per-platform tarball that bundles `chrome-headless-shell`, plus a GitHub Actions CI pipeline (lint/test/audit/compat-gate) and a tag-triggered release pipeline.

**Architecture:** The renderer learns to find a `chrome-headless-shell` bundled next to its own executable (so a packaged artifact runs offline with no system Chrome). A shell packaging script builds the release binaries + `libwkhtmltox` + vendored headers and bundles a downloaded `chrome-headless-shell` into a versioned tarball. Repo-hygiene configs (`rust-toolchain.toml`, `deny.toml`) pin the toolchain and enforce license/advisory policy. Two GitHub Actions workflows wire it together: `ci.yml` (push/PR) and `release.yml` (tag).

**Tech Stack:** Rust 2021 workspace (`wkhtmltox-rs/`), `chrome-headless-shell` from Chrome for Testing, QPDF (system, pkg-config), `cargo-deny`/`cargo-audit`, `actionlint`, the Python compat harness (PyMuPDF/numpy/scikit-image), GitHub Actions.

## Global Constraints

(From `docs/superpowers/specs/2026-06-28-no-qt-engine-design.md` §2/§7/§8/§9 + prior-milestone facts — every task implicitly includes these.)

- **Bundle `chrome-headless-shell`** as the primary engine; **system Chrome via `WKHTMLTOX_CHROME` is the config fallback** (spec §2). The deliverable is NOT a single static binary — it is a tarball with the browser bundled (~150–200 MB), documented (spec §2 non-goals, §9).
- **v1 platforms: Linux x86_64 + macOS (arm64).** Windows is explicitly post-v1 (TODO). Do NOT add Windows CI/packaging in M7; a `win64` branch may be left in the platform-detect `case` as an unreached stub only if it costs nothing.
- `cargo-deny` enforces license + advisory policy (spec §7). Allowed licenses must cover the actual dep tree: LGPL-3.0-or-later (our crates), Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0/Unicode-DFS-2016, Zlib, MPL-2.0.
- The C lib ships with the vendored headers `crates/wkhtmltox-capi/include/{pdf.h,image.h}` (spec §8). The repo-root `LICENSE` (LGPL-3.0-or-later) ships in every artifact.
- Version is `env!("CARGO_PKG_VERSION")` = `0.12.6` (per-crate in `wkhtmltopdf-cli`/`wkhtmltoimage-cli`/`wkhtmltox-capi`). The packaging version is read from `cargo metadata`, never hardcoded.
- Bin/lib names: `wkhtmltopdf` (crate `wkhtmltopdf-cli`), `wkhtmltoimage` (crate `wkhtmltoimage-cli`), `libwkhtmltox.{so,dylib}` + `libwkhtmltox.a` (crate `wkhtmltox-capi`, `[lib] name = "wkhtmltox"`, `crate-type=["cdylib","staticlib"]`).
- Build deps every runner needs BEFORE `cargo build`: **qpdf dev** (Linux `libqpdf-dev`, macOS `brew install qpdf`) for `wkhtmltox-pdf-sys/build.rs` (pkg-config `libqpdf`) + a C++17 compiler.
- `wkhtmltox-core` / `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`. LGPL header on new Rust files. Commit per task + the trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- **Honesty constraint:** GitHub Actions cannot run in the dev sandbox. Workflow YAML is authored + `actionlint`-validated; its first real execution is on GitHub. The plan must not claim a workflow "passed CI" — only that it lint-validates. Locally-runnable deliverables (find_chrome test, package.sh, deny.toml) ARE run and verified.
- Working dir for `cargo`/`python3` commands: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs`. The packaging script, workflows, configs that live at repo root use `/Users/jihlenburg/src/wkhtmltopdf`.

---

## File Structure

- `crates/wkhtmltox-render-chromium/src/launch.rs` — MODIFY: add bundled `chrome-headless-shell` discovery (sibling of `current_exe`) with a testable helper.
- `scripts/package.sh` — CREATE (repo root `scripts/`): build + stage + bundle chrome → versioned tarball.
- `rust-toolchain.toml` — CREATE (repo root): pin channel + components.
- `deny.toml` — CREATE (repo root): cargo-deny license + advisory policy.
- `.github/workflows/ci.yml` — CREATE: lint/test/audit + Linux compat-gate.
- `.github/workflows/release.yml` — CREATE: tag-triggered per-platform packaging + upload.
- `wkhtmltox-rs/BUILD.md` — CREATE: build-from-source + packaging + bundled-Chrome model docs.

---

### Task 1 — Bundled `chrome-headless-shell` discovery

**Goal:** `find_chrome` prefers a `chrome-headless-shell` bundled next to the running executable, so a packaged artifact runs offline. Order: explicit `WKHTMLTOX_CHROME` override (unchanged, fail-loud) → bundled sibling → system candidates.

**Files:**
- Modify: `crates/wkhtmltox-render-chromium/src/launch.rs`

**Interfaces:**
- Produces: unchanged public `find_chrome() -> Option<PathBuf>`; new private `bundled_chrome_in(dir: &std::path::Path) -> Option<PathBuf>` (testable, no `current_exe`).
- Consumes: nothing new.

- [ ] **Step 1: Write the failing test**

Append to `crates/wkhtmltox-render-chromium/src/launch.rs` (create a `#[cfg(test)] mod tests` if none exists):

```rust
#[cfg(test)]
mod tests {
    use super::bundled_chrome_in;
    use std::fs;

    #[test]
    fn finds_sibling_chrome_headless_shell() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" };
        let p = dir.path().join(name);
        fs::write(&p, b"#!/bin/sh\n").unwrap();
        let found = bundled_chrome_in(dir.path()).expect("sibling should be found");
        assert_eq!(found, p);
    }

    #[test]
    fn finds_chrome_in_subdir_layout() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" };
        let sub = dir.path().join("chrome-headless-shell");
        fs::create_dir_all(&sub).unwrap();
        let p = sub.join(name);
        fs::write(&p, b"#!/bin/sh\n").unwrap();
        assert_eq!(bundled_chrome_in(dir.path()).unwrap(), p);
    }

    #[test]
    fn none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(bundled_chrome_in(dir.path()).is_none());
    }
}
```

Ensure `tempfile` is a `[dev-dependencies]` of `wkhtmltox-render-chromium` (it is used by other tests in the workspace; if missing from THIS crate's Cargo.toml, add `tempfile = "3"` under `[dev-dependencies]`).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p wkhtmltox-render-chromium --lib launch::tests`
Expected: FAIL to COMPILE — `bundled_chrome_in` not found.

- [ ] **Step 3: Implement the discovery**

Replace the body of `find_chrome` and add the helper. Keep the existing `WKHTMLTOX_CHROME` env branch and the `CANDIDATES` list verbatim; insert the bundled probe between them:

```rust
pub fn find_chrome() -> Option<PathBuf> {
    // 1. Explicit override wins (fail-loud if set-but-missing — preserved).
    if let Ok(p) = std::env::var("WKHTMLTOX_CHROME") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
        eprintln!(
            "wkhtmltox: WKHTMLTOX_CHROME={p:?} does not exist; \
             platform-default Chrome search is disabled when the env var is set"
        );
        return None;
    }
    // 2. A chrome-headless-shell bundled next to our executable (self-contained artifact).
    if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
        if let Some(bundled) = bundled_chrome_in(&exe) {
            return Some(bundled);
        }
    }
    // 3. System Chrome candidates (config fallback).
    const CANDIDATES: &[&str] = &[
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    ];
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Look for a bundled `chrome-headless-shell` under `dir` (the directory holding
/// our executable). Checks a sibling binary and a `chrome-headless-shell/`
/// subdirectory layout (Chrome for Testing unzips into a versioned subdir, which
/// the packaging script flattens to one of these two shapes).
fn bundled_chrome_in(dir: &std::path::Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" };
    let candidates = [dir.join(name), dir.join("chrome-headless-shell").join(name)];
    candidates.into_iter().find(|p| p.exists())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p wkhtmltox-render-chromium --lib launch::tests`
Expected: 3 tests PASS.

- [ ] **Step 5: Clippy + no-regression**

Run: `cargo clippy -p wkhtmltox-render-chromium --lib -- -D warnings && cargo build -p wkhtmltox-render-chromium`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add wkhtmltox-rs/crates/wkhtmltox-render-chromium/src/launch.rs wkhtmltox-rs/crates/wkhtmltox-render-chromium/Cargo.toml
git commit -m "feat(chromium): discover chrome-headless-shell bundled next to the executable

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2 — Packaging script (`scripts/package.sh`)

**Goal:** One command produces a self-contained, versioned, per-platform tarball: the two binaries, `libwkhtmltox` (shared + static), the vendored headers, the LICENSE, a README, and a bundled `chrome-headless-shell`. Verified by an OFFLINE render using only the bundled browser.

**Files:**
- Create: `scripts/package.sh` (repo root)

**Interfaces:**
- Consumes: the Task 1 bundled-Chrome discovery (the offline test depends on it); `cargo metadata` (version); `jq` (present); `curl`/`unzip`.
- Produces: `dist/wkhtmltox-rs-<version>-<platform>.tar.gz`.

- [ ] **Step 1: Write the script**

Create `scripts/package.sh`:

```bash
#!/usr/bin/env bash
# wkhtmltox-rs packaging — build release artifacts + bundle chrome-headless-shell
# into a self-contained per-platform tarball.
#
#   scripts/package.sh                 # full package incl. chrome-headless-shell
#   scripts/package.sh --no-chrome     # skip the ~150 MB chrome download (fast)
#   CHROME_VERSION=131.0.6778.204 scripts/package.sh   # pin a chrome version
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WS="$REPO_ROOT/wkhtmltox-rs"
SKIP_CHROME=0
[ "${1:-}" = "--no-chrome" ] && SKIP_CHROME=1

# Chrome for Testing "Stable" channel is the default; pin via CHROME_VERSION.
CHROME_VERSION="${CHROME_VERSION:-131.0.6778.204}"

# ── platform detection → (pkg label, chrome-for-testing platform, dylib ext) ──
OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS/$ARCH" in
  Linux/x86_64)   PLAT="linux-x86_64";  CFT_PLAT="linux64";   DYLIB="so"  ;;
  Darwin/arm64)   PLAT="macos-arm64";   CFT_PLAT="mac-arm64"; DYLIB="dylib" ;;
  Darwin/x86_64)  PLAT="macos-x86_64";  CFT_PLAT="mac-x64";   DYLIB="dylib" ;;
  *) echo "package.sh: unsupported platform $OS/$ARCH (v1 = Linux x86_64, macOS arm64/x86_64)" >&2; exit 2 ;;
esac

echo "==> building release binaries + libwkhtmltox ($PLAT)"
( cd "$WS" && cargo build --release \
    -p wkhtmltopdf-cli -p wkhtmltoimage-cli -p wkhtmltox-capi )

VERSION="$(cd "$WS" && cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name=="wkhtmltopdf-cli") | .version')"
[ -n "$VERSION" ] || { echo "package.sh: could not read version" >&2; exit 1; }

STAGE="$REPO_ROOT/dist/wkhtmltox-rs-$VERSION-$PLAT"
rm -rf "$STAGE"; mkdir -p "$STAGE/bin" "$STAGE/lib" "$STAGE/include"

TARGET="$WS/target/release"
cp "$TARGET/wkhtmltopdf" "$TARGET/wkhtmltoimage" "$STAGE/bin/"
cp "$TARGET/libwkhtmltox.$DYLIB" "$STAGE/lib/"
cp "$TARGET/libwkhtmltox.a" "$STAGE/lib/" 2>/dev/null || true
cp "$WS/crates/wkhtmltox-capi/include/pdf.h" "$WS/crates/wkhtmltox-capi/include/image.h" "$STAGE/include/"
cp "$REPO_ROOT/LICENSE" "$STAGE/"

cat > "$STAGE/README.txt" <<EOF
wkhtmltox-rs $VERSION ($PLAT)
A no-Qt reimplementation of wkhtmltopdf/wkhtmltoimage driving headless Chromium.

bin/wkhtmltopdf, bin/wkhtmltoimage   command-line tools
bin/chrome-headless-shell            bundled rendering engine (used automatically)
lib/libwkhtmltox.$DYLIB, .a          C ABI library
include/pdf.h, include/image.h       C ABI headers
LICENSE                              LGPL-3.0-or-later

The tools find bin/chrome-headless-shell automatically (no system Chrome needed).
Override with the WKHTMLTOX_CHROME env var to point at a different browser.
EOF

if [ "$SKIP_CHROME" -eq 0 ]; then
  echo "==> fetching chrome-headless-shell $CHROME_VERSION ($CFT_PLAT)"
  JSON_URL="https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json"
  DL_URL="$(curl -fsSL "$JSON_URL" \
    | jq -r --arg v "$CHROME_VERSION" --arg p "$CFT_PLAT" \
        '.versions[] | select(.version==$v) | .downloads."chrome-headless-shell"[]? | select(.platform==$p) | .url' \
    | head -1)"
  [ -n "$DL_URL" ] || { echo "package.sh: no chrome-headless-shell $CHROME_VERSION for $CFT_PLAT" >&2; exit 1; }
  TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
  curl -fsSL "$DL_URL" -o "$TMP/chs.zip"
  unzip -q "$TMP/chs.zip" -d "$TMP"
  # Chrome for Testing unzips to chrome-headless-shell-<platform>/; flatten into bin/.
  SRC_DIR="$(find "$TMP" -maxdepth 1 -type d -name 'chrome-headless-shell-*' | head -1)"
  cp -R "$SRC_DIR" "$STAGE/bin/chrome-headless-shell"
  # Ensure the launcher the discovery looks for is executable.
  chmod +x "$STAGE/bin/chrome-headless-shell/chrome-headless-shell" 2>/dev/null || true
else
  echo "==> --no-chrome: skipping chrome-headless-shell bundle"
fi

echo "==> creating tarball"
TARBALL="$REPO_ROOT/dist/wkhtmltox-rs-$VERSION-$PLAT.tar.gz"
( cd "$REPO_ROOT/dist" && tar czf "$TARBALL" "wkhtmltox-rs-$VERSION-$PLAT" )
echo "packaged: $TARBALL"
```

Note the subdir layout: the script unzips chrome into `bin/chrome-headless-shell/` (a directory), so the Task 1 `bundled_chrome_in` `dir.join("chrome-headless-shell").join(name)` branch finds `bin/chrome-headless-shell/chrome-headless-shell`. The binaries live in `bin/`, so `current_exe().parent()` == `bin/`, and the subdir probe resolves correctly.

- [ ] **Step 2: Make it executable + add `dist/` to gitignore**

```bash
chmod +x scripts/package.sh
```
Add `dist/` to the repo-root `.gitignore` (create the line if absent) so packaged artifacts are never committed.

- [ ] **Step 3: Run the fast path (no chrome) to verify build + staging**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf && scripts/package.sh --no-chrome`
Expected: builds release binaries, prints `packaged: .../dist/wkhtmltox-rs-0.12.6-macos-arm64.tar.gz`. Then verify staging contents:
`tar tzf dist/wkhtmltox-rs-0.12.6-macos-arm64.tar.gz | sort` should list `bin/wkhtmltopdf`, `bin/wkhtmltoimage`, `lib/libwkhtmltox.dylib`, `lib/libwkhtmltox.a`, `include/pdf.h`, `include/image.h`, `LICENSE`, `README.txt`.

- [ ] **Step 4: Run the full path (with chrome) — the self-containment proof**

Run: `cd /Users/jihlenburg/src/wkhtmltopdf && scripts/package.sh`
Expected: downloads chrome-headless-shell (~150 MB), bundles it. Then prove OFFLINE operation — the bundled binary must render using ONLY the bundled browser, with no system Chrome and no env override:

```bash
STAGE=dist/wkhtmltox-rs-0.12.6-macos-arm64
printf '<h1>packaged offline render</h1>' > /tmp/pkg-test.html
env -u WKHTMLTOX_CHROME PATH=/usr/bin:/bin "$PWD/$STAGE/bin/wkhtmltopdf" /tmp/pkg-test.html /tmp/pkg-test.pdf
head -c 5 /tmp/pkg-test.pdf   # expect: %PDF-
```
Expected: a valid `%PDF-` file produced using the bundled `chrome-headless-shell` (this exercises the Task 1 discovery end-to-end). If the ~150 MB download is not feasible in the environment, run Step 3 only, and clearly note in the report that the offline-render proof was not executed — the find_chrome unit tests (Task 1) plus the staging-contents check are the gating evidence.

- [ ] **Step 5: Commit**

```bash
git add scripts/package.sh .gitignore
git commit -m "feat(packaging): package.sh — self-contained per-platform tarball w/ bundled chrome-headless-shell

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3 — Repo-hygiene configs (`rust-toolchain.toml`, `deny.toml`)

**Goal:** Pin the toolchain + components for reproducible CI, and enforce the spec's license/advisory policy with `cargo-deny` — validated by actually running `cargo deny check` locally.

**Files:**
- Create: `rust-toolchain.toml` (repo root)
- Create: `deny.toml` (repo root)

**Interfaces:** none (config only).

- [ ] **Step 1: Create `rust-toolchain.toml`**

```toml
# Pin the toolchain CI uses. The workspace MSRV floor is 1.74 (Cargo.toml
# rust-version); CI builds/tests on current stable for up-to-date lints.
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2: Create `deny.toml`**

cargo-deny v2 config. License allowlist covers the actual dep tree (spec §7); advisories deny vulnerabilities and warn (not deny) unmaintained — matching `security-audit.sh`'s stance that the lone `proc-macro-error2` unmaintained advisory is LOW:

```toml
# cargo-deny policy — license + advisory enforcement (spec §7).
[advisories]
version = 2
yanked = "deny"
ignore = []

[licenses]
version = 2
# allow-list: our crates + the transitive dependency tree.
allow = [
    "LGPL-3.0-or-later",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
    "MPL-2.0",
    "CC0-1.0",
]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "warn"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- [ ] **Step 3: Install cargo-deny and run the check**

cargo-deny is not installed. Install it (prefer the fast Homebrew bottle; fall back to cargo install):
`brew install cargo-deny 2>/dev/null || cargo install --locked cargo-deny`
Then run from the workspace:
`cd /Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs && cargo deny --manifest-path Cargo.toml check 2>&1 | tail -30`
(cargo-deny reads `deny.toml` from the workspace root or repo root; if it doesn't auto-find the repo-root `deny.toml`, pass `--config ../deny.toml`.)
Expected: `licenses ok`, `advisories ok` (or `advisories` with the single allowed `proc-macro-error2` unmaintained WARNING — a warning is acceptable, an ERROR is not). **If a license is rejected**, add the specific SPDX id the error names to the `allow` list and re-run until `cargo deny check` passes with no errors. Record the final output in the report.

- [ ] **Step 4: Commit**

```bash
git add rust-toolchain.toml deny.toml
git commit -m "chore(ci): pin toolchain (rust-toolchain.toml) + cargo-deny license/advisory policy

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4 — GitHub Actions workflows (`ci.yml`, `release.yml`)

**Goal:** A CI workflow (lint/test/audit + a Linux compat-gate against the 0.12.6 oracle) and a tag-triggered release workflow (per-platform `package.sh` + artifact upload). Authored and `actionlint`-validated (NOT executed here).

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`

**Interfaces:** Consumes `scripts/package.sh` (Task 2), `deny.toml`/`rust-toolchain.toml` (Task 3), `scripts/security-audit.sh --full`, `tests/compat/compare.py --gate`.

- [ ] **Step 1: Create `.github/workflows/ci.yml`**

```yaml
name: CI
on:
  push:
    branches: ["**"]
  pull_request:
permissions:
  contents: read
jobs:
  lint-test:
    name: lint + test (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-22.04, macos-14]
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        working-directory: wkhtmltox-rs
    steps:
      - uses: actions/checkout@v4
      - name: Install qpdf (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y libqpdf-dev qpdf
      - name: Install qpdf (macOS)
        if: runner.os == 'macOS'
        run: brew install qpdf
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: wkhtmltox-rs
      - name: rustfmt
        run: cargo fmt --all --check
      - name: clippy (production libs, -D warnings)
        run: cargo clippy --workspace --lib -- -D warnings
      - name: build
        run: cargo build --workspace
      - name: test (non-gated; Chrome e2e are #[ignore])
        run: cargo test --workspace
      - name: cargo-audit
        run: cargo install --locked cargo-audit && cargo audit
      - name: cargo-deny
        run: cargo install --locked cargo-deny && cargo deny --manifest-path Cargo.toml check --config ../deny.toml
      - name: static security audit
        run: bash ../scripts/security-audit.sh --full

  compat-gate:
    name: compat gate vs wkhtmltopdf 0.12.6 oracle (Linux)
    runs-on: ubuntu-22.04
    defaults:
      run:
        working-directory: wkhtmltox-rs
    steps:
      - uses: actions/checkout@v4
      - name: Install build + runtime deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libqpdf-dev qpdf python3-pip fonts-liberation \
            libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libxkbcommon0 \
            libxcomposite1 libxdamage1 libxrandr2 libgbm1 libpango-1.0-0 libasound2
          python3 -m pip install --quiet pymupdf numpy scikit-image
      - name: Install wkhtmltopdf 0.12.6 oracle (jammy .deb)
        run: |
          curl -fsSL -o /tmp/wkhtmltox.deb \
            https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox_0.12.6.1-2.jammy_amd64.deb
          sudo apt-get install -y /tmp/wkhtmltox.deb
          echo "WKHTMLTOX_ORACLE=/usr/local/bin/wkhtmltopdf" >> "$GITHUB_ENV"
          echo "WKHTMLTOX_IMAGE_ORACLE=/usr/local/bin/wkhtmltoimage" >> "$GITHUB_ENV"
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: wkhtmltox-rs
      - name: Bundle chrome-headless-shell next to the render example
        run: bash ../scripts/package.sh --no-chrome  # builds release bins
      - name: Install a headless Chrome for the harness
        uses: browser-actions/setup-chrome@v1
        id: chrome
      - name: Run compat gate
        env:
          WKHTMLTOX_CHROME: ${{ steps.chrome.outputs.chrome-path }}
        run: python3 tests/compat/compare.py --gate
```

Note: the compat-gate job uses `--gate` against the **default corpus** (`tests/compat/corpus`), whose floors live in `tests/compat/thresholds.json`. It does NOT gate the legacy corpus (those floors are corpus-specific — see M6). If `setup-chrome`'s output name differs, adjust the `WKHTMLTOX_CHROME` reference to the action's documented output.

- [ ] **Step 2: Create `.github/workflows/release.yml`**

```yaml
name: Release
on:
  push:
    tags: ["v*"]
permissions:
  contents: write
jobs:
  package:
    name: package (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-22.04, macos-14]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Install qpdf (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y libqpdf-dev qpdf
      - name: Install qpdf (macOS)
        if: runner.os == 'macOS'
        run: brew install qpdf
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: wkhtmltox-rs
      - name: Package
        run: bash scripts/package.sh
      - name: Upload release asset
        uses: softprops/action-gh-release@v2
        with:
          files: dist/*.tar.gz
```

- [ ] **Step 3: Install actionlint and validate both workflows**

`brew install actionlint 2>/dev/null || go install github.com/rhysd/actionlint/cmd/actionlint@latest`
Run from repo root: `actionlint .github/workflows/ci.yml .github/workflows/release.yml`
Expected: no errors. Fix any actionlint findings (quoting, expression syntax, deprecated runner labels). If actionlint truly cannot be installed, fall back to a YAML syntax validation (`python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/*.yml` — install pyyaml if needed) and a manual structural review; record which validation was used.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci: GitHub Actions — CI (lint/test/audit + compat gate) + tag release packaging

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5 — Build & packaging documentation

**Goal:** Document building from source, the bundled-Chrome model + system-Chrome fallback, the artifact layout, the size note, and how to run the CI checks locally.

**Files:**
- Create: `wkhtmltox-rs/BUILD.md`

**Interfaces:** none (docs).

- [ ] **Step 1: Write `wkhtmltox-rs/BUILD.md`**

Cover, in concise sections: (a) **Prerequisites** — Rust (stable; MSRV 1.74), qpdf dev headers (`apt install libqpdf-dev` / `brew install qpdf`), a C++17 compiler. (b) **Build** — `cargo build --release` from `wkhtmltox-rs/`; the produced `wkhtmltopdf`/`wkhtmltoimage`/`libwkhtmltox`. (c) **Rendering engine** — the tools need `chrome-headless-shell`: a packaged tarball bundles it (used automatically via sibling discovery); from a source build, set `WKHTMLTOX_CHROME=/path/to/chrome` or install a system Chrome (discovery order: `WKHTMLTOX_CHROME` → bundled sibling → system candidates). (d) **Packaging** — `scripts/package.sh` (and `--no-chrome`, `CHROME_VERSION`); the ~150–200 MB bundled-browser size note (spec §9); the artifact layout (`bin/`, `lib/`, `include/`, `LICENSE`). (e) **CI checks locally** — `cargo fmt --all --check`, `cargo clippy --workspace --lib -- -D warnings`, `cargo test --workspace`, `cargo deny check`, `cargo audit`, `bash scripts/security-audit.sh --full`, and the compat gate `python3 tests/compat/compare.py --gate` (needs the 0.12.6 oracle via `WKHTMLTOX_ORACLE` + Chrome + `pip install pymupdf numpy scikit-image`). (f) **Gated Chrome e2e tests** — `cargo test -- --ignored --test-threads=1`. Keep it accurate to the real commands; no invented flags.

- [ ] **Step 2: Sanity-check the documented commands exist**

Verify each command referenced is real: `scripts/package.sh` exists and accepts `--no-chrome`; `compare.py` accepts `--gate`; `security-audit.sh` accepts `--full`. (Spot-check by `--help`/reading, do not run the heavy ones.)

- [ ] **Step 3: Commit**

```bash
git add wkhtmltox-rs/BUILD.md
git commit -m "docs: BUILD.md — build-from-source, bundled-Chrome model, packaging, local CI checks

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Milestone Gate (controller-run, not an implementer task)

After Tasks 1–5 review clean:

1. **Whole-milestone Opus review** over the full M7 diff (`scripts/review-package <M7-base> HEAD`): focus on (a) the find_chrome discovery order being correct + not breaking the existing env-override fail-loud behavior; (b) `package.sh` robustness (set -euo pipefail, no unquoted expansions, the chrome flatten/discovery shapes actually align with Task 1, no secret/token handling); (c) the workflows being well-formed + least-privilege (`permissions:` scoped, no `pull_request_target`, no untrusted code exec, pinned action majors); (d) `deny.toml` not allow-listing anything dangerous. Security-spot: the release workflow has `contents: write` — confirm it only uploads built artifacts and runs no untrusted input.
2. **Static audit:** `bash scripts/security-audit.sh --full` (expect HIGH=0).
3. **Fix wave** for any Critical/Important findings.
4. **Push** `git push fork codex/macos-arm64-wkhtmltopdf`.
5. Update `TODO.md` (M7 → Done; the v1 critical path is now EMPTY → the loop terminates) + `logbook.md`.

---

## Self-Review

**1. Spec/TODO coverage:**
- "bundle `chrome-headless-shell`" → Task 1 (discovery) + Task 2 (bundling in the artifact) ✅ (spec §2)
- "per-platform artifacts (Linux/macOS)" → Task 2 `package.sh` platform `case` + Task 4 release matrix ✅
- "GitHub Actions workflow (cargo test/clippy/fmt + cargo-deny/cargo-audit + compat gate + the security-audit script)" → Task 4 `ci.yml` (every one of those steps present) ✅
- License/advisory policy (`cargo-deny`, spec §7) → Task 3 `deny.toml` ✅
- Vendored headers ship with the lib (spec §8) → Task 2 stages `include/` ✅

**2. Placeholder scan:** No "TBD"/"handle errors"/"similar to". Full contents given for launch.rs, package.sh, deny.toml, rust-toolchain.toml, ci.yml, release.yml. The few adaptive instructions (the `setup-chrome` output name in Step 4.1; the `cargo deny` config-path auto-find in 3.3; the actionlint-install fallback in 4.3) point at real external tools whose exact surface is environment-dependent — each carries a concrete fallback, not a blank.

**3. Type/name consistency:** Bin names (`wkhtmltopdf`/`wkhtmltoimage`), lib (`libwkhtmltox.{so,dylib}`/`.a`), header paths (`crates/wkhtmltox-capi/include/{pdf.h,image.h}`), version source (`cargo metadata` → `wkhtmltopdf-cli` `.version`), and the bundled-chrome subdir shape (`bin/chrome-headless-shell/chrome-headless-shell`, matching Task 1's `bundled_chrome_in` subdir branch) are consistent across Tasks 1/2/4/5.

**4. Honesty/risk notes:** GitHub Actions is authored + actionlint-validated, NOT executed in-sandbox (stated in Global Constraints + the milestone gate). The CI clippy gate is scoped to `--lib` (production code is clippy-clean per M6; `--all-targets` would trip pre-existing toolchain-drift lints in test code that are explicitly out of scope). `cargo fmt --all --check` may surface pre-existing formatting drift — Task 4's implementer must run it locally and, if the tree isn't fmt-clean, either `cargo fmt` the tree (separate, clearly-scoped) or note the gate will fail on first run; do NOT silently leave a red fmt gate. The chrome-headless-shell version is pinned (`CHROME_VERSION` default) for reproducibility; bump it as a maintenance task. Implementation chose a shell `package.sh` over the spec's suggested `xtask` crate (YAGNI: a script is simpler and directly testable here; an `xtask` refactor is a noted post-v1 option).
