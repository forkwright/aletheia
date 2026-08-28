//! Web research tools: web_fetch (HTTP GET to text).
//!
//! Web search is now handled by Anthropic's server-side `web_search` tool,
//! configured via `NousConfig.server_tools`. This module only provides
//! `web_fetch` for direct URL retrieval.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::http::{TokioHostResolver, validate_url_not_internal};
use koina::id::ToolName;

use crate::builtins::http_client::{SafeRequest, send_with_safe_redirects};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::sandbox::{EgressGate, SandboxConfig};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, RollbackSupport, ToolCapabilityMetadata,
    ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult, ToolStability, ToolTag,
};

use super::workspace::{extract_opt_u64, extract_str};

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

struct WebFetchExecutor {
    egress: EgressGate,
}

impl ToolExecutor for WebFetchExecutor {
    // NOTE(#940): 105 lines: single SSRF-safe HTTP fetch operation: validate URL,
    // build client, send response, process response. One cohesive I/O operation.
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

            // SECURITY(#5071): the egress checkpoint runs before any other
            // work so `egress = "deny"` rejects immediately, matching the
            // subprocess sandbox's behavior of blocking socket creation
            // before a child process gets to run at all.
            if let Err(e) = self.egress.check_before_connect() {
                return Ok(ToolResult::error(e.to_string()));
            }

            let url = extract_str(&input.arguments, "url", &input.name)?;
            let max_length = extract_opt_u64(&input.arguments, "maxLength").unwrap_or(50_000);

            // WHY: protocol validation for user-supplied URLs, not endpoint
            // construction. The shared helper keeps the plaintext HTTP literal
            // in one audited place.
            if !koina::http::has_http_or_https_scheme(url) {
                return Ok(ToolResult::error("URL must start with http:// or https://"));
            }

            if let Err(msg) = validate_url_not_internal(url).await {
                return Ok(ToolResult::error(msg));
            }

            let mut headers = HashMap::new();
            headers.insert(
                "User-Agent".to_owned(),
                concat!(
                    "aletheia/",
                    env!("CARGO_PKG_VERSION"),
                    " (github.com/forkwright/aletheia)"
                )
                .to_owned(),
            );

            // SECURITY(#5071, #5229): drives the same redirect/egress chain
            // `http_request` uses (every hop revalidated against `gate` and
            // `resolver`, remote address checked post-connect) rather than a
            // second, independently-maintained GET-only copy of it.
            let response = match send_with_safe_redirects(
                &services.http_clients.ssrf_safe,
                SafeRequest {
                    method: reqwest::Method::GET,
                    url,
                    headers: &headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(30),
                },
                &TokioHostResolver,
                &self.egress,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            if !response.status().is_success() {
                return Ok(ToolResult::error(format!("HTTP {}", response.status())));
            }

            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();

            let body = match response.text().await {
                Ok(t) => t,
                Err(e) => return Ok(ToolResult::error(format!("failed to read body: {e}"))),
            };

            let text = if content_type.contains("text/html") {
                strip_html_tags(&body)
            } else {
                body
            };

            let max_len = usize::try_from(max_length).unwrap_or(usize::MAX);
            let truncated = if text.len() > max_len {
                let mut end = max_len;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!(
                    "{}...\n\n[Truncated at {max_length} characters]",
                    text.get(..end).unwrap_or(&text)
                )
            } else {
                text
            };

            Ok(ToolResult::text(truncated))
        })
    }
}

