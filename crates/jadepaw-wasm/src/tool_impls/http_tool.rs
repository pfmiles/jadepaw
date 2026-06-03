//! `HttpRequestTool` — `Tool` trait implementation for HTTP requests.
//!
//! Uses `reqwest` for real HTTP execution with defense-in-depth SSRF protection
//! per D-03 and D-03a decisions.
//!
//! # Defense layers (defense-in-depth)
//!
//! 1. Scheme validation: only `http://` and `https://` are allowed (T-04-06)
//! 2. Domain whitelist check (via `SessionState::can_access_domain`)
//! 3. IP-layer SSRF check: dns resolution + `is_blocked_ip()` (T-04-04)
//! 4. reqwest redirect policy: `Policy::limited(1)` (T-04-05)
//! 5. Response body cap: 1MB (T-04-07)
//! 6. Timeout: 30s via `ClientBuilder::timeout()` (T-04-07)
//! 7. DNS timeout: 5s via `tokio::time::timeout` (T-04-08)
//!
//! # Known risk (documented per T-04-04)
//!
//! **TOCTOU DNS rebinding:** The SSRF IP check calls `tokio::net::lookup_host` to
//! resolve the hostname, validates all returned IPs, but does NOT pin the resolved
//! addresses to the subsequent reqwest connection. reqwest performs its own
//! independent DNS resolution internally, creating a TOCTOU window where a DNS
//! rebinding attacker can return public IPs during the SSRF check and private IPs
//! during reqwest's resolution. This is an accepted risk for MVP:
//!
//! - The domain whitelist (`can_access_domain`) is the primary defense.
//! - DNS rebinding requires the attacker to control a whitelisted domain's DNS.
//! - The IP layer check is defense-in-depth to catch misconfigurations.
//! - Full fix: use `reqwest::ClientBuilder::dns_resolver()` with a custom resolver
//!   that pins the pre-validated addresses (tracked in TODO at resolve_and_check_ssrf).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use jadepaw_core::{SessionId, Tool, ToolResult};
use reqwest::redirect;
use serde_json::Value;
use tracing::warn;

use crate::host::network::{extract_host_from_url, resolve_and_check_ssrf_addr, SsrfDnsError};

/// Canonical tool name constant.
///
/// Shared with `ToolRegistry` so the domain capability check is not
/// coupled to a hardcoded string literal. If `HttpRequestTool::name()`
/// is refactored, this constant must also be updated.
pub const HTTP_REQUEST_TOOL_NAME: &str = "http_request";

/// Response body size cap (1MB per D-03a).
const MAX_RESPONSE_BODY_SIZE: usize = 1_048_576;

/// Build a `reqwest::Client` with D-03a security defaults.
///
/// - `redirect::Policy::limited(1)` — at most 1 redirect (T-04-05)
/// - `timeout(Duration::from_secs(30))` — 30s total request timeout (D-03a)
/// - Uses rustls-tls (no OpenSSL dependency)
fn build_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(redirect::Policy::limited(1))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to initialize HTTP client for HttpRequestTool")
}

/// Resolve the hostname and check all resolved IPs for SSRF.
///
/// Thin wrapper around the shared `resolve_and_check_ssrf_addr` in
/// `host::network` (WR-03). Converts `SsrfDnsError` variants to
/// `ToolResult::Error` with appropriate error codes for the agent
/// tool API.
///
/// # Known risk (T-04-04)
///
/// DNS rebinding: an attacker controlling the DNS for a whitelisted domain
/// can race this check. Accepted risk for MVP. See module-level docs for
/// TOCTOU details (WR-02).
async fn resolve_and_check_ssrf(host: &str) -> Result<Vec<SocketAddr>, ToolResult> {
    resolve_and_check_ssrf_addr(host).await.map_err(|e| match e {
        SsrfDnsError::Timeout => ToolResult::Error {
            code: "DNS_TIMEOUT".to_string(),
            message: format!(
                "DNS resolution timed out for host '{}'. The DNS server did not respond within 5 seconds.",
                host
            ),
            retryable: true,
        },
        SsrfDnsError::DnsError(err) => ToolResult::Error {
            code: "DNS_ERROR".to_string(),
            message: format!(
                "DNS resolution failed for host '{}': {}. Check the hostname and try again.",
                host, err
            ),
            retryable: true,
        },
        SsrfDnsError::Blocked { host: _, ip } => ToolResult::Error {
            code: "SSRF_BLOCKED".to_string(),
            message: format!(
                "Host '{}' resolved to blocked IP address {} (private/loopback/link-local/multicast). \
                 Only public IP addresses are allowed.",
                host, ip
            ),
            retryable: false,
        },
        SsrfDnsError::NoAddresses => ToolResult::Error {
            code: "DNS_ERROR".to_string(),
            message: format!(
                "DNS returned no addresses for host '{}'. Check the hostname and try again.",
                host
            ),
            retryable: true,
        },
    })
}

