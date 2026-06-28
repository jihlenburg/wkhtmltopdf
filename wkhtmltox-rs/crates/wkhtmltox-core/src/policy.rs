// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// ResourcePolicy: SSRF + local-file gating.
//
// Pure logic module — no I/O, no browser, no network calls. The intent is
// that this module is exhaustively unit-testable without any external
// dependencies and forms the single authoritative security boundary for all
// URL decisions.
//
// # Design notes
//
// * Default = **permissive** (matches historic wkhtmltopdf behaviour so
//   existing deployments get a drop-in upgrade).  Use `safe_profile()` or
//   `--safe` to harden.
//
// * `decide` is called **once per URL**, including redirect targets — the
//   redirect-to-`file://` hole is closed by always re-validating the
//   destination URL rather than trusting the redirect chain.
//
// * Literal-IP SSRF blocking only.  DNS-rebinding (a hostname that initially
//   resolves to a public IP, then to a private one) is a known post-v1
//   limitation; it is not addressed here.

/// Whether a URL request should be allowed or blocked, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// The URL is blocked; the `String` contains a human-readable reason.
    Block(String),
}

/// Policy governing which URLs a renderer is permitted to load.
///
/// The **default** is permissive (matching historic wkhtmltopdf behaviour) so
/// that existing users have a drop-in compatible upgrade path.  Use
/// [`ResourcePolicy::safe_profile`] for the hardened `--safe` mode.
#[derive(Debug, Clone)]
pub struct ResourcePolicy {
    /// Permit `file://` (and other local-resource) URLs.
    pub allow_local_file: bool,

    /// Paths that are always permitted even when `allow_local_file` is false.
    ///
    /// Each entry is matched as a path *prefix* against the decoded path
    /// component of the URL.  A separator (`/`) is required after the prefix
    /// so that `/var/www` does **not** accidentally permit `/var/www-evil`.
    pub allowed_paths: Vec<String>,

    /// Block HTTP/HTTPS requests whose host is a *literal* private, loopback,
    /// or link-local IP address (see [`is_private_ip`] for the full list of
    /// matched ranges).
    ///
    /// Non-IP hostnames are **not** blocked even when this flag is set —
    /// DNS-rebinding is a documented post-v1 limitation.
    pub block_private_ips: bool,

    /// Allow navigational external hyperlinks (e.g. `<a href="…">`).
    /// Does not affect subresource loading; that is governed by the URL checks
    /// above.
    pub allow_external_links: bool,

    /// Allow in-page `#anchor` links.
    pub allow_internal_links: bool,
}

impl Default for ResourcePolicy {
    /// Permissive defaults matching historic wkhtmltopdf behaviour.
    fn default() -> Self {
        Self {
            allow_local_file: true,
            allowed_paths: Vec::new(),
            block_private_ips: false,
            allow_external_links: true,
            allow_internal_links: true,
        }
    }
}

impl ResourcePolicy {
    /// Hardened profile corresponding to `--safe`.
    ///
    /// Disables local-file access and blocks HTTP/HTTPS requests to private,
    /// loopback, and link-local IP addresses.  Link flags are kept `true`
    /// because they govern navigational hyperlinks, not subresource loading.
    pub fn safe_profile() -> Self {
        Self {
            allow_local_file: false,
            allowed_paths: Vec::new(),
            block_private_ips: true,
            allow_external_links: true,
            allow_internal_links: true,
        }
    }

