//! `http_request` host function — network capability stub.
//!
//! Phase 2 implements a stub that always returns CapabilityDenied.
//! Full network capability enforcement is implemented in Phase 4.
//!
//! # Threat mitigations
//!
//! - T-02-10 (Elevation of Privilege): `can_access_domain()` checked before
//!   any outbound connection. Default deny (empty DomainPattern whitelist).

use std::future::Future;

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
/// # Phase 2 behavior
///
/// This function validates all inputs (bounds-checking, domain validation) and
/// returns -1 with a CapabilityDenied log entry. The actual HTTP request
/// logic will be implemented in Phase 4.
///
/// # Threat mitigations
///
/// - T-02-10 (Elevation of Privilege): domain capability check before any action
/// - T-02-09 (Info Disclosure): guest memory bounds-checked
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

        // Phase 2: stub — return CapabilityDenied
        // Phase 4 will implement actual HTTP request with:
        // - Method validation (GET/POST/PUT/PATCH/DELETE)
        // - SSRF prevention (block private/loopback IPs)
        // - Timeout via tokio::time::timeout
        // - Rate limiting per instance
        warn!(
            %session_id,
            "http_request: network capability not yet implemented in Phase 2 (URL: {}, domain: {})",
            url, domain
        );
        -1
    })
}

/// Extract the host portion from a URL string.
///
/// Handles both `http://example.com/path` and simple `example.com` forms.
/// Returns the domain without port or path.
fn extract_host_from_url(url: &str) -> &str {
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