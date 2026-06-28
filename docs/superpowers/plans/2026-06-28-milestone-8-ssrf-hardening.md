# wkhtmltox-rs Milestone 8 — SSRF / security hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Close the SSRF/local-file gaps that survived M4 — confirmed by an adversarially-verified gap audit — so `--safe` blocks `localhost`, hostnames that resolve to private IPs, and the missing private ranges (CGNAT, NAT64, 0/8).

**Architecture:** Extend the pure `ResourcePolicy` (core) with (1) the missing private IP ranges, (2) well-known loopback-hostname blocking, and (3) DNS resolution of non-IP hosts via an **injected `Resolver` trait** — so the policy stays exhaustively unit-testable without network/browser, while the Chromium renderer wires in a real `SystemResolver`. The literal-`decide()` path is preserved (delegates to a `NoopResolver`).

**Tech Stack:** Rust 2021 (`wkhtmltox-core` stays `#![forbid(unsafe_code)]`), `std::net` (resolution), the existing CDP Fetch enforcement in `wkhtmltox-render-chromium`.

## Global Constraints

- `wkhtmltox-core` + `wkhtmltox-render-chromium` stay `#![forbid(unsafe_code)]`.
- **Preserve the permissive default** (drop-in compat): `ResourcePolicy::default()` blocks nothing new. ALL new blocking is gated behind `block_private_ips` (i.e. `--safe`/`safe_profile()`).
- The policy module stays **pure and unit-testable without I/O** — DNS is reached only through the injected `Resolver` trait; the default `decide()` uses a `NoopResolver` (no network), so existing pure tests keep working.
- Reuse the existing range helpers (`is_private_ipv4`/`is_private_ipv6`) — do not duplicate range logic.
- LGPL header on any new file; `cargo clippy --lib -- -D warnings` clean on touched crates. Commit per task + trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Working dir for commands: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs`.

## Verified gaps this milestone closes (from the adversarial audit `wf_a415e345-d4a`)
- **localhost-alias** (HIGH): `http://localhost/` allowed under `--safe`.
- **hostname-resolves-to-private** (HIGH): a host whose DNS A/AAAA is private (e.g. `metadata.evil.com`→`169.254.169.254`) allowed.
- **cgnat-100-64** (MED): `100.64.0.0/10` not blocked.
- **nat64-prefix** (MED): `[64:ff9b::<private-v4>]` not blocked.
- **zero-slash-8** (LOW): only bare `0.0.0.0` blocked, not the rest of `0.0.0.0/8`.
- **toctou-dns-rebinding** (HIGH): PARTIAL — static records now blocked; active TTL=0 rebinding between our resolve and Chrome's connect is **documented as residual** (full fix needs IP-pinning / a forward proxy; out of scope, see Task 3 docs).

(Refuted by the audit, NOT in scope — Chrome normalizes these before our policy sees them, or re-fires per redirect hop: percent-encoded host, trailing-dot FQDN, expanded IPv4-mapped IPv6, redirect-URL/`C1b`/data-uri claims. Do NOT "fix" them.)

---

## File Structure
- `crates/wkhtmltox-core/src/policy.rs` — MODIFY: extend `is_private_ipv4`/`is_private_ipv6`; add `Resolver` trait + `SystemResolver` + `NoopResolver`; `is_loopback_hostname`; `ip_addr_is_private`; `decide_with_resolver`; `decide` delegates.
- `crates/wkhtmltox-render-chromium/src/renderer.rs` — MODIFY: call `decide_with_resolver(.., &SystemResolver)` in the Fetch enforcement path; gated e2e.

---

### Task 1 — Missing private IP ranges (pure helper fixes)

**Files:** Modify `crates/wkhtmltox-core/src/policy.rs` (`is_private_ipv4`, `is_private_ipv6`, doc table, tests).
**Interfaces:** Consumes/produces the existing private functions `is_private_ipv4(ip: [u8;4]) -> bool`, `is_private_ipv6(ip: [u8;16]) -> bool`. No signature change.

- [ ] **Step 1: Write failing tests** (in the policy `#[cfg(test)] mod tests`):