    /// Decide whether `url` may be loaded.
    ///
    /// `is_redirect` signals that this URL is a redirect target.  The *same*
    /// rules apply regardless — redirect targets are re-validated on purpose
    /// to close the redirect-to-`file://` vulnerability class.  The parameter
    /// is accepted so that callers can log whether the decision was for an
    /// original request or a redirect.
    pub fn decide(&self, url: &str, _is_redirect: bool) -> Decision {
        // --- Parse scheme -------------------------------------------------------
        // The scheme is everything before the first ':'.
        let colon = match url.find(':') {
            Some(pos) => pos,
            None => {
                return Decision::Block(format!("blocked: no scheme found in URL {:?}", url));
            }
        };

        let scheme = url[..colon].to_ascii_lowercase();
        let rest = &url[colon + 1..]; // everything after the ':'

        match scheme.as_str() {
            // ----------------------------------------------------------------
            // Inline data — always safe (no network, no filesystem).
            // ----------------------------------------------------------------
            "data" => Decision::Allow,

            // ----------------------------------------------------------------
            // HTTP / HTTPS — optionally block literal private-IP hosts.
            // ----------------------------------------------------------------
            "http" | "https" => {
                if self.block_private_ips {
                    let host = extract_http_host(rest);
                    if is_private_ip(host) {
                        return Decision::Block(format!(
                            "blocked: private/loopback host {:?} (SSRF protection)",
                            host
                        ));
                    }
                }
                Decision::Allow
            }

            // ----------------------------------------------------------------
            // Local-file schemes.
            // ----------------------------------------------------------------
            "file" | "local" => {
                if self.allow_local_file {
                    return Decision::Allow;
                }

                // Check whether the path is under one of the explicitly
                // allowed directory prefixes.
                let path = extract_file_path(rest);
                for allowed in &self.allowed_paths {
                    if path_is_under(path, allowed) {
                        return Decision::Allow;
                    }
                }

                Decision::Block(format!(
                    "blocked: local file access denied (path {:?})",
                    path
                ))
            }

            // ----------------------------------------------------------------
            // Everything else — block conservatively.
            // Covers: javascript:, blob:, about:, chrome:, and unknown
            // schemes.  These may be browser-internal or attacker-controlled.
            // ----------------------------------------------------------------
            _ => Decision::Block(format!(
                "blocked: unsupported scheme {:?} in URL {:?}",
                scheme, url
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Return `true` when `path` is exactly `allowed` or is a sub-path of it.
///
/// A separator (`/`) is always required so that `/var/www` does not match
/// `/var/www-evil`.
fn path_is_under(path: &str, allowed: &str) -> bool {
    // Exact match.
    if path == allowed {
        return true;
    }
    // Sub-path: allowed + '/' must be a prefix of path.
    let prefix_with_sep = if allowed.ends_with('/') {
        // Avoid double slash if the caller already added one.
        allowed.to_string()
    } else {
        format!("{}/", allowed)
    };
    path.starts_with(prefix_with_sep.as_str())
}

// ---------------------------------------------------------------------------
// HTTP host extraction
// ---------------------------------------------------------------------------

/// Extract the host substring from the authority of an `http`/`https` URL.
///
/// `rest` is the URL portion that follows the `:` after the scheme, i.e.
/// for `https://example.com/path` it is `//example.com/path`.
///
/// Returns the host without the port number.  For IPv6 literals the
/// brackets **are** included (e.g. `"[::1]"`) so that [`is_private_ip`]
/// can strip them cleanly.
fn extract_http_host(rest: &str) -> &str {
    // Strip the mandatory `//` authority introducer.
    let after_slashes = rest.strip_prefix("//").unwrap_or(rest);

    // Strip userinfo: drop everything up to and including the LAST '@'
    // that appears inside the authority (before the first path separator).
    // We look only within the authority to avoid stripping a '@' in the path.
    let authority_end = after_slashes
        .find(['/', '?', '#'])
        .unwrap_or(after_slashes.len());
    let authority = &after_slashes[..authority_end];
    let after_userinfo = if let Some(at) = authority.rfind('@') {
        &after_slashes[at + 1..]
    } else {
        after_slashes
    };

    // IPv6 literal `[…]` — the host ends at the closing `]`.
    if after_userinfo.starts_with('[') {
        if let Some(close) = after_userinfo.find(']') {
            return &after_userinfo[..=close];
        }
        // Malformed IPv6 literal — return what we have; it won't parse as an
        // IP so it won't be blocked (conservative: allow unknown).
        return after_userinfo;
    }

    // Ordinary host (IPv4 or DNS name): ends at the first `/`, `:`, `?`, `#`.
    let end = after_userinfo
        .find(['/', ':', '?', '#'])
        .unwrap_or(after_userinfo.len());

    &after_userinfo[..end]
}

// ---------------------------------------------------------------------------
// File path extraction
// ---------------------------------------------------------------------------

/// Extract the path component from the portion of a `file:` URL after the `:`.
///
/// Handles:
/// * `///path` (empty authority → path starts at the third `/`)
/// * `//host/path` (UNC / authority with hostname → strip authority, keep path)
/// * `/path` (relative/authority-free form — unusual)
fn extract_file_path(rest: &str) -> &str {
    if let Some(after_slashes) = rest.strip_prefix("//") {
        // Skip the (possibly empty) authority up to the first path `/`.
        if let Some(slash) = after_slashes.find('/') {
            &after_slashes[slash..]
        } else {
            // Authority-only URL with no path — treat as root.
            "/"
        }
    } else {
        // Authority-free form, rest is already the path.
        rest
    }
}

// ---------------------------------------------------------------------------
// Private-IP detection
// ---------------------------------------------------------------------------

/// Return `true` when `host` is a *literal* private, loopback, link-local, or
/// unspecified IP address.
///
/// Recognised ranges:
///
/// | Range            | Category         |
/// |-----------------|------------------|
/// | `127.0.0.0/8`   | IPv4 loopback    |
/// | `10.0.0.0/8`    | IPv4 private     |
/// | `172.16.0.0/12` | IPv4 private     |
/// | `192.168.0.0/16`| IPv4 private     |
/// | `169.254.0.0/16`| IPv4 link-local  |
/// | `0.0.0.0`       | IPv4 unspecified |
/// | `::1`           | IPv6 loopback    |
/// | `fc00::/7`      | IPv6 unique-local|
/// | `fe80::/10`     | IPv6 link-local  |
///
/// `host` may be:
/// * A bare IPv4 address: `"127.0.0.1"`
/// * A bare IPv6 address: `"::1"`, `"fc00::1"`
/// * A bracketed IPv6 address as it appears in URLs: `"[::1]"`, `"[fe80::1]"`
/// * Any other string (DNS name, etc.) — returns `false` without error.
///
/// **DNS-rebinding note:** this function tests only the *literal* value of
/// `host`; it does not perform DNS resolution.  A hostname that resolves to a
/// private IP is therefore not caught here — that is a known post-v1
/// limitation.
pub fn is_private_ip(host: &str) -> bool {
    let host = host.trim();

    // Strip brackets for IPv6 literals like "[::1]".
    let addr = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };

    // Try IPv4 first (more common in SSRF payloads).
    if let Some(ipv4) = parse_ipv4(addr) {
        return is_private_ipv4(ipv4);
    }

    // Try IPv6.
    if let Some(ipv6) = parse_ipv6(addr) {
        return is_private_ipv6(ipv6);
    }

    // Not an IP address (must be a DNS hostname) — not private.
    false
}

// ---------------------------------------------------------------------------
// IPv4 helpers
// ---------------------------------------------------------------------------

/// Parse an integer in decimal, hex (`0x…`/`0X…`), or octal (`0…`) notation.
fn parse_int32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if s.starts_with('0') && s.len() > 1 {
        // Leading zero → octal.
        u32::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// Parse one octet that may be decimal, hex, or octal. Returns `None` if value exceeds 255.
fn parse_octet(s: &str) -> Option<u8> {
    let n = parse_int32(s)?;
    if n > 255 {
        return None;
    }
    Some(n as u8)
}

/// Parse an IPv4 address, accepting dotted-decimal/hex/octal and single 32-bit integers.
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    if !s.contains('.') {
        // Single-integer form: decimal, hex, or octal 32-bit value.
        let n = parse_int32(s)?;
        return Some([
            ((n >> 24) & 0xFF) as u8,
            ((n >> 16) & 0xFF) as u8,
            ((n >> 8) & 0xFF) as u8,
            (n & 0xFF) as u8,
        ]);
    }

    // Dotted form: each part may be decimal, hex, or octal.
    let mut iter = s.splitn(5, '.');
    let a = parse_octet(iter.next()?)?;
    let b = parse_octet(iter.next()?)?;
    let c = parse_octet(iter.next()?)?;
    let d_str = iter.next()?;
    let d = parse_octet(d_str)?;
    if iter.next().is_some() {
        return None;
    }
    Some([a, b, c, d])
}

fn is_private_ipv4(ip: [u8; 4]) -> bool {
    let [a, b, _, _] = ip;
    match a {
        127 => true,                           // 127.0.0.0/8  loopback
        10 => true,                            // 10.0.0.0/8   private (RFC 1918)
        172 if (16..=31).contains(&b) => true, // 172.16.0.0/12 private (RFC 1918)
        192 if b == 168 => true,               // 192.168.0.0/16 private (RFC 1918)
        169 if b == 254 => true,               // 169.254.0.0/16 link-local (RFC 3927)
        0 if ip == [0, 0, 0, 0] => true,       // 0.0.0.0 unspecified
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// IPv6 helpers
// ---------------------------------------------------------------------------

/// Parse an IPv6 address string (without brackets) into 16 bytes, returning
/// `None` for any string that is not a syntactically valid IPv6 address.
///
/// Handles the `::` zero-run abbreviation, fully-expanded addresses, and
/// mixed IPv4-mapped notation (`::ffff:127.0.0.1`).
fn parse_ipv6(s: &str) -> Option<[u8; 16]> {
    // Strip any zone ID (e.g. "%25eth0" in a percent-encoded URL).
    let s = match s.find('%') {
        Some(pos) => &s[..pos],
        None => s,
    };

    // Split on `::` (zero-run abbreviation).
    let (left_str, right_raw, has_double_colon) = if let Some(pos) = s.find("::") {
        (&s[..pos], &s[pos + 2..], true)
    } else {
        (s, "", false)
    };

    // Detect mixed IPv4-in-IPv6 notation on the right side, e.g. `::ffff:127.0.0.1`.
    // The IPv4 literal is the segment after the last ':' when it contains a '.'.
    let (right_hex, embedded_v4): (&str, Option<[u8; 4]>) = if right_raw.contains('.') {
        if let Some(last_colon) = right_raw.rfind(':') {
            let ipv4_str = &right_raw[last_colon + 1..];
            let hex_part = &right_raw[..last_colon];
            let v4 = parse_ipv4(ipv4_str)?;
            (hex_part, Some(v4))
        } else {
            // Entire right side is IPv4: e.g. `::1.2.3.4`
            let v4 = parse_ipv4(right_raw)?;
            ("", Some(v4))
        }
    } else {
        (right_raw, None)
    };

    let left_groups = parse_ipv6_groups(left_str)?;
    let right_groups = parse_ipv6_groups(right_hex)?;

    // Each embedded IPv4 octet-pair counts as one u16 group (2 groups total).
    let v4_count = if embedded_v4.is_some() { 2 } else { 0 };
    let total = left_groups.len() + right_groups.len() + v4_count;

    let mut groups: Vec<u16> = if has_double_colon {
        if total > 8 {
            return None;
        }
        let mut g = left_groups;
        let target_len = 8 - right_groups.len() - v4_count;
        g.resize(target_len, 0u16);
        g.extend(right_groups);
        g
    } else {
        if total != 8 {
            return None;
        }
        let mut g = left_groups;
        g.extend(right_groups);
        g
    };

    // Append the embedded IPv4 as two big-endian u16 words.
    if let Some([a, b, c, d]) = embedded_v4 {
        groups.push(u16::from_be_bytes([a, b]));
        groups.push(u16::from_be_bytes([c, d]));
    }

    ipv6_groups_to_bytes(&groups)
}

/// Parse a colon-separated sequence of hex groups, returning an empty `Vec`
/// for an empty string.  Returns `None` if any group is invalid.
fn parse_ipv6_groups(part: &str) -> Option<Vec<u16>> {
    if part.is_empty() {
        return Some(Vec::new());
    }
    part.split(':')
        .map(|g| u16::from_str_radix(g, 16).ok())
        .collect()
}

fn ipv6_groups_to_bytes(groups: &[u16]) -> Option<[u8; 16]> {
    if groups.len() != 8 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, &g) in groups.iter().enumerate() {
        out[2 * i] = (g >> 8) as u8;
        out[2 * i + 1] = (g & 0xFF) as u8;
    }
    Some(out)
}

fn is_private_ipv6(ip: [u8; 16]) -> bool {
    // ::1 — loopback (RFC 4291)
    if ip == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] {
        return true;
    }

    // fc00::/7 — unique-local (RFC 4193)
    if ip[0] & 0xFE == 0xFC {
        return true;
    }

    // fe80::/10 — link-local (RFC 4291)
    if ip[0] == 0xFE && (ip[1] & 0xC0 == 0x80) {
        return true;
    }

    // ::ffff:0:0/96 — IPv4-mapped (RFC 4291 §2.5.5.2).
    if ip[..10] == [0u8; 10] && ip[10] == 0xFF && ip[11] == 0xFF {
        return is_private_ipv4([ip[12], ip[13], ip[14], ip[15]]);
    }

    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Default (permissive) policy
    // ========================================================================

    #[test]
    fn default_allows_file_etc_passwd() {
        let p = ResourcePolicy::default();
        assert_eq!(p.decide("file:///etc/passwd", false), Decision::Allow);
    }

    #[test]
    fn default_allows_http_example() {
        let p = ResourcePolicy::default();
        assert_eq!(p.decide("http://example.com/", false), Decision::Allow);
    }

    #[test]
    fn default_allows_link_local_imds() {
        // Default does NOT block private IPs — permissive compatibility mode.
        let p = ResourcePolicy::default();
        assert_eq!(
            p.decide("http://169.254.169.254/latest/meta-data/", false),
            Decision::Allow
        );
    }

    #[test]
    fn default_allows_https() {
        let p = ResourcePolicy::default();
        assert_eq!(p.decide("https://example.com/page", false), Decision::Allow);
    }

    // ========================================================================
    // safe_profile: local-file blocking
    // ========================================================================

    #[test]
    fn safe_blocks_file_etc_passwd() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("file:///etc/passwd", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_local_scheme() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("local:///some/path", false),
            Decision::Block(_)
        ));
    }

    // ========================================================================
    // safe_profile: SSRF private-IP blocking
    // ========================================================================

    #[test]
    fn safe_blocks_imds_link_local() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://169.254.169.254/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_loopback_with_port() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://127.0.0.1:9333/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_10_slash_8() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://10.1.2.3/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_192_168() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://192.168.0.1/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_ipv6_loopback_bracketed() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://[::1]/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_172_16_first_addr() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://172.16.0.1/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_172_31_last_addr() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://172.31.255.255/", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_allows_public_http() {
        let p = ResourcePolicy::safe_profile();
        assert_eq!(p.decide("http://example.com/", false), Decision::Allow);
    }

    #[test]
    fn safe_allows_public_https() {
        let p = ResourcePolicy::safe_profile();
        assert_eq!(
            p.decide("https://api.example.com/v1/data", false),
            Decision::Allow
        );
    }

    // ========================================================================
    // allowed_paths: path-prefix gating
    // ========================================================================

    #[test]
    fn allowed_path_permits_direct_subpath() {
        let p = ResourcePolicy {
            allow_local_file: false,
            allowed_paths: vec!["/var/www".to_string()],
            ..ResourcePolicy::safe_profile()
        };
        assert_eq!(p.decide("file:///var/www/x.css", false), Decision::Allow);
    }

    #[test]
    fn allowed_path_permits_nested_subpath() {
        let p = ResourcePolicy {
            allow_local_file: false,
            allowed_paths: vec!["/var/www".to_string()],
            ..ResourcePolicy::safe_profile()
        };
        assert_eq!(
            p.decide("file:///var/www/assets/logo.png", false),
            Decision::Allow
        );
    }

    #[test]
    fn allowed_path_blocks_outside() {
        let p = ResourcePolicy {
            allow_local_file: false,
            allowed_paths: vec!["/var/www".to_string()],
            ..ResourcePolicy::safe_profile()
        };
        assert!(matches!(
            p.decide("file:///etc/passwd", false),
            Decision::Block(_)
        ));
    }

    /// Ensure `/var/www` does not accidentally permit `/var/www-evil`.
    #[test]
    fn allowed_path_does_not_match_sibling_with_suffix() {
        let p = ResourcePolicy {
            allow_local_file: false,
            allowed_paths: vec!["/var/www".to_string()],
            ..ResourcePolicy::safe_profile()
        };
        assert!(matches!(
            p.decide("file:///var/www-evil/secret.txt", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn allowed_path_exact_match_permitted() {
        let p = ResourcePolicy {
            allow_local_file: false,
            allowed_paths: vec!["/var/www".to_string()],
            ..ResourcePolicy::safe_profile()
        };
        // The path itself (not a subdir) should also be allowed.
        assert_eq!(p.decide("file:///var/www", false), Decision::Allow);
    }

    // ========================================================================
    // Redirect targets — same rules apply
    // ========================================================================

    #[test]
    fn safe_blocks_redirect_to_file() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("file:///etc/passwd", true),
            Decision::Block(_)
        ));
    }

    #[test]
    fn safe_blocks_redirect_to_private_ip() {
        let p = ResourcePolicy::safe_profile();
        assert!(matches!(
            p.decide("http://127.0.0.1/internal", true),
            Decision::Block(_)
        ));
    }

    #[test]
    fn default_allows_redirect_to_file() {
        // Permissive profile still allows — the param changes nothing for default.
        let p = ResourcePolicy::default();
        assert_eq!(p.decide("file:///etc/passwd", true), Decision::Allow);
    }

    // ========================================================================
    // data: — always allowed
    // ========================================================================

    #[test]
    fn data_uri_always_allowed_safe() {
        let p = ResourcePolicy::safe_profile();
        assert_eq!(
            p.decide("data:text/html,<h1>hi</h1>", false),
            Decision::Allow
        );
    }

    #[test]
    fn data_uri_base64_allowed() {
        let p = ResourcePolicy::safe_profile();
        assert_eq!(
            p.decide("data:image/png;base64,iVBORw0KGgo=", false),
            Decision::Allow
        );
    }

    // ========================================================================
    // Unknown / unsupported schemes — blocked conservatively
    // ========================================================================

    #[test]
    fn javascript_scheme_blocked() {
        let p = ResourcePolicy::default();
        assert!(matches!(
            p.decide("javascript:alert(1)", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn blob_scheme_blocked() {
        let p = ResourcePolicy::default();
        assert!(matches!(
            p.decide("blob:https://example.com/uuid", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn no_scheme_url_blocked() {
        let p = ResourcePolicy::default();
        assert!(matches!(
            p.decide("//no-scheme/path", false),
            Decision::Block(_)
        ));
    }

    #[test]
    fn empty_url_blocked() {
        let p = ResourcePolicy::default();
        assert!(matches!(p.decide("", false), Decision::Block(_)));
    }

    // ========================================================================
    // is_private_ip — exhaustive range tests
    // ========================================================================

    // --- IPv4 loopback (127.0.0.0/8) ---

    #[test]
    fn loopback_127_0_0_1() {
        assert!(is_private_ip("127.0.0.1"));
    }

    #[test]
    fn loopback_127_first_addr() {
        assert!(is_private_ip("127.0.0.0"));
    }

    #[test]
    fn loopback_127_last_addr() {
        assert!(is_private_ip("127.255.255.255"));
    }

    // --- IPv4 private 10.0.0.0/8 ---

    #[test]
    fn private_10_0_0_0() {
        assert!(is_private_ip("10.0.0.0"));
    }

    #[test]
    fn private_10_1_2_3() {
        assert!(is_private_ip("10.1.2.3"));
    }

    #[test]
    fn private_10_last_addr() {
        assert!(is_private_ip("10.255.255.255"));
    }

    // --- IPv4 private 172.16.0.0/12 ---

    #[test]
    fn private_172_16_first() {
        assert!(is_private_ip("172.16.0.0"));
    }

    #[test]
    fn private_172_31_last() {
        assert!(is_private_ip("172.31.255.255"));
    }

    #[test]
    fn private_172_20_mid() {
        assert!(is_private_ip("172.20.5.5"));
    }

    #[test]
    fn not_private_172_15() {
        // Just outside the /12 range.
        assert!(!is_private_ip("172.15.0.0"));
    }

    #[test]
    fn not_private_172_32() {
        // Just outside the /12 range on the high end.
        assert!(!is_private_ip("172.32.0.0"));
    }

    // --- IPv4 private 192.168.0.0/16 ---

    #[test]
    fn private_192_168_0_0() {
        assert!(is_private_ip("192.168.0.0"));
    }

    #[test]
    fn private_192_168_1_100() {
        assert!(is_private_ip("192.168.1.100"));
    }

    #[test]
    fn not_private_192_169() {
        assert!(!is_private_ip("192.169.0.0"));
    }

    // --- IPv4 link-local 169.254.0.0/16 ---

    #[test]
    fn link_local_169_254_0_0() {
        assert!(is_private_ip("169.254.0.0"));
    }

    #[test]
    fn link_local_169_254_169_254() {
        // The AWS/GCP/Azure IMDS endpoint.
        assert!(is_private_ip("169.254.169.254"));
    }

    #[test]
    fn not_link_local_169_253() {
        assert!(!is_private_ip("169.253.0.0"));
    }

    // --- IPv4 unspecified 0.0.0.0 ---

    #[test]
    fn unspecified_0_0_0_0() {
        assert!(is_private_ip("0.0.0.0"));
    }

    #[test]
    fn not_private_0_0_0_1() {
        // Only 0.0.0.0 exact is blocked, not the rest of 0.0.0.0/8.
        assert!(!is_private_ip("0.0.0.1"));
    }

    // --- Public IPv4 ---

    #[test]
    fn public_8_8_8_8() {
        assert!(!is_private_ip("8.8.8.8"));
    }

    #[test]
    fn public_1_1_1_1() {
        assert!(!is_private_ip("1.1.1.1"));
    }

    #[test]
    fn public_203_0_113() {
        assert!(!is_private_ip("203.0.113.1"));
    }

    // --- Non-IP hostnames ---

    #[test]
    fn hostname_example_com() {
        assert!(!is_private_ip("example.com"));
    }

    #[test]
    fn hostname_localhost_string() {
        // The *string* "localhost" is not an IP; DNS resolution is not done.
        assert!(!is_private_ip("localhost"));
    }

    #[test]
    fn hostname_empty() {
        assert!(!is_private_ip(""));
    }

    // --- IPv6 loopback ::1 ---

    #[test]
    fn ipv6_loopback_bare() {
        assert!(is_private_ip("::1"));
    }

    #[test]
    fn ipv6_loopback_bracketed() {
        assert!(is_private_ip("[::1]"));
    }

    // --- IPv6 unique-local fc00::/7 ---

    #[test]
    fn ipv6_unique_local_fc00() {
        assert!(is_private_ip("fc00::1"));
    }

    #[test]
    fn ipv6_unique_local_fd00() {
        // fd prefix: 11111101 — high 7 bits are 1111110, inside fc00::/7.
        assert!(is_private_ip("fd00::1"));
    }

    #[test]
    fn ipv6_unique_local_fd_complex() {
        assert!(is_private_ip("[fd12:3456:789a:1::1]"));
    }

    // --- IPv6 link-local fe80::/10 ---

    #[test]
    fn ipv6_link_local_fe80_1() {
        assert!(is_private_ip("fe80::1"));
    }

    #[test]
    fn ipv6_link_local_fe80_all_zeros() {
        assert!(is_private_ip("fe80::"));
    }

    #[test]
    fn ipv6_link_local_febf() {
        // febf:: is the last address in fe80::/10.
        assert!(is_private_ip("febf::1"));
    }

    #[test]
    fn ipv6_link_local_fe80_bracketed() {
        assert!(is_private_ip("[fe80::1]"));
    }

    #[test]
    fn not_private_fec0() {
        // fec0:: was site-local (deprecated); not in fe80::/10 or fc00::/7.
        // fec0 = 1111 1110 1100 0000 — byte[1] & 0xC0 = 0xC0 ≠ 0x80 → not link-local.
        // byte[0] & 0xFE = 0xFE ≠ 0xFC → not unique-local.
        assert!(!is_private_ip("fec0::1"));
    }

    // --- Public IPv6 ---

    #[test]
    fn ipv6_public_2001_db8() {
        assert!(!is_private_ip("2001:db8::1"));
    }

    #[test]
    fn ipv6_public_cloudflare_dns() {
        assert!(!is_private_ip("2606:4700:4700::1111"));
    }

    // ========================================================================
    // Internal helper unit tests
    // ========================================================================

    #[test]
    fn extract_http_host_plain() {
        assert_eq!(extract_http_host("//example.com/path"), "example.com");
    }

    #[test]
    fn extract_http_host_with_port() {
        assert_eq!(extract_http_host("//127.0.0.1:8080/"), "127.0.0.1");
    }

    #[test]
    fn extract_http_host_ipv6() {
        assert_eq!(extract_http_host("//[::1]/path"), "[::1]");
    }

    #[test]
    fn extract_http_host_ipv6_with_port() {
        assert_eq!(extract_http_host("//[::1]:8080/path"), "[::1]");
    }

    #[test]
    fn extract_file_path_triple_slash() {
        assert_eq!(extract_file_path("///etc/passwd"), "/etc/passwd");
    }

    #[test]
    fn extract_file_path_with_authority() {
        assert_eq!(
            extract_file_path("//localhost/var/www/x.html"),
            "/var/www/x.html"
        );
    }

    #[test]
    fn path_is_under_exact() {
        assert!(path_is_under("/var/www", "/var/www"));
    }

    #[test]
    fn path_is_under_subdir() {
        assert!(path_is_under("/var/www/sub/file.css", "/var/www"));
    }

    #[test]
    fn path_is_under_sibling_rejected() {
        assert!(!path_is_under("/var/www-evil/secret", "/var/www"));
    }

    #[test]
    fn path_is_under_trailing_slash_in_allowed() {
        // Caller can also pass a trailing-slash prefix.
        assert!(path_is_under("/var/www/file.css", "/var/www/"));
    }

    #[test]
    fn ipv4_parse_rejects_five_octets() {
        assert!(parse_ipv4("1.2.3.4.5").is_none());
    }

    #[test]
    fn ipv4_parse_rejects_hostname() {
        assert!(parse_ipv4("example.com").is_none());
    }

    #[test]
    fn ipv4_parse_rejects_overflow_octet() {
        assert!(parse_ipv4("256.0.0.1").is_none());
    }

    // ========================================================================
    // C2 — userinfo stripping in extract_http_host
    // ========================================================================

    #[test]
    fn extract_http_host_strips_userinfo() {
        assert_eq!(extract_http_host("//user@127.0.0.1/"), "127.0.0.1");
    }

    #[test]
    fn extract_http_host_strips_userinfo_with_port() {
        assert_eq!(
            extract_http_host("//user:pass@example.com:8080/path"),
            "example.com"
        );
    }

    #[test]
    fn extract_http_host_strips_userinfo_ipv6() {
        assert_eq!(extract_http_host("//user@[::1]/path"), "[::1]");
    }

    #[test]
    fn safe_blocks_userinfo_loopback() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(
                p.decide("http://user@127.0.0.1/", false),
                Decision::Block(_)
            ),
            "userinfo@loopback must be blocked under safe"
        );
    }

    #[test]
    fn safe_blocks_userinfo_link_local() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(
                p.decide("http://user:pass@169.254.169.254/", false),
                Decision::Block(_)
            ),
            "userinfo@link-local must be blocked under safe"
        );
    }

    // ========================================================================
    // I1 — alternative IPv4 encodings
    // ========================================================================

    #[test]
    fn parse_ipv4_decimal_integer() {
        // 2130706433 == 0x7F000001 == 127.0.0.1
        assert_eq!(parse_ipv4("2130706433"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn parse_ipv4_hex_integer() {
        assert_eq!(parse_ipv4("0x7f000001"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn parse_ipv4_hex_integer_uppercase() {
        assert_eq!(parse_ipv4("0X7F000001"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn parse_ipv4_octal_dotted_first_octet() {
        // 0177 (octal) == 127
        assert_eq!(parse_ipv4("0177.0.0.1"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn parse_ipv4_hex_dotted() {
        assert_eq!(parse_ipv4("0x7f.0.0.1"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn safe_blocks_decimal_integer_loopback() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(p.decide("http://2130706433/", false), Decision::Block(_)),
            "decimal-integer loopback must be blocked under safe"
        );
    }

    #[test]
    fn safe_blocks_hex_integer_loopback() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(p.decide("http://0x7f000001/", false), Decision::Block(_)),
            "hex-integer loopback must be blocked under safe"
        );
    }

    #[test]
    fn safe_blocks_octal_dotted_loopback() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(p.decide("http://0177.0.0.1/", false), Decision::Block(_)),
            "octal-dotted loopback must be blocked under safe"
        );
    }

    #[test]
    fn safe_blocks_decimal_integer_imds() {
        // 169.254.169.254 == 0xA9FEA9FE == 2852039166
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(p.decide("http://2852039166/", false), Decision::Block(_)),
            "decimal-integer IMDS must be blocked"
        );
    }

    #[test]
    fn public_decimal_integer_allowed() {
        // 8.8.8.8 == 134744072 — public, must be allowed under default policy.
        let p = ResourcePolicy::default();
        assert_eq!(p.decide("http://134744072/", false), Decision::Allow);
    }

    // ========================================================================
    // I2 — IPv4-mapped IPv6
    // ========================================================================

    #[test]
    fn ipv4_mapped_ipv6_loopback_mixed_notation() {
        assert!(is_private_ip("::ffff:127.0.0.1"));
    }

    #[test]
    fn ipv4_mapped_ipv6_loopback_pure_hex() {
        // ::ffff:7f00:1 == ::ffff:127.0.0.1
        assert!(is_private_ip("::ffff:7f00:1"));
    }

    #[test]
    fn ipv4_mapped_ipv6_imds_mixed_notation() {
        assert!(is_private_ip("::ffff:169.254.169.254"));
    }

    #[test]
    fn ipv4_mapped_ipv6_imds_bracketed() {
        assert!(is_private_ip("[::ffff:169.254.169.254]"));
    }

    #[test]
    fn safe_blocks_ipv4_mapped_loopback() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(
                p.decide("http://[::ffff:127.0.0.1]/", false),
                Decision::Block(_)
            ),
            "IPv4-mapped loopback must be blocked under safe"
        );
    }

    #[test]
    fn safe_blocks_ipv4_mapped_imds() {
        let p = ResourcePolicy::safe_profile();
        assert!(
            matches!(
                p.decide("http://[::ffff:169.254.169.254]/", false),
                Decision::Block(_)
            ),
            "IPv4-mapped IMDS must be blocked under safe"
        );
    }

    #[test]
    fn ipv4_mapped_public_not_private() {
        // ::ffff:8.8.8.8 — public
        assert!(!is_private_ip("::ffff:8.8.8.8"));
    }
}
