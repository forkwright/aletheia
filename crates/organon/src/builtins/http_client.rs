//! Generic HTTP client tool (`http_request`) supporting POST/PUT/DELETE/PATCH
//! in addition to GET, with configurable headers and body.
//!
//! WHY: `web_fetch` handles only GET and strips HTML, leaving agents that need
//! to call REST APIs (POST JSON, PUT YAML, DELETE) without a path. This is the
//! generic method-aware HTTP tool, reusing the SSRF guards from `research.rs`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use reqwest::{Method, StatusCode};

use koina::http::{
    HostResolver, TokioHostResolver, validate_url_not_internal,
    validate_url_not_internal_with_resolver,
};
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    AdditionalProperties, InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory,
    ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::{extract_opt_str, extract_opt_u64, extract_str};

#[expect(
    clippy::result_large_err,
    reason = "ToolResult grew by receipt field; boxing would change public API"
)]
fn require_services(
    ctx: &ToolContext,
) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("tool services not configured"))
}

/// Headers that must never be forwarded from LLM input.
///
/// WHY: Host/Content-Length are computed by reqwest; the LLM setting them
/// produces malformed requests. Authorization, Cookie, and X-Api-Key are
/// credential-bearing; allowing agents to inject them without an explicit
/// operator-approved surface enables credential exfiltration through
/// arbitrary outbound HTTP requests. Forwarding/hop-by-hop headers
/// (X-Forwarded-For, Forwarded, Via, TE, etc.) are stripped for
/// defense-in-depth -- agents should not be able to annotate outbound
/// requests with origin-metadata, even to external public servers.
const FORBIDDEN_REQUEST_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "x-auth-token",
    // forwarding / hop-by-hop (defense-in-depth, #6004)
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-forwarded-port",
    "x-real-ip",
    "forwarded",
    "via",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "connection",
    "keep-alive",
    "proxy-connection",
];

/// Upper bound on response body size forwarded to the LLM.
const MAX_RESPONSE_BYTES: usize = 1_000_000;

const MAX_REDIRECTS: usize = 5;

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

async fn validated_redirect_target<R>(
    current_url: &reqwest::Url,
    location: &str,
    redirects_followed: usize,
    resolver: &R,
) -> std::result::Result<reqwest::Url, String>
where
    R: HostResolver + ?Sized,
{
    if redirects_followed >= MAX_REDIRECTS {
        return Err(format!("redirect limit exceeded: max {MAX_REDIRECTS}"));
    }

    let next_url = current_url
        .join(location)
        .map_err(|e| format!("invalid redirect Location: {e}"))?;

    // WARNING: DNS may change after validation and before reqwest connects.
    // The guard revalidates every URL before following it, but it cannot pin
    // the resolved address for the subsequent connection.
    validate_url_not_internal_with_resolver(next_url.as_str(), resolver).await?;

    Ok(next_url)
}

fn redirect_method(method: &Method, status: StatusCode) -> Method {
    if status == StatusCode::SEE_OTHER
        || ((status == StatusCode::MOVED_PERMANENTLY || status == StatusCode::FOUND)
            && *method != Method::GET
            && *method != Method::HEAD)
    {
        Method::GET
    } else {
        method.clone()
    }
}

