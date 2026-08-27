//! `web_search` tool — agent-side web search via the Brave Search API.
//!
//! WHY: Anthropic's server-side `web_search` tool only works with
//! Anthropic-hosted models; agents on other providers (Kimi, local Qwen, etc.)
//! need an agent-side search path.
//!
//! Availability: requires `BRAVE_SEARCH_API_KEY` in the environment. If the
//! key is absent the tool still registers, but every call returns an error
//! instructing the operator to configure the key. We deliberately register
//! unconditionally so tool discovery is deterministic; the runtime check
//! surfaces the missing configuration.
//!
//! WHY Brave: free tier supports 2 000 queries / month, returns JSON, and
//! does not require a Google CSE, scraping, or browser. DuckDuckGo has no
//! official JSON search API (only the Instant Answer endpoint, which rarely
//! returns web results).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use serde::Deserialize;

use koina::http::TokioHostResolver;
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

const BRAVE_SEARCH_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
const API_KEY_ENV: &str = "BRAVE_SEARCH_API_KEY";

/// Upper bound on the number of results returned to the LLM.
const MAX_RESULTS: u64 = 20;

#[derive(Debug, Deserialize)]
struct BraveResponse {
    #[serde(default)]
    web: Option<BraveWeb>,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Debug, Deserialize)]
struct BraveResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    description: String,
}

#[expect(
    clippy::result_large_err,
    reason = "ToolResult grew by receipt field; boxing would change public API"
)]
/// Take the SSRF-safe client, whose redirects are disabled.
///
/// SECURITY(#6910): this tool sends `X-Subscription-Token`, and reqwest strips
/// only `Authorization`, `Cookie`, `Proxy-Authorization` and `WWW-Authenticate`
/// across a cross-host redirect -- a custom header rides along to whatever host
/// the response names. On the general client, whose default policy follows ten
/// redirects with no per-hop revalidation, that hands the credential to an
/// attacker-chosen destination. `send_with_safe_redirects` drives the chain
/// itself and puts every hop through the same egress checkpoint `http_request`
/// uses, which is why it needs the client that does not redirect on its own.
fn require_http_client(ctx: &ToolContext) -> std::result::Result<reqwest::Client, ToolResult> {
    ctx.services
        .as_deref()
        .map(|s| s.http_clients.ssrf_safe.clone())
        .ok_or_else(|| ToolResult::error("tool services not configured"))
}

struct WebSearchExecutor {
    egress: EgressGate,
}

impl WebSearchExecutor {
    fn new(egress: EgressGate) -> Self {
        Self { egress }
    }
}

impl ToolExecutor for WebSearchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            // SECURITY(#5071): the egress checkpoint runs before any other
            // work, including the API-key check, so `egress = "deny"`
            // rejects immediately and consistently regardless of whether
            // the operator has configured Brave Search.
            if let Err(e) = self.egress.check_before_connect() {
                return Ok(ToolResult::error(e.to_string()));
            }

            let query = extract_str(&input.arguments, "query", &input.name)?;
            let requested = extract_opt_u64(&input.arguments, "maxResults").unwrap_or(5);
            let count = requested.clamp(1, MAX_RESULTS);

            let api_key = match std::env::var(API_KEY_ENV) {
                Ok(k) if !k.is_empty() => k,
                _ => {
                    return Ok(ToolResult::error(format!(
                        "web_search unavailable: set {API_KEY_ENV} in the environment (Brave Search API key required)"
                    )));
                }
            };

            let client = match require_http_client(ctx) {
                Ok(c) => c,
                Err(r) => return Ok(r),
            };

            let mut url: reqwest::Url = match BRAVE_SEARCH_ENDPOINT.parse() {
                Ok(u) => u,
                Err(e) => {
                    return Ok(ToolResult::error(format!("invalid search endpoint: {e}")));
                }
            };
            url.query_pairs_mut()
                .append_pair("q", query)
                .append_pair("count", &count.to_string());

            let mut headers = HashMap::new();
            headers.insert("X-Subscription-Token".to_owned(), api_key);
            headers.insert("Accept".to_owned(), "application/json".to_owned());
            headers.insert(
                "User-Agent".to_owned(),
                concat!(
                    "aletheia/",
                    env!("CARGO_PKG_VERSION"),
                    " (organon web_search)"
                )
                .to_owned(),
            );

            // SECURITY(#6910, #5071, #5229): every hop -- the first request and
            // any redirect the endpoint names -- goes through the same egress
            // checkpoint and internal-address revalidation `http_request` uses.
            // The separate pre-flight this replaces validated only the fixed
            // endpoint and then left the client to follow redirects on its own,
            // carrying `X-Subscription-Token` to wherever they led.
            let response = match send_with_safe_redirects(
                &client,
                SafeRequest {
                    method: reqwest::Method::GET,
                    url: url.as_str(),
                    headers: &headers,
                    body: None,
                    timeout: std::time::Duration::from_secs(20),
                },
                &TokioHostResolver,
                &self.egress,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("search failed: {e}"))),
            };

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default -- WHY: error path; empty body preferred over secondary decode error
                return Ok(ToolResult::error(format!(
                    "search API returned {status}: {body}"
                )));
            }

            let parsed: BraveResponse = match response.json().await {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "failed to parse search response: {e}"
                    )));
                }
            };

            let results = parsed.web.map(|w| w.results).unwrap_or_default();

            if results.is_empty() {
                return Ok(ToolResult::text(format!("no results for: {query}")));
            }

            let mut out = String::new();
            for (i, r) in results.iter().enumerate() {
                let n = i + 1;
                let _ = writeln!(
                    out,
                    "{n}. {title}\n   {url}\n   {desc}\n",
                    title = r.title,
                    url = r.url,
                    desc = r.description
                );
            }

            Ok(ToolResult::text(out.trim_end().to_owned()))
        })
    }
}