```rust
#[test]
fn safe_blocks_cgnat_100_64() {
    let p = ResourcePolicy::safe_profile();
    assert!(matches!(p.decide("http://100.64.0.1/", false), Decision::Block(_)));
    assert!(matches!(p.decide("http://100.127.255.255/", false), Decision::Block(_)));
    // boundary: 100.63 and 100.128 are public, must stay allowed
    assert!(matches!(p.decide("http://100.63.0.1/", false), Decision::Allow));
    assert!(matches!(p.decide("http://100.128.0.1/", false), Decision::Allow));
}

#[test]
fn safe_blocks_zero_slash_8_non_zero() {
    let p = ResourcePolicy::safe_profile();
    assert!(matches!(p.decide("http://0.0.0.1/", false), Decision::Block(_)));
    assert!(matches!(p.decide("http://0.255.255.255/", false), Decision::Block(_)));
}

#[test]
fn safe_blocks_nat64_embedded_private() {
    let p = ResourcePolicy::safe_profile();
    // 64:ff9b::10.0.0.1 embeds RFC1918 10.0.0.1
    assert!(matches!(p.decide("http://[64:ff9b::10.0.0.1]/", false), Decision::Block(_)));
    assert!(is_private_ip("[64:ff9b::169.254.169.254]"));
    // NAT64 embedding a PUBLIC v4 stays allowed (only the embedded addr matters)
    assert!(!is_private_ip("[64:ff9b::8.8.8.8]"));
}
```

There is an existing test asserting only `0.0.0.0` is blocked (the audit cited lines ~917-920, e.g. `assert!(!is_private_ip("0.0.0.1"))`). FIND it and UPDATE it to the new behavior (`0.0.0.1` is now private). That is an intended behavior change, not a regression.

- [ ] **Step 2: Run, verify failure:** `cargo test -p wkhtmltox-core policy::` → the 3 new tests fail.

- [ ] **Step 3: Extend `is_private_ipv4`** — add CGNAT and widen 0/8:

```rust
fn is_private_ipv4(ip: [u8; 4]) -> bool {
    let [a, b, _, _] = ip;
    match a {
        127 => true,                            // 127.0.0.0/8  loopback
        10 => true,                             // 10.0.0.0/8   private (RFC 1918)
        172 if (16..=31).contains(&b) => true,  // 172.16.0.0/12 private (RFC 1918)
        192 if b == 168 => true,                // 192.168.0.0/16 private (RFC 1918)
        169 if b == 254 => true,                // 169.254.0.0/16 link-local (RFC 3927)
        100 if (64..=127).contains(&b) => true, // 100.64.0.0/10 CGNAT (RFC 6598)
        0 => true,                              // 0.0.0.0/8 "this network" (RFC 1122)
        _ => false,
    }
}
```

- [ ] **Step 4: Extend `is_private_ipv6`** — add the NAT64 well-known prefix `64:ff9b::/96`, delegating the embedded IPv4 to `is_private_ipv4` (insert before the final `false`):

```rust
    // 64:ff9b::/96 — NAT64 well-known prefix (RFC 6146); last 32 bits embed an IPv4.
    if ip[0] == 0x00 && ip[1] == 0x64 && ip[2] == 0xFF && ip[3] == 0x9B
        && ip[4..12] == [0u8; 8]
    {
        return is_private_ipv4([ip[12], ip[13], ip[14], ip[15]]);
    }
```

- [ ] **Step 5: Update the `is_private_ip` doc table** to list the three new ranges (`100.64.0.0/10` CGNAT, `0.0.0.0/8`, `64:ff9b::/96` NAT64).

- [ ] **Step 6: Run, verify pass + no regression:** `cargo test -p wkhtmltox-core && cargo clippy -p wkhtmltox-core --lib -- -D warnings`.

- [ ] **Step 7: Commit** `feat(policy): block CGNAT 100.64/10, full 0.0.0.0/8, NAT64 64:ff9b::/96 under --safe`.

---

### Task 2 — Resolver trait + hostname resolution + loopback aliases

**Files:** Modify `crates/wkhtmltox-core/src/policy.rs`.
**Interfaces:**
- Produces: `pub trait Resolver { fn resolve(&self, host: &str) -> Vec<std::net::IpAddr>; }`; `pub struct SystemResolver;` (impl); `pub struct NoopResolver;` (impl, returns empty); `pub fn ResourcePolicy::decide_with_resolver(&self, url: &str, is_redirect: bool, resolver: &dyn Resolver) -> Decision`.
- Consumes: `is_private_ipv4`/`is_private_ipv6` (Task 1), `extract_http_host`, `parse_ipv4`/`parse_ipv6`, `is_private_ip`.
- `decide(&self, url, is_redirect)` is preserved as `self.decide_with_resolver(url, is_redirect, &NoopResolver)`.