async fn send_with_safe_redirects<R>(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    timeout: std::time::Duration,
    resolver: &R,
) -> std::result::Result<reqwest::Response, String>
where
    R: HostResolver + ?Sized,
{
    let mut current_url: reqwest::Url = url.parse().map_err(|e| format!("invalid URL: {e}"))?;
    let mut current_method = method;
    let mut include_body = body.is_some();
    let mut redirects_followed = 0;

    loop {
        let mut req = client
            .request(current_method.clone(), current_url.clone())
            .timeout(timeout)
            .header(
                "User-Agent",
                concat!(
                    "aletheia/",
                    env!("CARGO_PKG_VERSION"),
                    " (organon http_request)"
                ),
            );
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if include_body && let Some(b) = body {
            req = req.body(b.to_owned());
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        if !response.status().is_redirection() {
            return Ok(response);
        }

        let status = response.status();
        let Some(location) = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
        else {
            return Ok(response);
        };

        current_url =
            validated_redirect_target(&current_url, location, redirects_followed, resolver).await?;
        let next_method = redirect_method(&current_method, status);
        include_body = include_body
            && next_method == current_method
            && (status == StatusCode::TEMPORARY_REDIRECT
                || status == StatusCode::PERMANENT_REDIRECT);
        current_method = next_method;
        redirects_followed += 1;
    }
}

struct HttpRequestExecutor;

impl ToolExecutor for HttpRequestExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(r) => return Ok(r),
            };

            let url = extract_str(&input.arguments, "url", &input.name)?;
            let method_str = extract_opt_str(&input.arguments, "method").unwrap_or("GET");
            let body = extract_opt_str(&input.arguments, "body");
            let timeout_secs = extract_opt_u64(&input.arguments, "timeoutSecs").unwrap_or(30);

            // WHY: validating user-supplied URL scheme, not constructing an
            // endpoint. Delegates to the shared helper so the plaintext HTTP
            // literal stays in one audited place.
            if !koina::http::has_http_or_https_scheme(url) {
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

            let response = match send_with_safe_redirects(
                &services.http_clients.ssrf_safe,
                method.clone(),
                url,
                &headers,
                body,
                std::time::Duration::from_secs(timeout_secs),
                &TokioHostResolver,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(e)),
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
                        ..Default::default()
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
                        ..Default::default()
                    },
                ),
                (
                    "headers".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Object,
                        description: "Request headers as a string->string map".to_owned(),
                        enum_values: None,
                        default: None,
                        additional_properties: Some(AdditionalProperties::Schema(Box::new(
                            PropertyDef {
                                property_type: PropertyType::String,
                                description: "Header value".to_owned(),
                                ..Default::default()
                            },
                        ))),
                        ..Default::default()
                    },
                ),
                (
                    "body".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Request body (sent verbatim)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "timeoutSecs".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Request timeout in seconds (default 30)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["url".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Mcp],
        tags: vec![ToolTag::Fetch],
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashMap;
    use std::net::SocketAddr;

    use koina::http::ResolveHostFuture;

    use super::*;

    #[derive(Default)]
    struct MockResolver {
        addrs_by_host: HashMap<String, Vec<SocketAddr>>,
    }

    impl HostResolver for MockResolver {
        fn resolve_host<'a>(&'a self, host: &'a str, _port: u16) -> ResolveHostFuture<'a> {
            Box::pin(async move {
                self.addrs_by_host
                    .get(host)
                    .cloned()
                    .ok_or_else(|| format!("missing mock host: {host}"))
            })
        }
    }

    fn public_base_url() -> reqwest::Url {
        "https://public.example/start"
            .parse()
            .expect("test URL should parse")
    }

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
    fn http_request_def_is_lazy() {
        let def = http_request_def();
        assert!(!def.auto_activate);
        assert_eq!(def.category, ToolCategory::Research);
    }

    #[tokio::test]
    async fn redirect_to_private_ip_literal_is_blocked() {
        let mut resolver = MockResolver::default();
        resolver.addrs_by_host.insert(
            "169.254.169.254".to_owned(),
            vec![SocketAddr::from(([169, 254, 169, 254], 443))],
        );

        let err = validated_redirect_target(
            &public_base_url(),
            "https://169.254.169.254/latest",
            0,
            &resolver,
        )
        .await
        .expect_err("private redirect target must be rejected");

        assert!(err.contains("private/internal"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn redirect_to_blocked_hostname_is_blocked() {
        let err = validated_redirect_target(
            &public_base_url(),
            "https://localhost/admin",
            0,
            &MockResolver::default(),
        )
        .await
        .expect_err("blocked hostname must be rejected");

        assert!(err.contains("blocked hostname"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn redirect_to_hostname_resolving_private_is_blocked() {
        let mut resolver = MockResolver::default();
        resolver.addrs_by_host.insert(
            "rebind.example".to_owned(),
            vec![SocketAddr::from(([10, 0, 0, 7], 443))],
        );

        let err = validated_redirect_target(
            &public_base_url(),
            "https://rebind.example/metadata",
            0,
            &resolver,
        )
        .await
        .expect_err("private DNS redirect target must be rejected");

        assert!(err.contains("private/internal"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn sixth_redirect_is_refused() {
        let err = validated_redirect_target(
            &public_base_url(),
            "https://public.example/six",
            5,
            &MockResolver::default(),
        )
        .await
        .expect_err("sixth redirect must be rejected");

        assert!(err.contains("redirect limit"), "unexpected error: {err}");
    }

    #[test]
    fn extract_headers_rejects_forwarding_hop_by_hop_headers() {
        // WHY: regression for #6004 -- defense-in-depth denylist must cover
        // forwarding/hop-by-hop headers so agents cannot annotate outbound
        // requests with origin-metadata.
        let blocked = [
            "X-Forwarded-For",
            "X-Forwarded-Host",
            "X-Forwarded-Proto",
            "X-Forwarded-Port",
            "X-Real-IP",
            "Forwarded",
            "Via",
            "TE",
            "Trailer",
            "Transfer-Encoding",
            "Upgrade",
            "Connection",
            "Keep-Alive",
            "Proxy-Connection",
        ];
        for header in blocked {
            let args = serde_json::json!({ "headers": { header: "injected" } });
            let err =
                extract_headers(&args).expect_err("forwarding/hop-by-hop header must be rejected");
            assert!(
                err.contains("not permitted"),
                "header {header} must be rejected, got: {err}"
            );
        }
    }
}
