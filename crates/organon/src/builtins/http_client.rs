//! Generic HTTP client tool (`http_request`) supporting POST/PUT/DELETE/PATCH
//! in addition to GET, with configurable headers and body.
//!
//! WHY: `web_fetch` handles only GET and strips HTML. Agents that need to call
//! REST APIs (POST JSON, PUT YAML, DELETE) had no path. This adds a generic
//! method-aware HTTP tool that reuses the SSRF guards from `research.rs`.
//! Closes #3441.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;

use indexmap::IndexMap;
use reqwest::{Method, redirect};

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::workspace::{extract_opt_str, extract_opt_u64, extract_str};

/// Cloud-metadata / loopback hostnames rejected outright.
///
/// WHY: duplicated with `research.rs` rather than shared, because each tool's
/// allowlist is an independent security boundary; a future widening of one
/// should not silently widen the other. Kept in sync by convention.
const BLOCKED_HOSTNAMES: &[&str] = &["localhost", "metadata.google.internal"];

/// Headers that must never be forwarded from LLM input.
///
/// WHY: Host/Content-Length are computed by reqwest; the LLM setting them
/// produces malformed requests. Authorization is operator-sensitive; agents
/// should never inject credentials without an explicit surface (future work).
const FORBIDDEN_REQUEST_HEADERS: &[&str] = &["host", "content-length"];

/// Upper bound on response body size forwarded to the LLM.
const MAX_RESPONSE_BYTES: usize = 1_000_000;

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.octets().first() == Some(&0)
                || *v4 == Ipv4Addr::new(169, 254, 169, 254)
                || *v4 == Ipv4Addr::new(169, 254, 169, 123)
        }
        IpAddr::V6(v6) => {
            *v6 == Ipv6Addr::LOCALHOST
                || (v6.segments().first().unwrap_or(&0) & 0xfe00) == 0xfc00
                || (v6.segments().first().unwrap_or(&0) & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.octets().first() == Some(&0)
                })
        }
    }
}

async fn validate_url_not_internal(url_str: &str) -> std::result::Result<(), String> {
    let parsed: reqwest::Url = url_str.parse().map_err(|e| format!("invalid URL: {e}"))?;

    let host = parsed.host_str().ok_or("URL has no host")?;
    let host_lower = host.to_lowercase();
    for blocked in BLOCKED_HOSTNAMES {
        if host_lower == *blocked {
            return Err(format!("blocked hostname: {host}"));
        }
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| format!("DNS resolution failed for {host}: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }

    for addr in &addrs {
        if is_private_ip(&addr.ip()) {
            return Err("URL resolves to a private/internal IP address".to_owned());
        }
    }

    Ok(())
}

fn parse_method(raw: &str) -> std::result::Result<Method, String> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "DELETE" => Ok(Method::DELETE),
        "PATCH" => Ok(Method::PATCH),
        "HEAD" => Ok(Method::HEAD),
        other => Err(format!("unsupported method: {other}")),
    }
}

fn extract_headers(
    args: &serde_json::Value,
) -> std::result::Result<HashMap<String, String>, String> {
    let Some(raw) = args.get("headers") else {
        return Ok(HashMap::new());
    };
    let Some(obj) = raw.as_object() else {
        return Err("headers must be a JSON object of string->string".to_owned());
    };
    let mut out = HashMap::new();
    for (k, v) in obj {
        let value = v
            .as_str()
            .ok_or_else(|| format!("header {k} must be a string value"))?;
        let lower = k.to_ascii_lowercase();
        if FORBIDDEN_REQUEST_HEADERS.contains(&lower.as_str()) {
            return Err(format!("header not permitted: {k}"));
        }
        out.insert(k.clone(), value.to_owned());
    }
    Ok(out)
}

struct HttpRequestExecutor;

