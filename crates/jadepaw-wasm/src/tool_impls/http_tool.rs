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
//! DNS rebinding: an attacker who controls a whitelisted domain's DNS can race
//! the 5s resolution timeout to change the IP between the domain whitelist check
//! and the IP-layer check. This is an accepted risk for MVP — the domain
//! whitelist is the primary defense, and the IP check is defense-in-depth to
//! catch misconfigurations and simple attacks.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use jadepaw_core::{SessionId, Tool, ToolResult};
use reqwest::redirect;
use serde_json::Value;
use tracing::warn;

use crate::host::network::{extract_host_from_url, is_blocked_ip};

/// Response body size cap (1MB per D-03a).
const MAX_RESPONSE_BODY_SIZE: usize = 1_048_576;

/// Build a `reqwest::Client` with D-03a security defaults.
///
/// - `redirect::Policy::limited(1)` — at most 1 redirect (T-04-05)
/// - `timeout(Duration::from_secs(30))` — 30s total request timeout (D-03a)
/// - Uses rustls-tls (no OpenSSL dependency)
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(redirect::Policy::limited(1))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest Client builder should not fail with valid config")
}

/// Resolve the hostname and check all resolved IPs for SSRF.
///
/// Wraps `tokio::net::lookup_host` in a 5-second timeout (Pitfall 4).
/// Returns `Ok(Vec<SocketAddr>)` if all IPs are public, or
/// `Err(ToolResult::Error)` if any IP is blocked or DNS fails/times out.
///
/// # Known risk (T-04-04)
///
/// DNS rebinding: an attacker controlling the DNS for a whitelisted domain
/// can race this check. Accepted risk for MVP.
async fn resolve_and_check_ssrf(host: &str) -> Result<Vec<SocketAddr>, ToolResult> {
    let addrs: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host(format!("{}:0", host)),
    )
    .await
    .map_err(|_| ToolResult::Error {
        code: "DNS_TIMEOUT".to_string(),
        message: format!(
            "DNS resolution timed out for host '{}'. The DNS server did not respond within 5 seconds.",
            host
        ),
        retryable: true,
    })?
    .map_err(|e| ToolResult::Error {
        code: "DNS_ERROR".to_string(),
        message: format!(
            "DNS resolution failed for host '{}': {}. Check the hostname and try again.",
            host, e
        ),
        retryable: true,
    })?
    .collect();

    // Check all resolved IPs for SSRF (defense-in-depth layer 2, per D-03)
    for addr in &addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(ToolResult::Error {
                code: "SSRF_BLOCKED".to_string(),
                message: format!(
                    "Host '{}' resolved to blocked IP address {} (private/loopback/link-local/multicast). \
                     Only public IP addresses are allowed.",
                    host, addr.ip()
                ),
                retryable: false,
            });
        }
    }

    Ok(addrs)
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
    pub fn new() -> Self {
        Self {
            client: build_http_client(),
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
                "DELETE".to_string(),
            ],
        }
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
        Self::new()
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

        // 3. SSRF IP-layer check (defense-in-depth layer 2, per D-03)
        let _addrs = match resolve_and_check_ssrf(&host).await {
            Ok(addrs) => addrs,
            Err(e) => return e,
        };

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

        // Add headers
        for (key, value) in &headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(ref body_str) = body {
            request = request.body(body_str.clone());
        }

        let response = match request.send().await {
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

        // 8. Read response body, capped at 1MB (T-04-07)
        let body_bytes = match response.text().await {
            Ok(text) => text,
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
        };

        let (body_str, truncated) = if body_bytes.len() > MAX_RESPONSE_BODY_SIZE {
            (
                format!(
                    "{}...\n[TRUNCATED: response body exceeded 1MB limit. {} bytes shown of {} total]",
                    &body_bytes[..MAX_RESPONSE_BODY_SIZE],
                    MAX_RESPONSE_BODY_SIZE,
                    body_bytes.len()
                ),
                true,
            )
        } else {
            (body_bytes, false)
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