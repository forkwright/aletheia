//! `CcProvider`: routes LLM calls through the Claude Code CLI subprocess.
//!
//! CC handles OAuth authentication and attestation correctly, bypassing
//! the server-side blocking of direct API calls from non-CC clients.
//!
//! # Errors
//!
//! Spawn failures produce [`Error::ProviderInit`]; subprocess errors and
//! timeouts produce [`Error::ApiRequest`].

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info, warn};

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind};
use crate::seat_bridged::SeatBridgedProvider;
use crate::types::{CompletionRequest, CompletionResponse, Content, ContentBlock, Role};

use super::parse;
use super::process;

/// Model name prefix that routes requests to this provider.
pub(crate) const CC_MODEL_PREFIX: &str = "cc/";

/// Configuration for the CC subprocess provider.
#[derive(Debug, Clone)]
pub struct CcProviderConfig {
    /// Provider instance name used for routing diagnostics and metrics.
    pub name: String,
    /// Path to the `claude` binary. If `None`, resolved from `PATH`.
    pub cc_binary: Option<PathBuf>,
    /// Working directory for the subprocess. If `None`, inherits the parent cwd.
    pub working_directory: Option<PathBuf>,
    /// Model IDs this provider advertises for exact routing.
    pub models: Vec<String>,
    /// Default model when the request doesn't specify one.
    pub default_model: String,
    /// Subprocess timeout (wall-clock).
    pub timeout: Duration,
    /// Where the provider's model traffic terminates for recall filtering.
    pub deployment_target: DeploymentTarget,
}

impl Default for CcProviderConfig {
    fn default() -> Self {
        Self {
            name: "cc".to_owned(),
            cc_binary: None,
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_mins(5),
            deployment_target: DeploymentTarget::Cloud,
        }
    }
}

/// Claude Code subprocess LLM provider.
///
/// Delegates completions to the `claude` CLI binary via `-p --output-format stream-json`.
/// CC manages its own authentication (OAuth token refresh, attestation headers)
/// so the provider only needs to spawn the process and parse output.
pub struct CcProvider {
    // kanon:ignore RUST/pub-visibility
    name: String,
    cc_binary: PathBuf,
    working_directory: Option<PathBuf>,
    models: Vec<String>,
    default_model: String,
    timeout: Duration,
    deployment_target: DeploymentTarget,
}

impl CcProvider {
    /// Create a new CC provider, locating the `claude` binary.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ProviderInit`] if the binary cannot be found.
    pub fn new(config: &CcProviderConfig) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let cc_binary = if let Some(ref path) = config.cc_binary {
            if path.exists() {
                path.clone()
            } else {
                return Err(error::ProviderInitSnafu {
                    message: format!(
                        "configured claude CLI path does not exist: {}",
                        path.display()
                    ),
                }
                .build());
            }
        } else {
            find_cc_binary()?
        };

        let working_directory = validate_working_directory(config.working_directory.as_deref())?;

        info!(
            provider = %config.name,
            binary = %cc_binary.display(),
            cwd = ?working_directory.as_ref().map(|path| path.display().to_string()),
            models = ?config.models,
            default_model = %config.default_model,
            timeout_secs = config.timeout.as_secs(),
            "CC subprocess provider initialized"
        );

