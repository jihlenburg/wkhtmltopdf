#!/usr/bin/env bash
# SessionStart hook — Claude Code on the web.
#
# Provisions the build/test dependencies so a fresh remote session can build the
# wkhtmltox-rs workspace, run clippy, and run the non-gated test suite right
# away. It reuses scripts/setup-dev.sh — the same script the devcontainer
# postCreateCommand and the CI workflow rely on — so there is one source of
# truth for "what the project needs to build".
#
# Scope: setup-dev.sh WITHOUT --full. The Chrome engine + the wkhtmltopdf
# 0.12.6 oracle (the --full extras) are intentionally NOT installed here:
#   * they are only needed for the #[ignore]d gated Chrome e2e tests and the
#     oracle compat harness — never for `cargo build` / clippy / the non-gated
#     `cargo test --workspace`;
#   * their downloads (Chrome-for-Testing, the GitHub oracle .deb) are blocked
#     by the web sandbox egress policy (403), so attempting them just wastes
#     time every session;
#   * they would add minutes to each session start.
# Need them locally? Run `scripts/setup-dev.sh --full` by hand.
#
# Properties: web-only (no-op outside Claude Code remote), synchronous,
# idempotent, non-interactive. A fast-path readiness probe short-circuits when
# the cached container already has the deps, so warm starts are near-instant.
set -euo pipefail

# Only run in the Claude Code remote (web) environment; no-op locally.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
export DEBIAN_FRONTEND=noninteractive

# Fast path: if the container snapshot already carries every build dep, skip the
# heavy provisioning entirely (the toolchain, qpdf headers, and the Python
# compat libs are what `cargo build` / clippy / `cargo test --workspace` need).
if command -v cargo >/dev/null 2>&1 \
  && pkg-config --exists libqpdf 2>/dev/null \
  && python3 -c "import fitz, numpy, skimage" >/dev/null 2>&1; then
  echo "SessionStart: wkhtmltox-rs build deps already present — skipping setup-dev.sh."
  exit 0
fi

LOG="${TMPDIR:-/tmp}/wkhtmltox-session-start.log"
echo "SessionStart: provisioning wkhtmltox-rs build deps via scripts/setup-dev.sh (log: $LOG)…"
if bash "$PROJECT_DIR/scripts/setup-dev.sh" >"$LOG" 2>&1; then
  echo "SessionStart: build deps ready (qpdf, Rust 1.96.0, Python compat libs)."
  echo "             Build/test with:  cd wkhtmltox-rs && cargo build --workspace && cargo test --workspace"
else
  rc=$?
  echo "SessionStart: setup-dev.sh FAILED (exit $rc). Last 30 lines of $LOG:" >&2
  tail -n 30 "$LOG" >&2 || true
  exit "$rc"
fi
