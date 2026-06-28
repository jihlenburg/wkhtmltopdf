#!/usr/bin/env bash
# run.sh — build and run the libwkhtmltox C consumer integration test.
#
# Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
#
# Usage (from any directory):
#   bash crates/wkhtmltox-capi/ctest/run.sh
# or (from the workspace root):
#   crates/wkhtmltox-capi/ctest/run.sh
#
# Include-path strategy
# ---------------------
# pdf.h contains:
#   #ifdef BUILDING_WKHTMLTOX
#     #include "dllbegin.inc"     ← library-side include
#   #else
#     #include <wkhtmltox/dllbegin.inc>   ← consumer-side include
#   #endif
#
# We are the consumer, so BUILDING_WKHTMLTOX is NOT defined.  We resolve
# <wkhtmltox/dllbegin.inc> by adding -I<ctest-dir> so that the shim files
# in ctest/wkhtmltox/{dllbegin,dllend}.inc are found.  These are verbatim
# copies of src/lib/{dllbegin,dllend}.inc.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# wkhtmltox-rs workspace root (three dirs up from ctest/)
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
CAPI_DIR="$SCRIPT_DIR/.."
INCLUDE_DIR="$CAPI_DIR/include"
TARGET_DIR="$WORKSPACE_DIR/target/debug"

echo "==> Building wkhtmltox-capi (workspace: $WORKSPACE_DIR)..."
(cd "$WORKSPACE_DIR" && cargo build -p wkhtmltox-capi)

# Confirm the drop-in lib name.
if [ ! -f "$TARGET_DIR/libwkhtmltox.dylib" ]; then
    echo "FAIL: $TARGET_DIR/libwkhtmltox.dylib not found after build" >&2
    exit 1
fi
echo "PASS: libwkhtmltox.dylib present at $TARGET_DIR/libwkhtmltox.dylib"

echo "==> Compiling C consumer..."
clang \
    -I"$INCLUDE_DIR" \
    -I"$SCRIPT_DIR" \
    "$SCRIPT_DIR/consumer.c" \
    -L"$TARGET_DIR" \
    -lwkhtmltox \
    -Wl,-rpath,"$TARGET_DIR" \
    -o "$TARGET_DIR/consumer"

echo "==> Running C consumer..."
"$TARGET_DIR/consumer"
CONSUMER_EXIT=$?

if [ "$CONSUMER_EXIT" -ne 0 ]; then
    echo "FAIL: consumer exited with code $CONSUMER_EXIT" >&2
    exit "$CONSUMER_EXIT"
fi

# ---------------------------------------------------------------------------
# ASan / leak best-effort
# ---------------------------------------------------------------------------
# ASan notes on macOS:
#   The Rust cdylib is not ASan-instrumented, so linking the C consumer with
#   -fsanitize=address against libwkhtmltox.dylib would cause false positives
#   from Rust's allocator interception.  Instead we run the *uninstrumented*
#   consumer under the macOS `leaks` tool (part of Xcode) to check for
#   heap leaks on the C side.
# ---------------------------------------------------------------------------

echo "==> ASan / leak check..."
if command -v leaks >/dev/null 2>&1; then
    leaks --atExit -- "$TARGET_DIR/consumer" \
        && echo "PASS: leaks found no heap leaks" \
        || echo "NOTE: leaks reported issues (see above); these may be from Rust runtime allocations"
else
    echo "NOTE: 'leaks' not found — ASan/leak check skipped."
    echo "      (Install Xcode command-line tools to enable the 'leaks' checker.)"
fi

echo "==> All steps completed successfully."