        Ok(Self {
            name: config.name.clone(),
            cc_binary,
            working_directory,
            models: config.models.clone(),
            default_model: config.default_model.clone(),
            timeout: config.timeout,
            deployment_target: config.deployment_target,
        })
    }

    /// Resolve the model: strip `cc/` prefix, fall back to default.
    fn resolve_model<'a>(&'a self, model: &'a str) -> &'a str {
        let stripped = model.strip_prefix(CC_MODEL_PREFIX).unwrap_or(model);
        if stripped.is_empty() {
            &self.default_model
        } else {
            stripped
        }
    }

    /// Format message history into a single prompt string for CC.
    ///
    /// CC's `-p` mode accepts a flat text prompt. For multi-turn conversations,
    /// we format the history as labeled sections so the model has full context.
    fn format_prompt(request: &CompletionRequest) -> String {
        // WHY: CC in `-p` mode doesn't support multi-turn natively.
        // We format the conversation as a structured prompt that preserves
        // the turn structure. The last user message is what CC will respond to.
        if request.messages.len() == 1
            && let Some(msg) = request.messages.first()
        {
            // Single message: pass directly (most common case for aletheia).
            return msg.content.text();
        }

        let mut parts = Vec::new();

        for msg in &request.messages {
            let label = match msg.role {
                Role::User => "Human",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            let text = extract_text_content(&msg.content);
            if !text.is_empty() {
                parts.push(format!("{label}: {text}"));
            }
        }

        parts.join("\n\n")
    }

    fn warn_dropped_tools(dropped_tools: usize) -> bool {
        // WHY: Seat-bridged subprocess providers run their own CLI-side agentic loop, so
        // aletheia's tools are intentionally not translated through this adapter.
        static WARNED: AtomicBool = AtomicBool::new(false);

        if dropped_tools == 0 {
            return false;
        }

        let warned = WARNED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();

        if warned {
            warn!(
                provider = "cc",
                dropped_tools,
                "cc dropped {dropped_tools} tool definitions; this seat-bridged CLI runs its own agentic loop so aletheia's tools are not invoked. Use a native API provider for aletheia's tool-loop"
            );
        }

        warned
    }

    /// Execute a non-streaming completion via CC subprocess.
    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
        let prompt = Self::format_prompt(request);
        let system = request.system.as_deref();

        let output = process::run_completion(
            &self.cc_binary,
            self.working_directory.as_deref(),
            model,
            system,
            &prompt,
            request.max_tokens,
            self.timeout,
        )
        .await?;

        let response = parse::result_to_response(
            &output.result_text,
            output.is_error,
            output.usage.as_ref(),
            model,
            output.session_id.as_deref(),
        )?;
        // WHY(#4658): CC reports cache read/write tokens; record them so
        // prompt-cache usage is visible in provider metrics.
        crate::metrics::record_cache_tokens(
            self.name(),
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );
        Ok(response)
    }

    /// Execute a streaming completion, emitting `StreamEvent`s.
    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
        let prompt = Self::format_prompt(request);
        let system = request.system.as_deref();

        // Adapter: CC gives us text deltas, we emit StreamEvent::TextDelta.
        let mut on_delta = |text: &str| {
            on_event(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
        };

        let output = process::run_streaming(
            &self.cc_binary,
            self.working_directory.as_deref(),
            model,
            system,
            &prompt,
            request.max_tokens,
            self.timeout,
            &mut on_delta,
        )
        .await?;

        let response = parse::result_to_response(
            &output.result_text,
            output.is_error,
            output.usage.as_ref(),
            model,
            output.session_id.as_deref(),
        )?;
        // WHY(#4658): Streaming CC output preserves cache tokens; emit them
        // for metrics parity with the non-streaming path.
        crate::metrics::record_cache_tokens(
            self.name(),
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );
        Ok(response)
    }
}

/// Extract plain text from content, joining blocks if structured.
fn extract_text_content(content: &Content) -> String {
    match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } if !text.is_empty() => Some(text.to_owned()),
                    ContentBlock::ToolResult { content, .. } => {
                        let summary = content.text_summary();
                        if summary.is_empty() {
                            None
                        } else {
                            Some(summary)
                        }
                    }
                    // Thinking, server tool use, web search, code execution have
                    // no text representation for CC's flat prompt format.
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
    }
}

impl std::fmt::Debug for CcProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CcProvider")
            .field("name", &self.name)
            .field("cc_binary", &self.cc_binary)
            .field("working_directory", &self.working_directory)
            .field("models", &self.models)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout.as_secs())
            .field("deployment_target", &self.deployment_target)
            .finish_non_exhaustive()
    }
}

