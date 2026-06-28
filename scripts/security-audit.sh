#!/usr/bin/env bash
# wkhtmltox-rs — post-feature cybersecurity audit (static, tuned to this project).
# Usage:
#   scripts/security-audit.sh            # audit the latest commit IF its subject starts with "feat"
#   scripts/security-audit.sh --full     # audit the whole wkhtmltox-rs/ tree regardless
# Exit codes: 0 = ok / nothing to do, 2 = HIGH-severity finding(s) (wakes the agent via asyncRewake).
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT" || exit 0
SCOPE="wkhtmltox-rs"
RG="$(command -v rg || echo grep)"

MODE="${1:-auto}"
SUBJECT="$(git log -1 --format=%s 2>/dev/null || echo '')"
SHA="$(git rev-parse --short HEAD 2>/dev/null || echo nogit)"
STAMP="$(date +%Y%m%d-%H%M%S)"

# Only audit features (feat:) unless --full is passed.
if [ "$MODE" != "--full" ]; then
  case "$SUBJECT" in
    feat*|feat\(*) : ;;                       # a feature commit -> audit
    *) exit 0 ;;                              # not a feature -> nothing to do
  esac
fi

OUTDIR="$ROOT/.superpowers/security"
mkdir -p "$OUTDIR"
REPORT="$OUTDIR/audit-${SHA}-${STAMP}.md"
HIGH=0; MED=0; LOW=0

say() { printf '%s\n' "$*" >> "$REPORT"; }
hit() { # sev "title" "rg-output"
  local sev="$1" title="$2" body="$3"
  [ -z "$body" ] && return 0
  case "$sev" in HIGH) HIGH=$((HIGH+1));; MED) MED=$((MED+1));; LOW) LOW=$((LOW+1));; esac
  say ""; say "### [$sev] $title"; say '```'; say "$body"; say '```'
}

say "# Security audit — ${SCOPE} @ ${SHA} (${STAMP})"
say "Trigger: ${MODE} | commit subject: ${SUBJECT:-<none>}"
say "Scope: \`${SCOPE}/\` (rust + C++ shim + build + harness)"

# --- HIGH: unsafe outside the two sanctioned FFI crates ---
UNSAFE="$($RG -n '\bunsafe\b' "$SCOPE/crates/wkhtmltox-core" "$SCOPE/crates/wkhtmltox-render-chromium" 2>/dev/null | $RG -v 'forbid\(unsafe_code\)' || true)"
hit HIGH "unsafe in a crate that must be safe (core/render-chromium)" "$UNSAFE"

