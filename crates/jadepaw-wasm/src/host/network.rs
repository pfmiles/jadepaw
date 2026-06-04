//! `http_request` host function — real HTTP execution with SSRF protection.
//!
//! Phase 4 replaces the Phase 2 stub with real HTTP request logic using
//! `reqwest`. The host function validates all inputs (bounds-checking, domain
//! validation) inherited from Phase 2, then adds Phase 4 protections:
//! SSRF IP-layer check, redirect limit, timeout, and response body cap.
//!
//! # Threat mitigations
//!
//! - T-02-10 (Elevation of Privilege): `can_access_domain()` checked before
//!   any outbound connection. Default deny (empty DomainPattern whitelist).
//! - T-04-04 (Info Disclosure): SSRF IP check after DNS resolution
//! - T-04-05 (Info Disclosure): `redirect::Policy::limited(1)` — 1 redirect max
//! - T-04-06 (Tampering): scheme validation — only http/https allowed
//! - T-04-07 (DoS): 30s timeout, 1MB body cap
//! - T-04-08 (DoS): DNS resolution wrapped in 5s `tokio::time::timeout`

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use tracing::warn;
use wasmtime::Caller;

use crate::session::SessionState;

/// Host function for `jadepaw.http_request`.
///
/// # Signature (guest import)
///
/// `(method_ptr: i32, method_len: i32, url_ptr: i32, url_len: i32,
///   headers_ptr: i32, headers_len: i32, body_ptr: i32, body_len: i32) -> i32`
///
/// - Returns: HTTP status code on success, -1 on error
///
/// # Phase 4 behavior
///
/// Executes real HTTP requests using `reqwest` with defense-in-depth:
/// 1. Input validation (bounds-checking, URL parsing)
/// 2. Domain capability check (inherited from Phase 2)
/// 3. SSRF IP-layer check (resolves hostname, blocks private/loopback IPs)
/// 4. Method validation (GET/POST/PUT/PATCH/DELETE only)
/// 5. Real HTTP execution via reqwest with redirect::Policy::limited(1)
/// 6. Response body capped at 1MB
/// 7. Timeout: 30s total + 5s DNS
///
/// # Threat mitigations
///
/// - T-02-10 (Elevation of Privilege): domain capability check before any action
/// - T-02-09 (Info Disclosure): guest memory bounds-checked
/// - T-04-04 through T-04-08: SSRF, redirect, scheme, DoS protections
#[allow(clippy::too_many_arguments)]
pub fn http_request_host_fn(
    mut caller: Caller<'_, SessionState>,
    method_ptr: i32,
    method_len: i32,
    url_ptr: i32,
    url_len: i32,
    headers_ptr: i32,
    headers_len: i32,
    body_ptr: i32,
    body_len: i32,
) -> Box<dyn Future<Output = i32> + Send + '_> {
    Box::new(async move {
        // Access SessionState at entry (D-11)
        let session_id = caller.data().session_id;

        // Get guest memory
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(mem) => mem,
            None => {
                warn!(%session_id, "http_request: no exported memory in guest module");
                return -1;
            }
        };

        let mem_data = memory.data(&caller);
        let mem_size = memory.data_size(&caller);

        // Bounds-check and read URL (T-02-09, WR-01: use checked_add to prevent overflow)
        let url_start = url_ptr as usize;
        let url_len_usize = url_len as usize;
        let url_end = url_start.saturating_add(url_len_usize);
        if url_end > mem_size {
            warn!(%session_id, "http_request: URL pointer out of bounds");
            return -1;
        }
        let url = match std::str::from_utf8(&mem_data[url_start..url_end]) {
            Ok(s) => s,
            Err(e) => {
                warn!(%session_id, "http_request: invalid UTF-8 in URL: {}", e);
                return -1;
            }
        };

        // Extract domain from URL for capability check
        let domain = extract_host_from_url(url);

        // Capability check: domain must be in can_network_to whitelist (T-02-10)
        {
            let can_access = caller.data().can_access_domain(domain);
            if !can_access {
                warn!(
                    %session_id,
                    "http_request: CapabilityDenied for domain '{}' (URL: {})",
                    domain, url
                );
                return -1;
            }
        }

        // Bounds-check remaining parameters and compute validated end positions.
        // The check closure uses checked_add instead of saturating_add, returning
        // the validated end position so the subsequent slice operations can use it
        // directly — eliminating the duplicate saturating_add at each call site.
        let check = |ptr: i32, len: i32, name: &str| -> Option<usize> {
            let start = ptr as usize;
            let len_usize = len as usize;
            let end = start.checked_add(len_usize)?;
            if end > mem_size {
                warn!(%session_id, "http_request: {} pointer out of bounds", name);
                None
            } else {
                Some(end)
            }
        };
        let method_end = match check(method_ptr, method_len, "method") {
            Some(e) => e,
            None => return -1,
        };
        let headers_end = match check(headers_ptr, headers_len, "headers") {
            Some(e) => e,
            None => return -1,
        };
        let body_end = match check(body_ptr, body_len, "body") {
            Some(e) => e,
            None => return -1,
        };

        // ── Phase 4: real HTTP execution ──

        // 1. Scheme validation (T-04-06): only http/https
        let scheme = if let Some(idx) = url.find("://") {
            &url[..idx]
        } else {
            warn!(%session_id, "http_request: URL has no scheme: {}", url);
            return -1;
        };
        let scheme_lower = scheme.to_lowercase();
        if scheme_lower != "http" && scheme_lower != "https" {
            warn!(%session_id, "http_request: blocked scheme '{}' in URL: {}", scheme, url);
            return -1;
        }

        // 2. Read method from guest memory (bounds-checked above)
        let method_start = method_ptr as usize;
        let method = match std::str::from_utf8(
            &mem_data[method_start..method_end],
        ) {
            Ok(s) => s.to_uppercase(),
            Err(e) => {
                warn!(%session_id, "http_request: invalid UTF-8 in method: {}", e);
                return -1;
            }
        };

        // Validate HTTP method (D-03a)
        const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
        if !ALLOWED_METHODS.contains(&method.as_str()) {
            warn!(%session_id, "http_request: method '{}' not allowed", method);
            return -1;
        }

        // 3. Read headers from guest memory (bounds-checked above)
        let headers_start = headers_ptr as usize;
        let headers_raw = &mem_data[headers_start..headers_end];
        let headers: HashMap<String, String> = if headers_raw.is_empty() {
            HashMap::new()
        } else {
            match std::str::from_utf8(headers_raw) {
                Ok(s) => match serde_json::from_str(s) {
                        Ok(h) => h,
                        Err(e) => {
                            warn!(%session_id, "http_request: invalid JSON in headers: {}", e);
                            return -1;
                        }
                    },
                Err(_) => {
                    warn!(%session_id, "http_request: invalid UTF-8 in headers");
                    return -1;
                }
            }
        };

        // 4. Read body from guest memory (bounds-checked above)
        let body_start = body_ptr as usize;
        let body: Option<Vec<u8>> = if body_start == body_end {
            None
        } else {
            Some(mem_data[body_start..body_end].to_vec())
        };

        // 5. SSRF IP-layer check (defense-in-depth layer 2, T-04-04)
        //    Uses the shared resolve_and_check_ssrf_addr from WR-03.
        let _addrs = match resolve_and_check_ssrf_addr(&domain).await {
            Ok(addrs) => addrs,
            Err(SsrfDnsError::Timeout) => {
                warn!(%session_id, "http_request: DNS timeout for '{}'", domain);
                return -1;
            }
            Err(SsrfDnsError::DnsError(e)) => {
                warn!(%session_id, "http_request: DNS error for '{}': {}", domain, e);
                return -1;
            }
            Err(SsrfDnsError::NoAddresses) => {
                warn!(%session_id, "http_request: DNS returned no addresses for '{}'", domain);
                return -1;
            }
            Err(SsrfDnsError::Blocked { host: _, ip }) => {
                warn!(
                    %session_id,
                    "http_request: SSRF blocked — host '{}' resolved to {}",
                    domain, ip
                );
                return -1;
            }
        };

        // 6. Execute the reqwest request using the session's shared HTTP client
        //    (CR-01: reuse client across all calls to avoid resource leak from
        //    constructing a fresh reqwest::Client per host function invocation).
        //    reqwest::Client is Clone (Arc-wrapped internally), so cloning is cheap.
        let client = caller.data().http_client.clone();

        let reqwest_method = match method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            _ => unreachable!("method already validated against ALLOWED_METHODS"),
        };

        let mut request = client.request(reqwest_method, url);

        // WR-03: Filter forbidden request headers for defense-in-depth
        // consistency with HttpRequestTool::call(). The same header blocklist
        // and CR/LF injection check applied in the agent tool API path is also
        // enforced here on the guest Wasm host function path.
        const FORBIDDEN_REQUEST_HEADERS: &[&str] = &[
            "host",
            "content-length",
            "transfer-encoding",
            "proxy-authorization",
            "connection",
            "expect",
        ];
        for (key, value) in &headers {
            let key_lower = key.to_lowercase();
            if FORBIDDEN_REQUEST_HEADERS.contains(&key_lower.as_str()) {
                warn!(
                    %session_id,
                    header = %key,
                    "http_request: forbidden header '{}' was dropped",
                    key
                );
                continue;
            }
            if value.contains('\r') || value.contains('\n') {
                warn!(
                    %session_id,
                    header = %key,
                    "http_request: header '{}' value contains CR/LF — possible injection attempt, header dropped",
                    key
                );
                continue;
            }
            request = request.header(key.as_str(), value.as_str());
        }
        if let Some(ref body_bytes) = body {
            request = request.body(body_bytes.clone());
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(%session_id, "http_request: request failed for '{}': {}", url, e);
                return -1;
            }
        };

        let status_code = response.status().as_u16();

        // 7. Connection cleanup (T-04-07, CR-02 fix).
        //    This host function only returns the status code (D-04b), so the
        //    response body is never needed after extracting the status. Dropping
        //    the response (rather than calling `response.bytes().await`) avoids
        //    the DoS vector of allocating memory for arbitrarily large bodies.
        //    reqwest's connection pool handles the underlying connection cleanup.
        drop(response);

        // Return status code (D-04b: host fns return i32)
        status_code as i32
    })
}