// NOTE(#940): 100 lines: byte-level HTML tag stripping state machine. The sequential
// character-by-character logic is inherently one operation; splitting would break the
// state machine's flow.
/// Strip HTML tags and collapse whitespace for readable text extraction.
#[expect(
    clippy::indexing_slicing,
    reason = "byte-level state machine: all accesses are bounds-checked by while i < bytes.len() and explicit length guards"
)]
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_whitespace = false;

    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let lower_bytes = lower.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // kanon:ignore RUST/indexing-slicing
            if i + 7 < lower_bytes.len() && &lower_bytes[i..i + 7] == b"<script" {
                in_script = true;
            }
            if i + 9 < lower_bytes.len() && &lower_bytes[i..i + 9] == b"</script>" {
                in_script = false;
                in_tag = false;
                i += 9;
                continue;
            }
            if i + 6 < lower_bytes.len() && &lower_bytes[i..i + 6] == b"<style" {
                in_style = true;
            }
            if i + 8 < lower_bytes.len() && &lower_bytes[i..i + 8] == b"</style>" {
                in_style = false;
                in_tag = false;
                i += 8;
                continue;
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if bytes[i] == b'>' {
            // kanon:ignore RUST/indexing-slicing
            in_tag = false;
            if !last_was_whitespace && !result.is_empty() {
                result.push(' ');
                last_was_whitespace = true;
            }
            i += 1;
            continue;
        }

        if in_tag || in_script || in_style {
            i += 1;
            continue;
        }

        if bytes[i] == b'&' {
            // kanon:ignore RUST/indexing-slicing
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"&lt;" {
                result.push('<');
                last_was_whitespace = false;
                i += 4;
                continue;
            }
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"&gt;" {
                result.push('>');
                last_was_whitespace = false;
                i += 4;
                continue;
            }
            if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"&amp;" {
                result.push('&');
                last_was_whitespace = false;
                i += 5;
                continue;
            }
            if i + 6 <= bytes.len() && &bytes[i..i + 6] == b"&nbsp;" {
                result.push(' ');
                last_was_whitespace = true;
                i += 6;
                continue;
            }
            if i + 6 <= bytes.len() && &bytes[i..i + 6] == b"&quot;" {
                result.push('"');
                last_was_whitespace = false;
                i += 6;
                continue;
            }
        }

        let ch = char::from(bytes[i]); // kanon:ignore RUST/indexing-slicing
        if ch.is_ascii_whitespace() {
            if !last_was_whitespace && !result.is_empty() {
                result.push(' ');
                last_was_whitespace = true;
            }
        } else {
            result.push(ch);
            last_was_whitespace = false;
        }
        i += 1;
    }

    result.trim().to_owned()
}

fn web_fetch_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("web_fetch"), // kanon:ignore RUST/expect
        description:
            "Fetch a URL and return its content as text. HTML pages are converted to readable text."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "url".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "URL to fetch".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "maxLength".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum response length in characters (default: 50000)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(50000)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["url".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read, ToolGroupId::Mcp],
        tags: vec![ToolTag::Fetch],
    }
}