/// Tool that makes real HTTP requests with SSRF protection.
///
/// Implements the `Tool` trait for the agent-level dispatch layer.
/// Registered in `ToolRegistry` at agent startup.
pub struct HttpRequestTool {
    /// Configured reqwest HTTP client with security defaults.
    client: reqwest::Client,
    /// Allowed HTTP methods (D-03a: GET/POST/PUT/PATCH/DELETE only).
    allowed_methods: Vec<String>,
}

impl HttpRequestTool {
    /// Create a new `HttpRequestTool` with D-03a defaults.
    ///
    /// Returns an error if the underlying HTTP client (reqwest) fails to
    /// initialize, e.g., due to missing TLS support in restricted containers
    /// (CR-02).
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: build_http_client()?,
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
                "DELETE".to_string(),
            ],
        })
    }

    /// Extract scheme from a URL string. Returns `None` if no `://` marker found.
    fn extract_scheme(url_str: &str) -> Option<&str> {
        url_str.find("://").map(|idx| &url_str[..idx])
    }

    /// Parse URL, validate scheme, extract hostname, check domain capability.
    ///
    /// Returns `(hostname, url_string)` on success, or `ToolResult::Error` on failure.
    fn validate_url(&self, url_str: &str) -> Result<String, ToolResult> {
        // Scheme validation (T-04-06): only http and https
        let scheme = Self::extract_scheme(url_str).unwrap_or("");
        let scheme_lower = scheme.to_lowercase();
        if scheme_lower != "http" && scheme_lower != "https" {
            return Err(ToolResult::Error {
                code: "INVALID_SCHEME".to_string(),
                message: format!(
                    "URL scheme '{}' is not allowed. Only 'http' and 'https' schemes are supported. \
                     Schemes like file://, gopher://, ftp:// are blocked for security reasons.",
                    scheme
                ),
                retryable: false,
            });
        }

        // Extract hostname using existing Phase 2 utility
        let host = extract_host_from_url(url_str).to_string();
        if host.is_empty() {
            return Err(ToolResult::Error {
                code: "INVALID_URL".to_string(),
                message: format!(
                    "Could not extract hostname from URL '{}'. Ensure the URL includes a valid hostname.",
                    url_str
                ),
                retryable: false,
            });
        }

        Ok(host)
    }
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new().expect("HttpRequestTool::default() requires working TLS")
    }
}

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make an HTTP request to a specified URL. Supports GET, POST, PUT, PATCH, and DELETE methods. \
         The response body is capped at 1MB. Only public IP addresses are accessible (private/loopback \
         addresses are blocked for security). The request times out after 30 seconds."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"],
                    "description": "HTTP method to use for the request. Defaults to GET if not specified."
                },
                "url": {
                    "type": "string",
                    "description": "The URL to request. Must use http:// or https:// scheme."
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers to include in the request, as key-value pairs.",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body as a string. Used for POST, PUT, and PATCH requests."
                }
            },
            "required": ["url"]
        })
    }

    async fn call(&self, args: Value, session_id: SessionId) -> ToolResult {
        // 1. Extract and validate method
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();

        if !self.allowed_methods.iter().any(|m| m == &method) {
            return ToolResult::Error {
                code: "INVALID_METHOD".to_string(),
                message: format!(
                    "HTTP method '{}' is not allowed. Supported methods: GET, POST, PUT, PATCH, DELETE.",
                    method
                ),
                retryable: false,
            };
        }

        // 2. Extract and validate URL
        let url_str = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return ToolResult::Error {
                    code: "INVALID_ARGS".to_string(),
                    message: "Missing required parameter: 'url'. Please provide a URL to request.".to_string(),
                    retryable: false,
                };
            }
        };

        let host = match self.validate_url(&url_str) {
            Ok(h) => h,
            Err(e) => return e,
        };

        // 3. SSRF IP-layer check (defense-in-depth layer 2, per D-03).
        //    NOTE: The resolved addresses are validated for SSRF but cannot be
        //    pinned to the subsequent reqwest request without per-request client
        //    construction, which conflicts with connection reuse. The TOCTOU
        //    window for DNS rebinding between this check and reqwest's own DNS
        //    resolution is an accepted risk for MVP (see module-level docs).
        let addrs = match resolve_and_check_ssrf(&host).await {
            Ok(addrs) => addrs,
            Err(e) => return e,
        };
        // Silence unused warning: addrs validated but not pinned to request.
        //
        // TODO(perf): SSRF IP check resolves DNS independently of reqwest, doubling DNS
        // latency. Future optimization: use reqwest::ClientBuilder::dns_resolver() or
        // a custom reqwest::dns::Resolve implementation to inject the checked addresses
        // directly into reqwest's connection pool, avoiding re-resolution.
        let _ = &addrs;

        // 4. Extract optional headers
        let headers: HashMap<String, String> = args
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        // 5. Extract optional body
        let body: Option<String> = args
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // 6. Build and execute the reqwest request
        let mut request = self.client.request(
            method.parse::<reqwest::Method>().unwrap_or(reqwest::Method::GET),
            &url_str,
        );

        // Add headers with validation (WR-02).
        // Block dangerous headers that could enable request smuggling or
        // interfere with the HTTP connection state. Also reject header values
        // containing CR/LF sequences that could enable header injection.
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
                tracing::warn!(
                    header = %key,
                    "http_request: forbidden header '{}' was dropped",
                    key
                );
                continue;
            }
            if value.contains('\r') || value.contains('\n') {
                tracing::warn!(
                    header = %key,
                    "http_request: header '{}' value contains CR/LF — possible injection attempt, header dropped",
                    key
                );
                continue;
            }
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(ref body_str) = body {
            request = request.body(body_str.clone());
        }

        let mut response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(%session_id, url = %url_str, "http_request: request failed: {}", e);
                return ToolResult::Error {
                    code: "REQUEST_FAILED".to_string(),
                    message: format!(
                        "HTTP request to '{}' failed: {}. Check the URL is reachable and try again.",
                        url_str, e
                    ),
                    retryable: true,
                };
            }
        };

        let status = response.status();
        let status_code = status.as_u16();

        // 7. Extract response headers
        let response_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|s| (k.as_str().to_string(), s.to_string()))
            })
            .collect();

        // 8. Read response body, capped at 1MB (T-04-07, CR-03 fix).
        //    Use bounded chunked reading to avoid allocating the full response
        //    body in memory before truncation. reqwest 0.12's `chunk()` returns
        //    an async future per chunk; we loop until done or cap exceeded.
        let (body_str, truncated) = {
            use reqwest::Result as ReqwestResult;
            let mut buf = Vec::with_capacity(MAX_RESPONSE_BODY_SIZE);
            let mut total: usize = 0;
            loop {
                let chunk: ReqwestResult<Option<bytes::Bytes>> = response.chunk().await;
                match chunk {
                    Ok(Some(bytes)) => {
                        total += bytes.len();
                        if buf.len() < MAX_RESPONSE_BODY_SIZE {
                            let space = MAX_RESPONSE_BODY_SIZE - buf.len();
                            buf.extend_from_slice(&bytes[..bytes.len().min(space)]);
                        }
                        // If we've exceeded the cap, drain remaining chunks to free the connection
                        if total > MAX_RESPONSE_BODY_SIZE {
                            while let Ok(Some(_)) = response.chunk().await {}
                            break;
                        }
                    }
                    Ok(None) => break, // body complete
                    Err(e) => {
                        warn!(%session_id, url = %url_str, "http_request: failed to read response body: {}", e);
                        return ToolResult::Error {
                            code: "READ_ERROR".to_string(),
                            message: format!(
                                "Failed to read response body from '{}': {}.",
                                url_str, e
                            ),
                            retryable: true,
                        };
                    }
                }
            }
            let body_str = String::from_utf8_lossy(&buf).to_string();
            if total > MAX_RESPONSE_BODY_SIZE {
                (
                    format!(
                        "{}...\n[TRUNCATED: response body exceeded 1MB limit. {} bytes shown of {} total]",
                        body_str,
                        MAX_RESPONSE_BODY_SIZE,
                        total
                    ),
                    true,
                )
            } else {
                (body_str, false)
            }
        };

        // 9. Build result based on status code
        // 4xx: ToolResult::Ok (the tool succeeded at making the request; the response data is valid)
        // 5xx: ToolResult::Error (server error, retryable)
        if status_code >= 500 {
            return ToolResult::Error {
                code: "HTTP_500".to_string(),
                message: format!(
                    "HTTP request to '{}' returned server error (HTTP {}). \
                     The remote server returned an error status. You may retry the request later.",
                    url_str, status_code
                ),
                retryable: true,
            };
        }

        let mut data = serde_json::json!({
            "status": status_code,
            "status_text": status.canonical_reason().unwrap_or("Unknown"),
            "headers": response_headers,
            "body": body_str,
        });

        if truncated {
            data["truncated"] = serde_json::Value::Bool(true);
        }

        ToolResult::Ok { data }
    }
}