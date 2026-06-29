#!/usr/bin/env bash
# wkhtmltox-rs — development environment setup for Linux (x86_64 or arm64).
# Provisions the build/test deps so a fresh box (incl. the Claude Code Web
# sandbox) can build the workspace and run the test suites.
#
#   scripts/setup-dev.sh           # build + test + harness deps (Rust, qpdf, C++, python)
#   scripts/setup-dev.sh --full    # ALSO install a Chrome engine + the wkhtmltopdf 0.12.6 oracle
#                                   # (needed for the gated Chrome e2e tests + the compat gate)
#
# Debian/Ubuntu (apt) focused — that's what CI runners and the web sandbox use.
# Adapt the package manager lines for other distros. Safe to re-run (idempotent-ish).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCH="$(uname -m)"   # x86_64 | aarch64
FULL=0
[ "${1:-}" = "--full" ] && FULL=1

SUDO=""; [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1 && SUDO="sudo"

echo "==> wkhtmltox-rs dev setup (arch=$ARCH, full=$FULL)"

# ── 1. Build + harness system deps ──────────────────────────────────────────
if command -v apt-get >/dev/null 2>&1; then
  $SUDO apt-get update
  # libqpdf-dev: wkhtmltox-pdf-sys links libqpdf via pkg-config (+ C++17 compiler).
  # python3 + venv/pip: the tests/compat harness (PyMuPDF/numpy/scikit-image).
  $SUDO apt-get install -y --no-install-recommends \
    build-essential pkg-config clang \
    libqpdf-dev qpdf \
    curl ca-certificates unzip xz-utils jq \
    python3 python3-pip
else
  echo "!! non-apt distro: install build-essential/clang, libqpdf-dev, pkg-config, jq, python3-pip yourself" >&2
fi

# Python harness deps (PEP 668 systems need --break-system-packages or a venv).
python3 -m pip install --quiet --upgrade pymupdf numpy scikit-image 2>/dev/null \
  || python3 -m pip install --quiet --break-system-packages pymupdf numpy scikit-image \
  || echo "!! could not pip-install harness deps; run them in a venv if you need the compat harness" >&2

# ── 2. Rust toolchain (rust-toolchain.toml pins the exact version) ───────────
if ! command -v cargo >/dev/null 2>&1; then
  echo "==> installing rustup (rust-toolchain.toml will select the pinned version)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi
# Materialize the pinned toolchain + components by running a no-op in the repo.
( cd "$REPO_ROOT/wkhtmltox-rs" && rustup show >/dev/null 2>&1 || true; cargo --version )

# ── 3. (--full) Chrome engine + the 0.12.6 oracle ───────────────────────────
if [ "$FULL" -eq 1 ]; then
  echo "==> (--full) installing a Chrome engine"
  case "$ARCH" in
    x86_64)
      # Chrome for Testing publishes a linux64 chrome-headless-shell; bundle it.
      CHROME_VERSION="${CHROME_VERSION:-131.0.6778.204}"
      JSON="https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json"
      URL="$(curl -fsSL "$JSON" | jq -r --arg v "$CHROME_VERSION" \
        '.versions[] | select(.version==$v) | .downloads."chrome-headless-shell"[]? | select(.platform=="linux64") | .url' | head -1)"
      if [ -n "$URL" ]; then
        TMP="$(mktemp -d)"; curl -fsSL "$URL" -o "$TMP/chs.zip"; unzip -q "$TMP/chs.zip" -d "$HOME/.local/chrome"
        BIN="$(find "$HOME/.local/chrome" -name chrome-headless-shell -type f | head -1)"
        chmod +x "$BIN" 2>/dev/null || true
        echo "   chrome-headless-shell → $BIN"
        echo "   export WKHTMLTOX_CHROME=$BIN   # add to your shell profile"
      else
        echo "!! could not resolve chrome-headless-shell $CHROME_VERSION for linux64" >&2
      fi
      ;;
    aarch64|arm64)
      # Chrome for Testing has NO linux-arm64 build → use the distro chromium.
      echo "   arm64 Linux: no Chrome-for-Testing build; installing system chromium"
      $SUDO apt-get install -y chromium 2>/dev/null || $SUDO apt-get install -y chromium-browser 2>/dev/null \
        || echo "!! install a chromium yourself and set WKHTMLTOX_CHROME" >&2
      echo "   set WKHTMLTOX_CHROME to your chromium binary if discovery fails"
      ;;
  esac

  echo "==> (--full) installing the wkhtmltopdf 0.12.6 oracle (compat harness)"
  if [ "$ARCH" = "x86_64" ]; then
    curl -fsSL -o /tmp/wkhtmltox.deb \
      https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox_0.12.6.1-2.jammy_amd64.deb || true
    $SUDO apt-get install -y /tmp/wkhtmltox.deb 2>/dev/null \
      && echo "   export WKHTMLTOX_ORACLE=/usr/local/bin/wkhtmltopdf" \
      || echo "!! oracle .deb install failed; the compat gate needs WKHTMLTOX_ORACLE set manually" >&2
  else
    echo "!! no amd64 oracle .deb on arm64; the compat harness needs a 0.12.6 oracle from another source" >&2
  fi
fi

echo "==> done. Sanity check:  cd wkhtmltox-rs && cargo build --workspace"