/// Register the `web_fetch` research tool into the registry.
///
/// # Errors
///
/// Returns an error if `web_fetch` is already registered.
pub(crate) fn register(registry: &mut ToolRegistry, sandbox: &SandboxConfig) -> Result<()> {
    let egress = EgressGate::from_config(sandbox);
    registry.register(web_fetch_def(), Box::new(WebFetchExecutor { egress }))?;
    registry.declare_capability(
        ToolName::from_static("web_fetch"), // kanon:ignore RUST/expect
        ToolCapabilityMetadata {
            owner: "organon::builtins::research".to_owned(),
            stability: ToolStability::Stable,
            // WHY Supported, unlike this tool's sibling Irreversible
            // classifications: web_fetch is a read (GET the URL, return
            // text) that mutates no local aletheia state. Reversibility
            // here drives approval gating for the SSRF/egress risk of an
            // agent-chosen URL, not an undo requirement -- there is
            // nothing on aletheia's side to roll back.
            rollback: RollbackSupport::Supported,
            ..ToolCapabilityMetadata::default()
        },
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::testing::install_crypto_provider;
    use crate::types::{ServerToolConfig, ToolContext, ToolHttpClients, ToolInput, ToolServices};

    use super::*;

    fn mock_ctx() -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    #[test]
    fn strip_html_basic() {
        let html = "<html><body><h1>Title</h1><p>Hello world</p></body></html>";
        let text = strip_html_tags(html);
        assert_eq!(
            text, "Title Hello world",
            "expected text to equal \"Title Hello world\""
        );
    }

    #[test]
    fn strip_html_script_and_style() {
        let html = "<p>Before</p><script>var x = 1;</script><style>.a{}</style><p>After</p>";
        let text = strip_html_tags(html);
        assert_eq!(
            text, "Before After",
            "expected text to equal \"Before After\""
        );
    }

    #[test]
    fn strip_html_entities() {
        let html = "&amp; &lt;tag&gt; &quot;quoted&quot;";
        let text = strip_html_tags(html);
        assert_eq!(
            text, "& <tag> \"quoted\"",
            "expected text to equal \"& <tag> \"quoted\"\""
        );
    }

    #[test]
    fn strip_html_whitespace_collapse() {
        let html = "<p>  lots   of    spaces  </p>";
        let text = strip_html_tags(html);
        assert_eq!(
            text, "lots of spaces",
            "expected text to equal \"lots of spaces\""
        );
    }

    #[test]
    fn web_fetch_def_is_lazy() {
        let def = web_fetch_def();
        assert!(!def.auto_activate, "expected def.auto_activate to be false");
        assert_eq!(
            def.category,
            ToolCategory::Research,
            "expected def.category to equal ToolCategory::Research"
        );
    }

    fn test_executor() -> WebFetchExecutor {
        WebFetchExecutor {
            egress: EgressGate::new(crate::sandbox::EgressPolicy::Allow, &[]),
        }
    }

    #[tokio::test]
    async fn web_fetch_blocked_by_egress_deny() {
        // SECURITY(#5071) regression: the anchor issue. With egress=deny,
        // web_fetch must refuse before making any network attempt.
        let ctx = mock_ctx();
        let executor = WebFetchExecutor {
            egress: EgressGate::new(crate::sandbox::EgressPolicy::Deny, &[]),
        };
        let input = ToolInput {
            name: ToolName::from_static("web_fetch"),
            tool_use_id: "toolu_deny".to_owned(),
            arguments: serde_json::json!({"url": "https://example.com"}),
        };
        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error, "egress=deny must block web_fetch");
        assert!(
            result.content.text_summary().contains("deny"),
            "error should name the egress policy: {result:?}"
        );
    }

    #[tokio::test]
    async fn web_fetch_invalid_url() {
        let ctx = mock_ctx();
        let executor = test_executor();
        let input = ToolInput {
            name: ToolName::from_static("web_fetch"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"url": "not-a-url"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("http"),
            "expected result.content.text_summary().contains(\"http\") to be true"
        );
    }

    #[tokio::test]
    async fn web_fetch_rejects_dns_rebound_response_from_configured_client() {
        // SECURITY(#5229) regression + plumbing check, combined: start a local
        // HTTP server and use reqwest's per-client DNS override to point
        // example.com at it (a stand-in for a DNS answer changing between the
        // pre-connect SSRF check and the client's own connect-time
        // resolution). Two things must both be true:
        //  1. The local server DID receive the request -- proving web_fetch
        //     routed through the operator-configured `ssrf_http_client`
        //     rather than building a fresh one (a fresh client would ignore
        //     the override and either hit the real example.com or fail).
        //  2. The result is nonetheless an error -- proving the post-connect
        //     remote_addr() check rejects the rebound private/loopback peer
        //     instead of silently returning its body to the LLM.
        install_crypto_provider();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let local_addr = listener.local_addr().expect("local addr");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let n = stream.read(&mut chunk).await.expect("read");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(chunk.get(..n).expect("n is bounded by chunk.len()"));
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&buf);
            assert!(
                request.contains("GET / HTTP/1.1"),
                "expected GET request, got: {request}"
            );
            let body = "hello from configured client";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.expect("write");
        });

        let ssrf_http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .resolve("example.com", local_addr)
            .build()
            .expect("test client should build");
        let ctx = mock_ctx_with_ssrf_client(ssrf_http_client);
        let executor = test_executor();
        let input = ToolInput {
            name: ToolName::from_static("web_fetch"),
            tool_use_id: "toolu_2".to_owned(),
            arguments: serde_json::json!({"url": "http://example.com"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");

        // Proof 1: the local server saw the request (plumbing used the
        // configured client + its DNS override), asserted inside `server`.
        server.await.expect("server task");

        // Proof 2: the rebound (loopback) peer was rejected, not returned.
        assert!(
            result.is_error,
            "a response from a DNS-rebound private address must be rejected, got: {result:?}"
        );
        let summary = result.content.text_summary();
        assert!(
            summary.contains("private") || summary.contains("rebind"),
            "error should explain the rejection: {result:?}"
        );
    }

    fn mock_ctx_with_ssrf_client(ssrf_http_client: reqwest::Client) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_clients: ToolHttpClients {
                    general: reqwest::Client::new(),
                    ssrf_safe: ssrf_http_client,
                },
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }
}