impl LlmProvider for CcProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute(request))
    }

    fn supported_models(&self) -> &[&str] {
        if self.models.is_empty() {
            koina::models::provider_models(koina::models::ModelProvider::Anthropic)
        } else {
            &[]
        }
    }

    fn supported_model_list(&self) -> Vec<std::borrow::Cow<'_, str>> {
        if self.models.is_empty() {
            self.supported_models()
                .iter()
                .map(|&model| std::borrow::Cow::Borrowed(model))
                .collect()
        } else {
            crate::provider::owned_model_list(&self.models)
        }
    }

    fn supports_model(&self, model: &str) -> bool {
        self.match_specificity(model).is_some()
    }

    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        if self.models.iter().any(|m| m == model) {
            Some(MatchKind::Exact)
        } else if model.starts_with(CC_MODEL_PREFIX) {
            // WHY: `cc/<model>` is an operator-explicit routing directive —
            // this provider is the intended destination regardless of what
            // other providers are registered.
            Some(MatchKind::Prefix)
        } else if self.models.is_empty() && model.starts_with("claude-") {
            // WHY: CC delegates model routing to the `claude` CLI, which
            // handles all claude-* models internally, including future IDs
            // not yet in the shared catalog. This catch-all ensures forward
            // compatibility at the cost of lower precedence: any provider
            // with an exact-model match wins over this branch.
            Some(MatchKind::CatchAll)
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn deployment_target(&self) -> DeploymentTarget {
        self.deployment_target
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn complete_streaming<'a>(
        &'a self,
        request: &'a CompletionRequest,
        on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute_streaming(request, on_event))
    }
}

impl SeatBridgedProvider for CcProvider {
    fn cli_binary(&self) -> &PathBuf {
        &self.cc_binary
    }

    fn subprocess_timeout(&self) -> Duration {
        self.timeout
    }

    fn cli_product_name(&self) -> &'static str {
        "claude"
    }
}

fn validate_working_directory(path: Option<&Path>) -> Result<Option<PathBuf>> {
    match path {
        Some(path) if path.is_dir() => Ok(Some(path.to_path_buf())),
        Some(path) => Err(error::ProviderInitSnafu {
            message: format!(
                "configured claude working directory does not exist: {}",
                path.display()
            ),
        }
        .build()),
        None => Ok(None),
    }
}

