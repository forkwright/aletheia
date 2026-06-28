//! `CcProvider`: routes LLM calls through the Claude Code CLI subprocess.
//!
//! CC handles OAuth authentication and attestation correctly, bypassing
//! the server-side blocking of direct API calls from non-CC clients.
//!
//! # Errors
//!
//! Runtime spawn failures, subprocess exits, and timeouts produce
//! [`Error::SubprocessFailure`](crate::error::Error::SubprocessFailure).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info, warn};

use crate::anthropic::StreamEvent;
use crate::circuit_breaker::CircuitBreaker;
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
    circuit_breaker: CircuitBreaker,
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
            circuit_breaker: CircuitBreaker::with_defaults(config.name.clone()),
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

    fn check_circuit_breaker(&self) -> Result<()> {
        if self.circuit_breaker.is_allowed() {
            return Ok(());
        }

        Err(error::ApiRequestSnafu {
            message: format!(
                "provider circuit-breaker open: {:?}",
                self.circuit_breaker.state()
            ),
        }
        .build())
    }

    fn record_subprocess_error(&self, err: &error::Error) {
        if err.is_retryable() {
            self.circuit_breaker.on_failure();
        }
    }

    /// Execute a non-streaming completion via CC subprocess.
    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let retry_policy = crate::retry::subprocess_retry_policy();
        let mut last_error = None;

        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                warn!(
                    provider = %self.name,
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying CC subprocess completion after transient error"
                );
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }

            self.check_circuit_breaker()?;
            match self.execute_once(request).await {
                Ok(response) => {
                    self.circuit_breaker.on_success();
                    return Ok(response);
                }
                Err(err) if err.is_retryable() && attempt < retry_policy.max_retries => {
                    self.record_subprocess_error(&err);
                    warn!(
                        provider = %self.name,
                        attempt,
                        error = %err,
                        "CC subprocess completion failed with retryable error"
                    );
                    last_error = Some(err);
                }
                Err(err) => {
                    self.record_subprocess_error(&err);
                    return Err(err);
                }
            }
        }

        Err(error::ApiRequestSnafu {
            message: "CC subprocess completion failed after retry loop".to_owned(),
        }
        .build())
    }

    async fn execute_once(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
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
        let retry_policy = crate::retry::subprocess_retry_policy();
        let mut last_error = None;

        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                warn!(
                    provider = %self.name,
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying CC streaming subprocess after transient error"
                );
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }

            self.check_circuit_breaker()?;
            match self.execute_streaming_once(request, on_event).await {
                Ok(response) => {
                    self.circuit_breaker.on_success();
                    return Ok(response);
                }
                Err(err) if err.is_retryable() && attempt < retry_policy.max_retries => {
                    self.record_subprocess_error(&err);
                    warn!(
                        provider = %self.name,
                        attempt,
                        error = %err,
                        "CC streaming subprocess failed with retryable error"
                    );
                    last_error = Some(err);
                }
                Err(err) => {
                    self.record_subprocess_error(&err);
                    return Err(err);
                }
            }
        }

        Err(error::ApiRequestSnafu {
            message: "CC streaming subprocess failed after retry loop".to_owned(),
        }
        .build())
    }

    async fn execute_streaming_once(
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
#[path = "provider_tests.rs"]
mod tests;