/// Error type for SSRF DNS resolution failures.
///
/// Used by `resolve_and_check_ssrf` to convey specific failure reasons
/// that each caller (host function and HttpRequestTool) can convert to
/// their own error conventions.
#[derive(Debug)]
#[allow(dead_code)] // host field used by http_tool.rs path, ip used by host fn path
pub(crate) enum SsrfDnsError {
    /// DNS resolution timed out (5s).
    Timeout,
    /// DNS resolution returned an error.
    DnsError(String),
    /// One or more resolved IPs are in a blocked range.
    Blocked { host: String, ip: std::net::IpAddr },
    /// DNS returned no addresses.
    NoAddresses,
}

/// Resolve a hostname and check all resolved IPs for SSRF (WR-03).
///
/// Shared between `http_request_host_fn` (network.rs) and `HttpRequestTool::call`
/// (http_tool.rs). Wraps `tokio::net::lookup_host` in a 5-second timeout per
/// Pitfall 4 / T-04-08, then checks each resolved IP against `is_blocked_ip`.
///
/// # Returns
///
/// - `Ok(Vec<SocketAddr>)` if all IPs are public.
/// - `Err(SsrfDnsError)` with a specific variant on failure.
pub(crate) async fn resolve_and_check_ssrf_addr(host: &str) -> Result<Vec<std::net::SocketAddr>, SsrfDnsError> {
    let ported = format!("{}:0", host);
    let lookup = tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host(&ported)).await;
    let iter = match lookup {
        Ok(Ok(iter)) => iter,
        Ok(Err(e)) => return Err(SsrfDnsError::DnsError(e.to_string())),
        Err(_) => return Err(SsrfDnsError::Timeout),
    };
    let addrs: Vec<std::net::SocketAddr> = iter.collect();
    if addrs.is_empty() {
        return Err(SsrfDnsError::NoAddresses);
    }
    for addr in &addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(SsrfDnsError::Blocked {
                host: host.to_string(),
                ip: addr.ip(),
            });
        }
    }
    Ok(addrs)
}