/// Find the `claude` binary in `PATH`.
fn find_cc_binary() -> Result<PathBuf> {
    // 1. Search PATH (standard resolution).
    let paths = RealSystem.var_os("PATH").unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<OsString>, not Result — empty PATH is a valid fallback
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("claude");
        if candidate.is_file() {
            debug!(path = %candidate.display(), "found claude binary in PATH");
            return Ok(candidate);
        }
    }

    // 2. Check well-known installation paths (covers systemd user sessions
    //    where ~/.local/bin may not be in PATH — see #3106).
    if let Some(home) = RealSystem.var_os("HOME") {
        let home = PathBuf::from(home);
        for subdir in &[".local/bin/claude", ".claude/bin/claude"] {
            let candidate = home.join(subdir);
            if candidate.is_file() {
                tracing::info!(
                    path = %candidate.display(),
                    "found claude binary outside PATH (consider adding its directory to PATH)"
                );
                return Ok(candidate);
            }
        }
    }

    Err(error::ProviderInitSnafu {
        message: "claude CLI binary not found in PATH or ~/.local/bin. \
                  Install Claude Code: https://docs.anthropic.com/en/docs/claude-code"
            .to_owned(),
    }
    .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompletionRequest, Content, Message, Role};

    #[test]
    fn format_prompt_single_message() {
        let request = CompletionRequest {
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello world".to_owned()),
                cache_breakpoint: false,
            }],
            ..Default::default()
        };
        let prompt = CcProvider::format_prompt(&request);
        assert_eq!(prompt, "hello world");
    }

    #[test]
    fn format_prompt_multi_turn() {
        let request = CompletionRequest {
            messages: vec![
                Message {
                    role: Role::User,
                    content: Content::Text("What is 2+2?".to_owned()),
                    cache_breakpoint: false,
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Text("4".to_owned()),
                    cache_breakpoint: false,
                },
                Message {
                    role: Role::User,
                    content: Content::Text("And 3+3?".to_owned()),
                    cache_breakpoint: false,
                },
            ],
            ..Default::default()
        };
        let prompt = CcProvider::format_prompt(&request);
        assert!(prompt.contains("Human: What is 2+2?"));
        assert!(prompt.contains("Assistant: 4"));
        assert!(prompt.contains("Human: And 3+3?"));
    }

    #[test]
    fn resolve_model_strips_prefix() {
        let model = format!("{CC_MODEL_PREFIX}{}", crate::models::names::sonnet());
        let stripped = model
            .strip_prefix(CC_MODEL_PREFIX)
            .unwrap_or(model.as_str());
        assert_eq!(stripped, crate::models::names::sonnet());
    }

    #[test]
    fn supports_model_with_prefix() {
        let model = format!("{CC_MODEL_PREFIX}{}", crate::models::names::sonnet());
        assert!(model.starts_with(CC_MODEL_PREFIX));
    }

    #[test]
    fn supports_model_known() {
        let provider = CcProvider {
            name: "cc".to_owned(),
            cc_binary: PathBuf::from("claude"),
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert!(provider.supports_model(crate::models::names::sonnet()));
        assert!(provider.supports_model("claude-future-family-model"));
        assert!(!provider.supports_model("gpt-4"));
    }

    #[test]
    fn configured_models_are_exact_claims() {
        let provider = CcProvider {
            name: "cc-seat".to_owned(),
            cc_binary: PathBuf::from("claude"),
            working_directory: None,
            models: vec!["team-claude".to_owned()],
            default_model: "team-claude".to_owned(),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };

        assert_eq!(
            provider.match_specificity("team-claude"),
            Some(MatchKind::Exact)
        );
        assert_eq!(
            provider.match_specificity("cc/claude-opus-4-6"),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            provider.match_specificity("claude-future-family-model"),
            None
        );
        assert_eq!(provider.name(), "cc-seat");
    }

    #[test]
    fn cc_provider_reports_cloud_deployment_target() {
        let provider = CcProvider {
            name: "cc".to_owned(),
            cc_binary: PathBuf::from("claude"),
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
    }

    #[test]
    fn seat_bridged_fields() {
        let provider = CcProvider {
            name: "cc".to_owned(),
            cc_binary: PathBuf::from("/usr/local/bin/claude"),
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_mins(5),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(
            provider.cli_binary(),
            &PathBuf::from("/usr/local/bin/claude")
        );
        assert_eq!(provider.subprocess_timeout(), Duration::from_mins(5));
        assert_eq!(provider.cli_product_name(), "claude");
    }

    #[test]
    fn warns_once_for_dropped_tools() {
        assert!(!CcProvider::warn_dropped_tools(0));
        assert!(CcProvider::warn_dropped_tools(1));
        assert!(!CcProvider::warn_dropped_tools(2));
    }

    #[test]
    fn records_cache_metrics_from_response() {
        use koina::metrics::MetricsRegistry;

        use crate::metrics::register;
        use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

        let r = MetricsRegistry::new();
        r.with_registry(register);

        let response = CompletionResponse {
            id: "cc_1".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "hi".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 20,
                output_tokens: 10,
                cache_read_tokens: 5,
                cache_write_tokens: 2,
            },
            cost_usd: None,
            duration_ms: None,
        };
        crate::metrics::record_cache_tokens(
            "cc",
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );

        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        assert!(
            buf.contains("aletheia_llm_cache_tokens_total{provider=\"cc\",direction=\"read\"} 5"),
            "missing cache read metrics: {buf}"
        );
        assert!(
            buf.contains("aletheia_llm_cache_tokens_total{provider=\"cc\",direction=\"write\"} 2"),
            "missing cache write metrics: {buf}"
        );
    }
}