impl ToolExecutor for HttpRequestExecutor {
    #[expect(
        clippy::too_many_lines,
        reason = "single HTTP lifecycle — validate URL, build client, send, render response; splitting would fragment one cohesive I/O operation"
    )]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let _ = ctx; // unused: no service dependencies

            let url = extract_str(&input.arguments, "url", &input.name)?;
            let method_str = extract_opt_str(&input.arguments, "method").unwrap_or("GET");
            let body = extract_opt_str(&input.arguments, "body");
            let timeout_secs = extract_opt_u64(&input.arguments, "timeoutSecs").unwrap_or(30);

            // SAFE: validating user-supplied URL scheme, not constructing an endpoint
            if !url.starts_with("http://") && !url.starts_with("https://") {
                // kanon:ignore SECURITY/insecure-transport -- scheme validation, not construction
                return Ok(ToolResult::error("URL must start with http:// or https://"));
            }

            let method = match parse_method(method_str) {
                Ok(m) => m,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            let headers = match extract_headers(&input.arguments) {
                Ok(h) => h,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            if let Err(msg) = validate_url_not_internal(url).await {
                return Ok(ToolResult::error(msg));
            }

            let client = match reqwest::Client::builder()
                .redirect(redirect::Policy::custom(|attempt| {
                    let url = attempt.url();
                    let host = match url.host_str() {
                        Some(h) => h.to_lowercase(),
                        None => return attempt.stop(),
                    };
                    for blocked in BLOCKED_HOSTNAMES {
                        if host == *blocked {
                            return attempt.stop();
                        }
                    }
                    if let Some(addr) = url.host_str().and_then(|h| h.parse::<IpAddr>().ok())
                        && is_private_ip(&addr)
                    {
                        return attempt.stop();
                    }
                    if attempt.previous().len() >= 5 {
                        return attempt.stop();
                    }
                    attempt.follow()
                }))
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
            {
                Ok(c) => c,
                Err(e) => return Ok(ToolResult::error(format!("client build failed: {e}"))),
            };

            let mut req = client.request(method.clone(), url).header(
                "User-Agent",
                concat!(
                    "aletheia/",
                    env!("CARGO_PKG_VERSION"),
                    " (organon http_request)"
                ),
            );
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            if let Some(b) = body {
                req = req.body(b.to_owned());
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("request failed: {e}"))),
            };

            let status = response.status();
            let status_code = status.as_u16();

            let mut header_summary: Vec<(String, String)> = response
                .headers()
                .iter()
                .filter_map(|(k, v)| {
                    v.to_str()
                        .ok()
                        .map(|s| (k.as_str().to_owned(), s.to_owned()))
                })
                .collect();
            header_summary.sort_by(|a, b| a.0.cmp(&b.0));

            let body_bytes = match response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("failed to read body: {e}")));
                }
            };

            let (body_text, truncated) = if body_bytes.len() > MAX_RESPONSE_BYTES {
                // WHY: truncate at a valid UTF-8 boundary in the decoded
                // string so the rendered text stays valid UTF-8 even when
                // the response body is arbitrary binary.
                let decoded = String::from_utf8_lossy(&body_bytes).into_owned();
                let mut end = MAX_RESPONSE_BYTES.min(decoded.len());
                while end > 0 && !decoded.is_char_boundary(end) {
                    end -= 1;
                }
                let partial = decoded.get(..end).unwrap_or("").to_owned();
                (partial, true)
            } else {
                (String::from_utf8_lossy(&body_bytes).into_owned(), false)
            };

            let mut rendered = format!("status: {status_code}\n");
            rendered.push_str("headers:\n");
            for (k, v) in &header_summary {
                let _ = writeln!(rendered, "  {k}: {v}");
            }
            rendered.push_str("body:\n");
            rendered.push_str(&body_text);
            if truncated {
                rendered.push_str("\n[response truncated]");
            }

            // WHY: Return an error-typed result for non-2xx so the LLM can
            // distinguish network success from HTTP failure without parsing
            // the status line. Body is still included for context.
            if status.is_success() {
                Ok(ToolResult::text(rendered))
            } else {
                Ok(ToolResult::error(rendered))
            }
        })
    }
}

/// Register the `http_request` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(http_request_def(), Box::new(HttpRequestExecutor))?;
    Ok(())
}

fn http_request_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("http_request"), // kanon:ignore RUST/expect
        description:
            "Make an HTTP request (GET/POST/PUT/DELETE/PATCH/HEAD) with configurable headers and body. Returns status, headers, and body."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "url".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Absolute http(s) URL".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "method".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "HTTP method (default: GET)".to_owned(),
                        enum_values: Some(vec![
                            "GET".to_owned(),
                            "POST".to_owned(),
                            "PUT".to_owned(),
                            "DELETE".to_owned(),
                            "PATCH".to_owned(),
                            "HEAD".to_owned(),
                        ]),
                        default: Some(serde_json::json!("GET")),
                    },
                ),
                (
                    "headers".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Object,
                        description: "Request headers as a string->string map".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "body".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Request body (sent verbatim)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeoutSecs".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Request timeout in seconds (default 30)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30)),
                    },
                ),
            ]),
            required: vec!["url".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_method_accepts_standard_methods() {
        for m in ["GET", "post", "Put", "DELETE", "patch", "head"] {
            parse_method(m).expect("standard method should parse");
        }
    }

    #[test]
    fn parse_method_rejects_unknown() {
        assert!(parse_method("FROBNICATE").is_err());
    }

    #[test]
    fn extract_headers_rejects_forbidden() {
        let args = serde_json::json!({ "headers": { "Host": "evil.example.com" } });
        let err = extract_headers(&args).expect_err("host header must be rejected");
        assert!(err.contains("not permitted"), "error should explain reason");
    }

    #[test]
    fn extract_headers_accepts_normal_headers() {
        let args = serde_json::json!({
            "headers": { "Content-Type": "application/json", "X-Request-Id": "abc" }
        });
        let h = extract_headers(&args).expect("headers should parse");
        assert_eq!(
            h.get("Content-Type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(h.get("X-Request-Id").map(String::as_str), Some("abc"));
    }

    #[test]
    fn extract_headers_rejects_non_string_values() {
        let args = serde_json::json!({ "headers": { "X": 42 } });
        assert!(extract_headers(&args).is_err());
    }

    #[test]
    fn is_private_ip_flags_loopback_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }

    #[test]
    fn is_private_ip_flags_aws_metadata() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
    }

    #[test]
    fn is_private_ip_allows_public_v4() {
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[test]
    fn http_request_def_is_lazy() {
        let def = http_request_def();
        assert!(!def.auto_activate);
        assert_eq!(def.category, ToolCategory::Research);
    }
}
