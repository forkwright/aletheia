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
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind};
use crate::seat_bridged::SeatBridgedProvider;
use crate::subprocess_provider;
use crate::types::{CompletionRequest, CompletionResponse};

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
            subprocess_provider::find_binary(
                "claude",
                &[".local/bin/claude", ".claude/bin/claude"],
                "claude CLI binary not found in PATH or ~/.local/bin. \
                 Install Claude Code: https://docs.anthropic.com/en/docs/claude-code",
            )?
        };

        let working_directory = subprocess_provider::validate_working_directory(
            config.working_directory.as_deref(),
            "claude",
        )?;

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

    /// Run the CC subprocess for a non-streaming completion, retrying once
    /// the subprocess itself (not just the surrounding call) on a transient
    /// spawn or timeout failure.
    ///
    /// WHY(#5763): a single-shot spawn race (binary mid-update) or a
    /// transient OS resource exhaustion previously propagated to the caller
    /// immediately with no self-healing, unlike `AnthropicProvider`'s HTTP
    /// retry loop. Safe to retry unconditionally on this path: no output has
    /// reached the caller before `run_completion` resolves.
    async fn run_completion_with_retry(
        &self,
        model: &str,
        system: Option<&str>,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<process::CcOutput> {
        subprocess_provider::run_with_retry(
            self.name(),
            "CC subprocess",
            || {
                process::run_completion(
                    &self.cc_binary,
                    self.working_directory.as_deref(),
                    model,
                    system,
                    prompt,
                    max_tokens,
                    self.timeout,
                )
            },
            |_| false,
        )
        .await
    }

    /// Run the CC subprocess for a streaming completion, retrying a
    /// transient spawn or timeout failure that occurs before any content
    /// delta has reached the caller.
    ///
    /// WHY(#5763, matching #4887): once `on_event` has received a delta, the
    /// caller has partial output; retrying at that point would duplicate it,
    /// so `content_started` latches permanently once true (mirrors
    /// `OpenAiProvider::execute_streaming_inner`).
    async fn run_streaming_with_retry(
        &self,
        model: &str,
        system: Option<&str>,
        prompt: &str,
        max_tokens: u32,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<process::CcOutput> {
        // WHY: AtomicBool rather than Cell -- the latch is shared between the
        // operation closure and the retry classifier, and this future must stay
        // Send. Cell<bool> is not Sync, so &Cell cannot be held across the await.
        let content_started = std::sync::atomic::AtomicBool::new(false);
        let mut on_delta = |text: &str| {
            content_started.store(true, std::sync::atomic::Ordering::Relaxed);
            on_event(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
        };
        subprocess_provider::run_with_retry(
            self.name(),
            "CC subprocess streaming",
            || {
                process::run_streaming(
                    &self.cc_binary,
                    self.working_directory.as_deref(),
                    model,
                    system,
                    prompt,
                    max_tokens,
                    self.timeout,
                    &mut on_delta,
                )
            },
            |err| {
                if content_started.load(std::sync::atomic::Ordering::Relaxed) {
                    warn!(
                        provider = %self.name,
                        error = %err,
                        "CC subprocess streaming failed after content started; cannot retry"
                    );
                    true
                } else {
                    false
                }
            },
        )
        .await
    }

    /// Execute a non-streaming completion via CC subprocess.
    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(
            &self.name,
            "claude",
            request.tools.len(),
        )?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = subprocess_provider::format_prompt(request);
        let system = request.system.as_deref();

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_completion_with_retry(model, system, &prompt, request.max_tokens)
                .await?;

            let response = parse::result_to_response(
                &output.result_text,
                output.is_error,
                output.usage.as_ref(),
                model,
                output.session_id.as_deref(),
            )?;
            Ok(response)
        }
        .await;

        subprocess_provider::record_completion_metrics(self.name(), model, start, &outcome);
        outcome
    }

    /// Execute a streaming completion, emitting `StreamEvent`s.
    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(
            &self.name,
            "claude",
            request.tools.len(),
        )?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = subprocess_provider::format_prompt(request);
        let system = request.system.as_deref();

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_streaming_with_retry(model, system, &prompt, request.max_tokens, on_event)
                .await?;

            let response = parse::result_to_response(
                &output.result_text,
                output.is_error,
                output.usage.as_ref(),
                model,
                output.session_id.as_deref(),
            )?;
            Ok(response)
        }
        .await;

        subprocess_provider::record_completion_metrics(self.name(), model, start, &outcome);
        outcome
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

    fn capabilities(&self) -> crate::provider::ProviderCapabilities {
        // WHY(#5253, #4510): CC delegates to the `claude` CLI's own agentic
        // loop and cannot translate aletheia's tools into it — see
        // `execute`'s `reject_tool_bearing_request` call, the backstop this
        // declaration lets routing avoid hitting in the normal path.
        crate::provider::ProviderCapabilities { tool_loop: false }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::types::{CompletionRequest, Content, ContentBlock, Message, Role, ToolDefinition};

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
    fn cc_provider_declares_no_tool_loop_capability() {
        // WHY(#5253): routing/fallback consult this so a tool-bearing turn
        // is never selected for cc in the first place — the #4510
        // `reject_tool_bearing_request` hard-fail is the backstop for
        // anything that slips through, not the primary mechanism.
        let provider = CcProvider {
            name: "cc".to_owned(),
            cc_binary: PathBuf::from("claude"),
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert!(!provider.capabilities().tool_loop);
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

    /// Write an executable shell script and return its path.
    ///
    /// WHY: write to a temp sibling and `sync_all` before renaming into
    /// place — the file's write descriptor is fully closed and flushed
    /// before the executable path exists, so a spawn immediately after this
    /// returns does not race a kernel `ETXTBSY`. Unlike the generic
    /// `write_script` helper in `cc::process_run_tests`, this one cannot
    /// also probe-spawn the file to double-check: the scripts under test
    /// here are stateful (a marker file toggles their behavior between
    /// invocations), and a throwaway probe spawn would itself count as the
    /// first invocation.
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    fn write_flaky_script(name: &str, body: &str) -> PathBuf {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let final_path = std::env::temp_dir().join(format!(
            "hermeneus_cc_retry_test_{name}_{}_{nonce}.sh",
            std::process::id()
        ));
        let tmp_path = final_path.with_extension("sh.tmp");
        let script = format!("#!/bin/sh\n{body}\n");
        {
            let mut f = std::fs::File::create(&tmp_path).unwrap_or_else(|e| {
                panic!("create {}: {e}", tmp_path.display());
            });
            f.write_all(script.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&tmp_path, &final_path).unwrap();
        final_path
    }

    fn flaky_provider(cc_binary: PathBuf) -> CcProvider {
        CcProvider {
            name: "cc".to_owned(),
            cc_binary,
            working_directory: None,
            models: Vec::new(),
            default_model: crate::models::names::opus().to_owned(),
            timeout: Duration::from_secs(10),
            deployment_target: DeploymentTarget::Cloud,
        }
    }

    fn single_message_request() -> CompletionRequest {
        CompletionRequest {
            model: crate::models::names::opus().to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
                cache_breakpoint: false,
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    async fn execute_recovers_from_a_single_transient_spawn_failure() {
        // WHY(#5763): before the retry loop, a single subprocess failure
        // (binary-update race, momentary OS resource exhaustion) propagated
        // to the caller immediately with no self-healing. This script fails
        // its first invocation and succeeds on the second (leaving a marker
        // file so it can tell the two apart); `execute` must recover instead
        // of returning the first failure.
        let marker = std::env::temp_dir().join(format!(
            "hermeneus_cc_retry_marker_{}_{}",
            std::process::id(),
            koina::uuid::uuid_v4()
        ));
        let _ = std::fs::remove_file(&marker);
        let script = write_flaky_script(
            "recovers",
            &format!(
                "cat > /dev/null\nif [ ! -f '{m}' ]; then\n  touch '{m}'\n  exit 1\nfi\nprintf '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok after retry\",\"is_error\":false}}\\n'",
                m = marker.display()
            ),
        );

        let provider = flaky_provider(script.clone());
        let request = single_message_request();

        let response = provider.execute(&request).await.unwrap();

        match response.content.first() {
            Some(ContentBlock::Text { text, .. }) => assert_eq!(text, "ok after retry"),
            other => panic!("expected a single text content block, got {other:?}"),
        }
        assert!(
            marker.exists(),
            "the flaky script's first invocation must have run and failed"
        );

        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&marker);
    }

    #[tokio::test]
    async fn execute_still_fails_once_the_retry_budget_is_exhausted() {
        // WHY(#5763): a persistently-failing subprocess must still surface an
        // error once retries are exhausted, not retry forever.
        let script = write_flaky_script("always_fails", "cat > /dev/null\nexit 1");
        let provider = flaky_provider(script.clone());
        let request = single_message_request();

        match provider.execute(&request).await {
            Ok(response) => panic!("expected a persistent failure, got: {response:?}"),
            Err(err) => assert!(
                err.is_retryable(),
                "a bare non-zero exit must classify as a retryable subprocess failure, got: {err}"
            ),
        }

        let _ = std::fs::remove_file(&script);
    }

    fn tool_bearing_request() -> CompletionRequest {
        let mut request = single_message_request();
        request.tools = vec![ToolDefinition {
            name: "read_file".to_owned(),
            description: "Read a file from disk".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }];
        request
    }

    #[tokio::test]
    async fn execute_hard_fails_on_tool_bearing_request() {
        // WHY(#4510): a tool-bearing turn must never reach the subprocess —
        // this provider's own agentic loop cannot run aletheia's tools. The
        // binary path here is deliberately nonexistent: if the capability
        // check were skipped, this test would fail with a spawn error
        // instead of the expected CapabilityMismatch, catching a regression
        // either way.
        let provider = flaky_provider(PathBuf::from("/nonexistent/claude-must-not-be-spawned"));
        let request = tool_bearing_request();

        match provider.execute(&request).await {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "cc"),
            Err(other) => panic!("expected Error::CapabilityMismatch, got: {other}"),
        }
    }

    #[tokio::test]
    async fn execute_streaming_hard_fails_on_tool_bearing_request() {
        // WHY(#4510): the streaming path must reject a tool-bearing request
        // before the subprocess is spawned, same as the non-streaming path.
        let provider = flaky_provider(PathBuf::from("/nonexistent/claude-must-not-be-spawned"));
        let request = tool_bearing_request();
        let mut events = Vec::new();

        match provider
            .execute_streaming(&request, &mut |event| events.push(event))
            .await
        {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "cc"),
            Err(other) => panic!("expected Error::CapabilityMismatch, got: {other}"),
        }
        assert!(
            events.is_empty(),
            "no stream events should be emitted before the capability check runs"
        );
    }

    #[tokio::test]
    async fn execute_without_tools_still_reaches_the_subprocess() {
        // WHY: pins the inverse of the two tests above — a tool-free request
        // must not be rejected by the new check, so the retry-budget test's
        // nonexistent-binary error (a spawn failure, not CapabilityMismatch)
        // still exercises the code path it did before #4510.
        let provider = flaky_provider(PathBuf::from("/nonexistent/claude-must-not-be-spawned"));
        let request = single_message_request();

        match provider.execute(&request).await {
            Ok(response) => panic!("expected a spawn failure, got: {response:?}"),
            Err(Error::CapabilityMismatch { .. }) => {
                panic!("a tool-free request must not raise CapabilityMismatch")
            }
            Err(_) => {}
        }
    }
}