/// Register the `web_search` tool.
///
/// # Errors
///
/// Returns an error if `web_search` is already registered.
pub(crate) fn register(registry: &mut ToolRegistry, sandbox: &SandboxConfig) -> Result<()> {
    let egress = EgressGate::from_config(sandbox);
    registry.register(web_search_def(), Box::new(WebSearchExecutor::new(egress)))?;
    registry.declare_capability(
        ToolName::from_static("web_search"), // kanon:ignore RUST/expect
        ToolCapabilityMetadata {
            owner: "organon::builtins::web_search".to_owned(),
            stability: ToolStability::Stable,
            // WHY Supported: the executor issues one egress-gated GET to the
            // Brave Search API and mutates no local state, so there is
            // nothing on aletheia's side to roll back.
            rollback: RollbackSupport::Supported,
            ..ToolCapabilityMetadata::default()
        },
    )?;
    Ok(())
}

fn web_search_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("web_search"), // kanon:ignore RUST/expect
        description: concat!(
            "Search the web via Brave Search. Returns a ranked list of ",
            "title/URL/snippet. Requires BRAVE_SEARCH_API_KEY in the environment."
        )
        .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "query".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Search query".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "maxResults".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum results (1-20, default 5)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(5)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["query".to_owned()],
        },
        category: ToolCategory::Research,
        // WHY: web_search contacts a third-party API using an operator API key,
        // consuming quota and producing external network traffic and privacy
        // side-effects. It is not fully reversible even though it does not
        // mutate local files; reversible network/quota calls need their own
        // classification so they are not automatically daemon-safe.
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read, ToolGroupId::Mcp],
        tags: vec![ToolTag::Fetch, ToolTag::Recon],
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId};

    use crate::types::{ServerToolConfig, ToolContext, ToolHttpClients, ToolServices};

    use super::*;

    // WHY(#3693): reqwest 0.13 requires a rustls crypto provider to be
    // installed before any `Client` is constructed. `mock_ctx` builds a
    // `reqwest::Client`; without this, the test panics with
    // "No provider set". `install_default` fails if the process already
    // installed one, so we swallow the result.
    fn ensure_crypto_provider() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    fn mock_ctx() -> ToolContext {
        ensure_crypto_provider();
        ToolContext {
            nous_id: NousId::new("alice").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp"),
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
    fn web_search_def_is_lazy() {
        let def = web_search_def();
        assert!(!def.auto_activate);
        assert_eq!(def.category, ToolCategory::Research);
    }

    fn test_executor() -> WebSearchExecutor {
        WebSearchExecutor::new(EgressGate::new(crate::sandbox::EgressPolicy::Allow, &[]))
    }

    #[tokio::test]
    async fn web_search_blocked_by_egress_deny() {
        // SECURITY(#5071) regression: web_search must consult sandbox.egress
        // before ever attempting to reach Brave Search, even when an API key
        // is configured.
        let ctx = mock_ctx();
        let executor =
            WebSearchExecutor::new(EgressGate::new(crate::sandbox::EgressPolicy::Deny, &[]));
        let input = ToolInput {
            name: ToolName::new("web_search").expect("valid"),
            tool_use_id: "toolu_deny".to_owned(),
            arguments: serde_json::json!({ "query": "should never reach the network" }),
        };
        let result = executor.execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error, "egress=deny must block web_search");
        assert!(
            result.content.text_summary().contains("deny"),
            "error should name the egress policy: {result:?}"
        );
    }

    #[test]
    fn registration_registers_web_search() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg, &crate::sandbox::SandboxConfig::default()).expect("register");
        let tn = ToolName::new("web_search").expect("valid");
        assert!(reg.get_def(&tn).is_some());
    }

    #[tokio::test]
    async fn missing_api_key_returns_configuration_error() {
        // SAFETY: we remove the env var for the duration of this test.
        // Serial-by-convention: tests in this module do not run in parallel
        // on the API key because only this test manipulates it.
        // SAFETY: set_var is unsafe on recent Rust; this test owns the env
        // manipulation scope.
        #[expect(
            unsafe_code,
            reason = "std::env::remove_var is unsafe on current Rust; test owns the scope"
        )]
        // SAFETY: test-scoped env manipulation; no other threads depend on this key.
        unsafe {
            std::env::remove_var(API_KEY_ENV);
        }

        let ctx = mock_ctx();
        let input = ToolInput {
            name: ToolName::new("web_search").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({ "query": "alice and bob" }),
        };
        let result = test_executor().execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error, "missing key must surface an error");
        assert!(
            result.content.text_summary().contains(API_KEY_ENV),
            "error should name the required env var"
        );
    }
}