- [ ] **Step 1: Write failing tests** (policy test module):

```rust
use std::collections::HashMap;
use std::net::IpAddr;

/// Test resolver mapping hostnames to fixed IPs.
struct MockResolver(HashMap<String, Vec<IpAddr>>);
impl Resolver for MockResolver {
    fn resolve(&self, host: &str) -> Vec<IpAddr> {
        self.0.get(host).cloned().unwrap_or_default()
    }
}
fn mock(pairs: &[(&str, &str)]) -> MockResolver {
    MockResolver(pairs.iter().map(|(h, ip)| {
        ((*h).to_string(), vec![ip.parse::<IpAddr>().unwrap()])
    }).collect())
}

#[test]
fn safe_blocks_localhost_hostname() {
    let p = ResourcePolicy::safe_profile();
    // No resolver needed — the loopback-alias check is pure.
    assert!(matches!(p.decide("http://localhost/admin", false), Decision::Block(_)));
    assert!(matches!(p.decide("http://localhost:6379/", false), Decision::Block(_)));
    assert!(matches!(p.decide("http://app.localhost/", false), Decision::Block(_)));
    assert!(matches!(p.decide("http://ip6-localhost/", false), Decision::Block(_)));
}

#[test]
fn safe_blocks_hostname_resolving_to_private() {
    let p = ResourcePolicy::safe_profile();
    let r = mock(&[("metadata.evil.com", "169.254.169.254"), ("intranet.example", "10.0.0.5")]);
    assert!(matches!(p.decide_with_resolver("http://metadata.evil.com/latest/", false, &r), Decision::Block(_)));
    assert!(matches!(p.decide_with_resolver("http://intranet.example/", false, &r), Decision::Block(_)));
}

#[test]
fn safe_allows_hostname_resolving_to_public() {
    let p = ResourcePolicy::safe_profile();
    let r = mock(&[("example.com", "93.184.216.34")]);
    assert!(matches!(p.decide_with_resolver("http://example.com/", false, &r), Decision::Allow));
}

#[test]
fn default_permissive_allows_localhost_and_private_hostnames() {
    // The default profile must NOT block (drop-in compat).
    let p = ResourcePolicy::default();
    let r = mock(&[("metadata.evil.com", "169.254.169.254")]);
    assert!(matches!(p.decide("http://localhost/", false), Decision::Allow));
    assert!(matches!(p.decide_with_resolver("http://metadata.evil.com/", false, &r), Decision::Allow));
}

#[test]
fn safe_unresolvable_host_is_allowed_no_private_observed() {
    let p = ResourcePolicy::safe_profile();
    let r = mock(&[]); // resolves to nothing
    // No private IP observed → allow (Chrome will simply fail to connect).
    assert!(matches!(p.decide_with_resolver("http://nonexistent.invalid/", false, &r), Decision::Allow));
}
```

- [ ] **Step 2: Run, verify failure:** `cargo test -p wkhtmltox-core policy::` → new tests fail (no `decide_with_resolver`, localhost not blocked).

- [ ] **Step 3: Add the `Resolver` trait + impls** (after the `Decision` enum / near the top of the module):

```rust
/// Resolves a hostname to its IP addresses. Injected so the policy stays
/// pure/unit-testable; the renderer supplies a real [`SystemResolver`].
pub trait Resolver {
    /// Return the resolved IPs, or an empty vec on failure/unknown host.
    fn resolve(&self, host: &str) -> Vec<std::net::IpAddr>;
}

/// Production resolver using the OS resolver. NOTE: blocking; relies on the
/// OS resolver timeout. Used only on the renderer's enforcement path.
pub struct SystemResolver;
impl Resolver for SystemResolver {
    fn resolve(&self, host: &str) -> Vec<std::net::IpAddr> {
        use std::net::ToSocketAddrs;
        (host, 0u16)
            .to_socket_addrs()
            .map(|it| it.map(|sa| sa.ip()).collect())
            .unwrap_or_default()
    }
}

/// Resolver that never resolves — backs the pure `decide` (literal-IP +
/// loopback-alias checks only, no network).
pub struct NoopResolver;
impl Resolver for NoopResolver {
    fn resolve(&self, _host: &str) -> Vec<std::net::IpAddr> {
        Vec::new()
    }
}
```