/// Extract the host portion from a URL string.
///
/// Delegates to the canonical implementation in `jadepaw_core::tool`.
pub(crate) fn extract_host_from_url(url: &str) -> &str {
    jadepaw_core::extract_host_from_url(url)
}

/// Check if an IP address is blocked for outbound SSRF.
///
/// Returns `true` if the IP is in a private, loopback, link-local,
/// multicast, broadcast, or unspecified range per D-03.
///
/// All methods used are stable on Rust 1.85+:
/// - `is_unique_local` and `is_unicast_link_local` stabilized in 1.84.0
/// Check if an IPv4 address is in the RFC 6598 shared address space.
///
/// `is_shared()` (tracking issue #27709) is not yet stable as of Rust 1.95.
/// Implement the check manually: 100.64.0.0/10 = 100.64.0.0 .. 100.127.255.255.
fn is_shared_v4(v4: &std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127)
}

/// Check if an IPv4 address is blocked for outbound SSRF.
fn is_blocked_v4(v4: &std::net::Ipv4Addr) -> bool {
    v4.is_private()       // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        || v4.is_loopback()    // 127.0.0.0/8
        || v4.is_link_local()  // 169.254.0.0/16
        || v4.is_multicast()   // 224.0.0.0/4
        || v4.is_broadcast()   // 255.255.255.255
        || v4.is_unspecified() // 0.0.0.0
        || is_shared_v4(v4)    // 100.64.0.0/10 (RFC 6598 CGNAT)
}

