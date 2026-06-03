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

use reqwest::redirect;
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
        let state = caller.data();
        let session_id = state.session_id;

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

        // Bounds-check remaining parameters for validation (T-02-09)
        let _all_valid = {
            let check = |ptr: i32, len: i32, name: &str| -> bool {
                let _start = ptr as usize;
                let len_usize = len as usize;
                let end = _start.saturating_add(len_usize);
                if end > mem_size {
                    warn!(%session_id, "http_request: {} pointer out of bounds", name);
                    false
                } else {
                    true
                }
            };
            check(method_ptr, method_len, "method")
                && check(headers_ptr, headers_len, "headers")
                && check(body_ptr, body_len, "body")
        };

        if !_all_valid {
            return -1;
        }

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
        let method_len_usize = method_len as usize;
        let method = match std::str::from_utf8(
            &mem_data[method_start..method_start.saturating_add(method_len_usize)],
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
        let headers_len_usize = headers_len as usize;
        let headers_raw = &mem_data[headers_start..headers_start.saturating_add(headers_len_usize)];
        let headers: HashMap<String, String> = if headers_raw.is_empty() {
            HashMap::new()
        } else {
            match std::str::from_utf8(headers_raw) {
                Ok(s) => serde_json::from_str(s).unwrap_or_default(),
                Err(_) => {
                    warn!(%session_id, "http_request: invalid UTF-8 in headers");
                    return -1;
                }
            }
        };

        // 4. Read body from guest memory (bounds-checked above)
        let body_start = body_ptr as usize;
        let body_len_usize = body_len as usize;
        let body: Option<Vec<u8>> = if body_len_usize == 0 {
            None
        } else {
            Some(mem_data[body_start..body_start.saturating_add(body_len_usize)].to_vec())
        };

        // 5. SSRF IP-layer check (defense-in-depth layer 2, T-04-04)
        //    DNS resolution wrapped in 5s timeout (T-04-08, Pitfall 4)
        let domain_for_dns = domain.to_string();
        let addrs_result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host(format!("{}:0", domain_for_dns)),
        )
        .await;

        let addrs = match addrs_result {
            Ok(Ok(iter)) => {
                let addrs: Vec<std::net::SocketAddr> = iter.collect();
                // Check all resolved IPs for SSRF (T-04-04)
                for addr in &addrs {
                    if is_blocked_ip(&addr.ip()) {
                        warn!(
                            %session_id,
                            "http_request: SSRF blocked — host '{}' resolved to {}",
                            domain, addr.ip()
                        );
                        return -1;
                    }
                }
                addrs
            }
            Ok(Err(e)) => {
                warn!(%session_id, "http_request: DNS error for '{}': {}", domain, e);
                return -1;
            }
            Err(_timeout) => {
                warn!(%session_id, "http_request: DNS timeout for '{}'", domain);
                return -1;
            }
        };

        // If DNS returned no addresses, fail
        if addrs.is_empty() {
            warn!(%session_id, "http_request: DNS returned no addresses for '{}'", domain);
            return -1;
        }

        // 6. Build and execute the reqwest request (T-04-05, T-04-07)
        let client = match reqwest::Client::builder()
            .redirect(redirect::Policy::limited(1))
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!(%session_id, "http_request: failed to build reqwest client: {}", e);
                return -1;
            }
        };

        let reqwest_method = match method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            _ => unreachable!("method already validated against ALLOWED_METHODS"),
        };

        let mut request = client.request(reqwest_method, url);
        for (key, value) in &headers {
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

/// Extract the host portion from a URL string.
///
/// Handles both `http://example.com/path` and simple `example.com` forms.
/// Returns the domain without port or path.
pub(crate) fn extract_host_from_url(url: &str) -> &str {
    // Strip scheme
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Strip path, query, fragment
    let host_and_port = if let Some(idx) = after_scheme.find('/') {
        &after_scheme[..idx]
    } else if let Some(idx) = after_scheme.find('?') {
        &after_scheme[..idx]
    } else if let Some(idx) = after_scheme.find('#') {
        &after_scheme[..idx]
    } else {
        after_scheme
    };

    // Strip port
    if let Some(idx) = host_and_port.find(':') {
        &host_and_port[..idx]
    } else {
        host_and_port
    }
}

/// Check if an IP address is blocked for outbound SSRF.
///
/// Returns `true` if the IP is in a private, loopback, link-local,
/// multicast, broadcast, or unspecified range per D-03.
///
/// All methods used are stable on Rust 1.85+:
/// - `is_unique_local` and `is_unicast_link_local` stabilized in 1.84.0
pub(crate) fn is_blocked_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            v4.is_private()       // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_loopback()    // 127.0.0.0/8
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_multicast()   // 224.0.0.0/4
                || v4.is_broadcast()   // 255.255.255.255
                || v4.is_unspecified() // 0.0.0.0
        }
        IpAddr::V6(v6) => {
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
}