- [ ] **Step 4: Add helpers** (near `is_private_ip`):

```rust
/// Well-known loopback hostnames that always resolve to 127.0.0.1/::1
/// (RFC 6761 §6.3 reserves `localhost` and the `.localhost` TLD).
fn is_loopback_hostname(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h == "ip6-localhost" || h == "ip6-loopback"
}

/// `true` when `host` parses as a literal IP (so it needs no DNS resolution).
fn host_is_ip_literal(host: &str) -> bool {
    let h = host.trim();
    let addr = if h.starts_with('[') && h.ends_with(']') { &h[1..h.len() - 1] } else { h };
    parse_ipv4(addr).is_some() || parse_ipv6(addr).is_some()
}

fn ip_addr_is_private(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_ipv4(v4.octets()),
        std::net::IpAddr::V6(v6) => is_private_ipv6(v6.octets()),
    }
}
```

- [ ] **Step 5: Refactor `decide` → `decide_with_resolver`.** Rename the current `decide` body to `decide_with_resolver(&self, url: &str, _is_redirect: bool, resolver: &dyn Resolver)`, and in its `http`/`https` arm replace the block-private section with:

```rust
"http" | "https" => {
    if self.block_private_ips {
        let host = extract_http_host(rest);
        // 1. Literal private/loopback/link-local IP.
        if is_private_ip(host) {
            return Decision::Block(format!(
                "blocked: private/loopback host {:?} (SSRF protection)", host));
        }
        // 2. Well-known loopback hostname alias (deterministic, no DNS).
        if is_loopback_hostname(host) {
            return Decision::Block(format!(
                "blocked: loopback hostname {:?} (SSRF protection)", host));
        }
        // 3. Resolve a DNS hostname and block if ANY resolved IP is private.
        if !host_is_ip_literal(host) {
            for ip in resolver.resolve(host) {
                if ip_addr_is_private(ip) {
                    return Decision::Block(format!(
                        "blocked: host {:?} resolves to private IP {} (SSRF protection)", host, ip));
                }
            }
        }
    }
    Decision::Allow
}
```

Then add the thin delegator:

```rust
pub fn decide(&self, url: &str, is_redirect: bool) -> Decision {
    self.decide_with_resolver(url, is_redirect, &NoopResolver)
}
```

- [ ] **Step 6: Run, verify pass:** `cargo test -p wkhtmltox-core && cargo clippy -p wkhtmltox-core --lib -- -D warnings`. Fix any existing decide-level test that assumed `localhost` is allowed under `--safe` (intended change).

- [ ] **Step 7: Commit** `feat(policy): resolve DNS hosts + block loopback aliases under --safe (SSRF, injectable Resolver)`.

---

### Task 3 — Renderer enforcement wiring + gated e2e + docs

**Files:** Modify `crates/wkhtmltox-render-chromium/src/renderer.rs`; update `crates/wkhtmltox-core/src/policy.rs` module docs.
**Interfaces:** Consumes `decide_with_resolver` + `SystemResolver` (Task 2).

- [ ] **Step 1: Switch the Fetch enforcement to the resolver path.** In `renderer.rs`, find every call to `policy.decide(url, is_redirect)` on the enforcement path (the audit cited `fetch_action` and the `open()` pump, ~lines 676–728). Change them to `policy.decide_with_resolver(url, is_redirect, &wkhtmltox_core::policy::SystemResolver)`. `SystemResolver` is a zero-sized unit struct, so construct it inline at the call site. Do NOT change any other M4 behavior (C1b document-allow, redirect re-validation, image-block, per-origin headers).

