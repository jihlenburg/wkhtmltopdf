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
cp "$TARGET/libwkhtmltox.a" "$STAGE/lib/"
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
  [ -n "$SRC_DIR" ] || { echo "package.sh: unexpected chrome-headless-shell zip layout" >&2; exit 1; }
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