# --- HIGH: hardcoded secrets / private keys ---
SECRETS="$($RG -nE '(secret|passwd|password|api[_-]?key|access[_-]?token|bearer)\s*[:=]\s*["'\''][^"'\'' ]{6,}' "$SCOPE" 2>/dev/null | $RG -vi '(test|example|placeholder|dummy|fake|TODO)' || true)"
KEYS="$($RG -n -- '-----BEGIN [A-Z ]*PRIVATE KEY-----' "$SCOPE" 2>/dev/null || true)"
hit HIGH "possible hardcoded secret" "$SECRETS"
hit HIGH "embedded private key" "$KEYS"

# --- HIGH: C++ extern "C" without a catch(...) backstop (exception must not cross FFI) ---
for f in $($RG -l 'extern "C"' "$SCOPE" -g '*.cpp' 2>/dev/null || true); do
  EXT=$($RG -c '^extern "C"' "$f" 2>/dev/null || echo 0)   # anchored: real definitions, not comment mentions
  CAT=$($RG -c 'catch \(\.\.\.\)' "$f" 2>/dev/null || echo 0)
  if [ "${EXT:-0}" -gt "${CAT:-0}" ]; then
    hit HIGH "C++ FFI function may lack catch(...) backstop" "$f: extern\"C\"=$EXT catch(...)=$CAT"
  fi
done

# --- HIGH: Rust extern "C" exports without catch_unwind (panic must not cross FFI) ---
for f in $($RG -l 'extern "C"' "$SCOPE" -g '*.rs' 2>/dev/null || true); do
  # Only relevant where we DEFINE exports (fn body), not extern blocks that just declare.
  if $RG -q 'pub (unsafe )?extern "C" fn' "$f" 2>/dev/null && ! $RG -q 'catch_unwind' "$f" 2>/dev/null; then
    hit HIGH "Rust extern \"C\" export without catch_unwind" "$f"
  fi
done

# --- MED: process spawning (command-injection surface) ---
SPAWN="$($RG -n 'Command::new|process::Command|std::process' "$SCOPE" -g '*.rs' 2>/dev/null || true)"
hit MED "process spawning — verify args are not attacker-influenced" "$SPAWN"

# --- MED: local-file / SSRF surface (wkhtmltopdf's historical CVE class) ---
SSRF="$($RG -n 'file://|allow_local_file_access|allow-file-access|169\.254|127\.0\.0\.1|metadata' "$SCOPE" -g '*.rs' 2>/dev/null || true)"
hit MED "file:// / SSRF surface — must be gated by ResourcePolicy (allowlist + redirect re-validation)" "$SSRF"

# --- MED: JS injected into the page (DOM/script injection) ---
JSINJ="$($RG -n 'eval_json|Runtime.evaluate|format!\("' "$SCOPE/crates/wkhtmltox-render-chromium" -g '*.rs' 2>/dev/null | $RG -i 'eval|evaluate' || true)"
hit MED "JS evaluated in page — ensure no untrusted interpolation into the script string" "$JSINJ"

# --- MED: panic/unwrap in FFI crates (should be handled, not panic across the boundary) ---
PANIC="$($RG -n '\.unwrap\(\)|\.expect\(|panic!\(|unreachable!\(' "$SCOPE/crates/wkhtmltox-pdf-sys" "$SCOPE/crates/wkhtmltox-capi" -g '*.rs' 2>/dev/null | $RG -v '#\[cfg\(test\)\]|tests?/' || true)"
hit MED "unwrap/expect/panic in an FFI crate (outside tests)" "$PANIC"

# --- LOW: security-relevant TODO/FIXME ---
TODO="$($RG -n 'TODO|FIXME|XXX|HACK' "$SCOPE" -g '*.rs' -g '*.cpp' 2>/dev/null | $RG -i 'sec|safe|unsafe|auth|sanitiz|valid|escap' || true)"
hit LOW "security-relevant TODO/FIXME" "$TODO"

# --- Dependency advisories (cargo-audit if present) ---
say ""; say "### Dependency advisories"
if command -v cargo-audit >/dev/null 2>&1; then
  AUD="$(cd "$SCOPE" && cargo audit 2>&1)"; RC=$?   # exit!=0 == actual vulnerabilities (warnings are allowed/exit 0)
  say '```'; say "$(printf '%s\n' "$AUD" | tail -40)"; say '```'
  if [ "$RC" -ne 0 ]; then hit HIGH "cargo audit found vulnerabilities (exit $RC)" "see Dependency advisories section"; fi
  if printf '%s\n' "$AUD" | $RG -q 'unmaintained'; then
    hit LOW "unmaintained dependency (advisory warning, not a vulnerability)" "$(printf '%s\n' "$AUD" | $RG 'Crate:|Title:|ID:' | head -8)"
  fi
else
  say "_cargo-audit not installed — RUSTSEC advisory scan skipped (install: cargo install cargo-audit)._"
fi

say ""; say "## Summary: HIGH=$HIGH  MEDIUM=$MED  LOW=$LOW"
say "_Static audit only. A semantic LLM review (the security-review skill) should run at each feature boundary for deeper coverage (SSRF logic, auth, injection)._"

# Console summary (becomes the asyncRewake message on exit 2).
echo "security-audit @ $SHA: HIGH=$HIGH MEDIUM=$MED LOW=$LOW -> $REPORT"
if [ "$HIGH" -gt 0 ]; then
  echo "HIGH-severity security findings after a feature commit — review $REPORT before continuing."
  exit 2
fi
exit 0