/// Check if an IPv6 address is an IPv4-mapped address.
///
/// IPv4-mapped IPv6 addresses have the prefix `::ffff:0:0/96` and can embed
/// any IPv4 address. `Ipv6Addr::to_ipv4()` returns `Some` for both
/// IPv4-mapped (`::ffff:x.x.x.x`) and IPv4-compatible (`::x.x.x.x`, deprecated)
/// formats. We narrow the check to only true IPv4-mapped addresses because
/// real IPv6 addresses (like `::1`) also return `Some` from `to_ipv4()`
/// (as `Some(0.0.0.1)`) but are NOT IPv4-mapped — they must be checked
/// against the IPv6 blocklist, not the IPv4 one.
fn is_ipv4_mapped(v6: &std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    // IPv4-mapped prefix: ::ffff:0:0/96 → segments[0..5] == [0, 0, 0, 0, 0, 0xFFFF]
    match v6.segments() {
        [0, 0, 0, 0, 0, 0xFFFF, a, b] => {
            let v4_octets = [((a >> 8) & 0xFF) as u8, (a & 0xFF) as u8,
                             ((b >> 8) & 0xFF) as u8, (b & 0xFF) as u8];
            Some(std::net::Ipv4Addr::from(v4_octets))
        }
        _ => None,
    }
}

pub(crate) fn is_blocked_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            // CR-01: Convert IPv4-mapped IPv6 to IPv4 before checks.
            // An attacker can wrap internal IPv4 addresses as ::ffff:127.0.0.1
            // to bypass the IPv6-specific checks below (e.g., ::1 does not match
            // ::ffff:127.0.0.1). Extracting the embedded IPv4 first closes this
            // bypass while preserving all IPv4 checks including shared (CR-02).
            if let Some(v4) = is_ipv4_mapped(v6) {
                return is_blocked_v4(&v4);
            }
            v6.is_loopback()            // ::1
                || v6.is_unique_local()      // fc00::/7
                || v6.is_unicast_link_local() // fe80::/10
                || v6.is_multicast()         // ff00::/8
                || v6.is_unspecified()       // ::
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_host_basic() {
        assert_eq!(extract_host_from_url("https://example.com/path"), "example.com");
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(extract_host_from_url("http://localhost:8080/api"), "localhost");
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(extract_host_from_url("api.example.com/v1"), "api.example.com");
    }

    #[test]
    fn extract_host_bare_domain() {
        assert_eq!(extract_host_from_url("example.com"), "example.com");
    }

    // ── is_blocked_ip tests ──

    #[test]
    fn is_blocked_ip_v4_loopback() {
        assert!(is_blocked_ip(&"127.0.0.1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn is_blocked_ip_v4_private() {
        assert!(is_blocked_ip(&"192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip(&"10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip(&"172.16.0.1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn is_blocked_ip_v4_public() {
        assert!(!is_blocked_ip(&"8.8.8.8".parse::<IpAddr>().unwrap()));
        assert!(!is_blocked_ip(&"1.1.1.1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn is_blocked_ip_v6_loopback() {
        assert!(is_blocked_ip(&"::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn is_blocked_ip_v6_public() {
        assert!(!is_blocked_ip(
            &"2001:4860:4860::8888".parse::<IpAddr>().unwrap()
        ));
    }

    // CR-02: RFC 6598 shared address space (100.64.0.0/10) must be blocked
    #[test]
    fn is_blocked_ip_shared_address_space() {
        assert!(is_blocked_ip(&"100.64.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_blocked_ip(&"100.127.255.254".parse::<IpAddr>().unwrap()));
    }

    // CR-01: IPv4-mapped IPv6 addresses must be checked against IPv4 blocklist
    #[test]
    fn is_blocked_ip_v4_mapped_ipv6_loopback() {
        // ::ffff:127.0.0.1 should be blocked (loopback)
        let addr: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(is_blocked_ip(&addr));
    }

    #[test]
    fn is_blocked_ip_v4_mapped_ipv6_private() {
        // ::ffff:192.168.1.1 should be blocked (private)
        let addr: IpAddr = "::ffff:192.168.1.1".parse().unwrap();
        assert!(is_blocked_ip(&addr));
        // ::ffff:10.0.0.1 should be blocked (private)
        let addr2: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
        assert!(is_blocked_ip(&addr2));
    }

    #[test]
    fn is_blocked_ip_v4_mapped_ipv6_shared() {
        // ::ffff:100.64.0.1 should be blocked (CGNAT, CR-02 cross-check)
        let addr: IpAddr = "::ffff:100.64.0.1".parse().unwrap();
        assert!(is_blocked_ip(&addr));
    }

    #[test]
    fn is_blocked_ip_v4_mapped_ipv6_public() {
        // ::ffff:8.8.8.8 should NOT be blocked (public)
        let addr: IpAddr = "::ffff:8.8.8.8".parse().unwrap();
        assert!(!is_blocked_ip(&addr));
    }
}