- [ ] **Step 2: Add a gated e2e** (real Chrome) in `crates/wkhtmltox-render-chromium/tests/chromium_e2e.rs` — model it on the existing `snapshot_under_safe_policy_blocks_private_ip_subresource`. Use the deterministic `localhost` alias (needs no external DNS): a page whose subresource targets `http://localhost:<closed-port>/x` under `--safe` must be BLOCKED (assert via the renderer's `blocked_urls` / the existing block-assertion mechanism the M4 test uses), while the page itself still renders:

```rust
#[test]
#[ignore = "requires a real Chrome; run with: cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1"]
fn snapshot_under_safe_blocks_localhost_subresource() {
    // Mirror the structure of snapshot_under_safe_policy_blocks_private_ip_subresource,
    // but point the subresource at http://localhost:9/ (discard port). Under --safe
    // the loopback-hostname rule must block it; assert it appears in blocked_urls.
    // (Match the real helper/setup that test uses — reuse it.)
}
```

Match the actual setup/assertion API of the existing M4 safe-policy test (read it first; reuse its harness rather than inventing one).

- [ ] **Step 3: Update `policy.rs` module docs** (the three "DNS-rebinding is a known post-v1 limitation" notes at the module header, the `block_private_ips` field doc, and the `is_private_ip` doc). Rewrite them to state the new reality accurately:
  - Literal-IP, loopback-hostname (`localhost`/`.localhost`/`ip6-*`), and **DNS-resolved** private IPs are now blocked under `--safe` (the renderer supplies a `SystemResolver`; the pure `decide`/`is_private_ip` remain literal-only by design).
  - **Residual limitation (document honestly):** *active* DNS-rebinding — a host that resolves PUBLIC at our `decide_with_resolver` check and PRIVATE at Chrome's subsequent connect (TTL=0) — is still possible, because this CDP-subprocess architecture cannot pin the IP Chrome connects to per-request. A complete fix requires IP-pinning (URL rewrite for HTTP / `--host-resolver-rules`) or a controlled forward proxy; tracked as a future milestone. Note also that `SystemResolver` performs a blocking lookup on the enforcement path (bounded by the OS resolver timeout + the renderer's overall deadline).

- [ ] **Step 4: Verify:** `cargo build -p wkhtmltox-render-chromium --tests && cargo clippy -p wkhtmltox-render-chromium --lib -- -D warnings`. If Chrome is available, run the new gated e2e: `cargo test -p wkhtmltox-render-chromium -- --ignored --test-threads=1 snapshot_under_safe_blocks_localhost_subresource` and report; else note it compiles + the policy unit tests are the evidence.

- [ ] **Step 5: Commit** `feat(chromium): enforce DNS-resolved SSRF policy (SystemResolver) + document residual rebinding`.

---

## Milestone Gate (controller-run)

After Tasks 1–3 review clean:
1. **Opus security review** over the full M8 diff (`scripts/review-package <M8-base> HEAD`): verify the new blocking is gated behind `block_private_ips` only (default stays permissive); resolver injection can't be bypassed; the loopback-alias + range additions have no false-positives on public addresses (boundary checks: `100.63`/`100.128` public, `172.15`/`172.32` public); no `unsafe`; the renderer still honors all M4 protections. Confirm the residual TOCTOU is documented, not silently claimed-fixed.
2. **Static audit:** `bash scripts/security-audit.sh --full` (expect HIGH=0).
3. **Fix wave** for any Critical/Important.
4. **Push**; update `TODO.md` (M8 → Done; mark the audit-confirmed gaps closed; note residual active-rebinding) + `logbook.md`.

## Self-Review
- **Gap coverage:** localhost → Task 2 (loopback alias); hostname→private → Task 2 (resolver); CGNAT/0-8/NAT64 → Task 1; active-rebinding TOCTOU → documented residual (Task 3). All 6 audit-confirmed gaps addressed (5 closed, 1 narrowed+documented).
- **Compat:** every new block is behind `block_private_ips`; `default()` unchanged; `decide()` stays pure (NoopResolver) so non-renderer callers and existing unit tests are unaffected except the intended localhost/0.0.0.1 behavior changes.
- **No placeholder:** real code for every helper, the trait, `decide_with_resolver`, and tests. The e2e step points the implementer at the existing M4 safe-policy test to reuse its harness (real existing code, not undefined).
- **Type consistency:** `Resolver`/`SystemResolver`/`NoopResolver`, `decide_with_resolver(url, is_redirect, &dyn Resolver)`, `ip_addr_is_private`, `is_loopback_hostname`, `host_is_ip_literal` used identically across tasks. Range helpers keep their `[u8;4]`/`[u8;16]` signatures.
