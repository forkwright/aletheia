//! `CodexProvider`: routes LLM calls through the Codex CLI subprocess.
//!
//! Codex handles OAuth authentication via its local CLI credential store.
//! The provider only resolves the binary, formats requests, spawns the
//! subprocess, and wraps plain-text output in Hermeneus response types.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tracing::info;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind};
use crate::seat_bridged::SeatBridgedProvider;
use crate::subprocess_provider;
use crate::types::{CompletionRequest, CompletionResponse};

use super::{parse, process};

/// Model name prefix that routes requests to this provider.
pub(crate) const CODEX_MODEL_PREFIX: &str = "codex/";

/// Configuration for the Codex subprocess provider.
#[derive(Debug, Clone)]
pub struct CodexProviderConfig {
    /// Provider instance name used for routing diagnostics and metrics.
    pub name: String,
    /// Path to the `codex` binary. If `None`, resolved from `PATH`.
    pub codex_binary: Option<PathBuf>,
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

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            name: "codex".to_owned(),
            codex_binary: None,
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_mins(5),
            deployment_target: DeploymentTarget::Cloud,
        }
    }
}

/// Codex subprocess LLM provider.
pub struct CodexProvider {
    // kanon:ignore RUST/pub-visibility
    name: String,
    codex_binary: PathBuf,
    working_directory: Option<PathBuf>,
    models: Vec<String>,
    default_model: String,
    timeout: Duration,
    deployment_target: DeploymentTarget,
}

impl CodexProvider {
    /// Create a new Codex provider, locating the `codex` binary.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ProviderInit`] if the binary cannot be found.
    pub fn new(config: &CodexProviderConfig) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let codex_binary = if let Some(ref path) = config.codex_binary {
            if path.exists() {
                path.clone()
            } else {
                return Err(error::ProviderInitSnafu {
                    message: format!(
                        "configured codex CLI path does not exist: {}",
                        path.display()
                    ),
                }
                .build());
            }
        } else {
            subprocess_provider::find_binary(
                "codex",
                &[".local/bin/codex", ".codex/bin/codex"],
                "codex CLI binary not found in PATH or ~/.local/bin. Install Codex CLI before enabling codex-provider",
            )?
        };

        let working_directory = subprocess_provider::validate_working_directory(
            config.working_directory.as_deref(),
            "codex",
        )?;

        info!(
            provider = %config.name,
            binary = %codex_binary.display(),
            cwd = ?working_directory.as_ref().map(|path| path.display().to_string()),
            models = ?config.models,
            default_model = %config.default_model,
            timeout_secs = config.timeout.as_secs(),
            "Codex subprocess provider initialized"
        );

        Ok(Self {
            name: config.name.clone(),
            codex_binary,
            working_directory,
            models: config.models.clone(),
            default_model: config.default_model.clone(),
            timeout: config.timeout,
            deployment_target: config.deployment_target,
        })
    }

    /// Resolve the model: strip `codex/` prefix, fall back to default.
    fn resolve_model<'a>(&'a self, model: &'a str) -> &'a str {
        let selected = if model.is_empty() {
            &self.default_model
        } else {
            model
        };
        let stripped = selected
            .strip_prefix(CODEX_MODEL_PREFIX)
            .unwrap_or(selected);
        if stripped.is_empty() {
            koina::models::names::codex()
        } else {
            stripped
        }
    }

    /// Run the Codex subprocess for a completion, retrying the subprocess
    /// call itself (not just the surrounding call) on a transient spawn or
    /// timeout failure.
    ///
    /// WHY(#5763): mirrors `CcProvider::run_completion_with_retry` — a
    /// single-shot spawn race (binary mid-update) or a transient OS
    /// resource exhaustion previously propagated to the caller immediately
    /// with no self-healing. Safe to retry unconditionally: Codex has no
    /// real per-line streaming (see `execute_streaming`'s doc comment), so
    /// no partial output can have reached the caller before this resolves —
    /// unlike `CcProvider`/`KimiProvider`, one retry helper covers both
    /// `execute` and `execute_streaming`.
    async fn run_completion_with_retry(
        &self,
        system_prompt: Option<&str>,
        prompt: &str,
    ) -> Result<process::CodexOutput> {
        subprocess_provider::run_with_retry(
            self.name(),
            "Codex subprocess",
            || {
                Box::pin(process::run_completion(
                    &self.codex_binary,
                    self.working_directory.as_deref(),
                    system_prompt,
                    prompt,
                    self.timeout,
                ))
            },
            |_| false,
        )
        .await
    }

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(&self.name, "codex", request.tools.len())?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = subprocess_provider::format_prompt(request);

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_completion_with_retry(request.system.as_deref(), &prompt)
                .await?;
            let parse::CodexParsedOutput { text, usage } = parse::parse_output(&output.stdout)?;
            let response = parse::text_to_response(&text, usage, model);
            Ok(response)
        }
        .await;

        subprocess_provider::record_completion_metrics(self.name(), model, start, &outcome);
        outcome
    }

    /// Execute a streaming completion, emitting `StreamEvent::TextDelta` for each
    /// output line.
    ///
    /// Codex emits plain text, not JSON-event streams, so "streaming" here means
    /// collecting the full output and emitting a single `TextDelta` event — which
    /// is functionally equivalent and avoids the caller having to special-case
    /// non-streaming codex responses.
    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(&self.name, "codex", request.tools.len())?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = subprocess_provider::format_prompt(request);

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_completion_with_retry(request.system.as_deref(), &prompt)
                .await?;
            let parse::CodexParsedOutput { text, usage } = parse::parse_output(&output.stdout)?;

            // WHY: Codex's CLI does not support line-by-line streaming; we emit the
            // full response as a single TextDelta so callers that consume
            // complete_streaming see consistent event-based output regardless of
            // which seat-bridged provider they're talking to.
            on_event(StreamEvent::TextDelta { text: text.clone() });

            let response = parse::text_to_response(&text, usage, model);
            Ok(response)
        }
        .await;

        subprocess_provider::record_completion_metrics(self.name(), model, start, &outcome);
        outcome
    }
}

impl std::fmt::Debug for CodexProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexProvider")
            .field("name", &self.name)
            .field("codex_binary", &self.codex_binary)
            .field("working_directory", &self.working_directory)
            .field("models", &self.models)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout.as_secs())
            .field("deployment_target", &self.deployment_target)
            .finish_non_exhaustive()
    }
}

impl LlmProvider for CodexProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute(request))
    }

    fn supported_models(&self) -> &[&str] {
        if self.models.is_empty() {
            koina::models::provider_models(koina::models::ModelProvider::Codex)
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
        } else if model.starts_with(CODEX_MODEL_PREFIX) {
            Some(MatchKind::Prefix)
        } else if self.models.is_empty() && self.supported_models().contains(&model) {
            Some(MatchKind::Exact)
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
        // WHY(#5253, #4510): Codex delegates to the `codex` CLI's own
        // agentic loop and cannot translate aletheia's tools into it — see
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

impl SeatBridgedProvider for CodexProvider {
    fn cli_binary(&self) -> &PathBuf {
        &self.codex_binary
    }

    fn subprocess_timeout(&self) -> Duration {
        self.timeout
    }

    fn cli_product_name(&self) -> &'static str {
        "codex"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::types::{CompletionRequest, Content, ContentBlock, Message, Role, ToolDefinition};

    #[test]
    fn resolve_model_strips_prefix() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(
            provider.resolve_model(&format!(
                "{CODEX_MODEL_PREFIX}{}",
                koina::models::names::codex()
            )),
            koina::models::names::codex()
        );
        assert_eq!(provider.resolve_model(""), koina::models::names::codex());
    }

    #[test]
    fn supports_model_with_prefix() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert!(provider.supports_model(&format!(
            "{CODEX_MODEL_PREFIX}{}",
            koina::models::names::codex()
        )));
        assert!(provider.supports_model(koina::models::names::codex()));
        assert!(!provider.supports_model("claude-sonnet-4-6"));
    }

    #[test]
    fn codex_provider_declares_no_tool_loop_capability() {
        // WHY(#5253): routing/fallback consult this so a tool-bearing turn
        // is never selected for codex in the first place — the #4510
        // `reject_tool_bearing_request` hard-fail is the backstop for
        // anything that slips through, not the primary mechanism.
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert!(!provider.capabilities().tool_loop);
    }

    #[test]
    fn match_specificity_prefers_prefix_and_exact() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(
            provider.match_specificity(&format!(
                "{CODEX_MODEL_PREFIX}{}",
                koina::models::names::codex()
            )),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            provider.match_specificity(koina::models::names::codex()),
            Some(MatchKind::Exact)
        );
        assert_eq!(provider.match_specificity("claude-sonnet-4-6"), None);
    }

    #[test]
    fn configured_models_are_exact_claims() {
        let provider = CodexProvider {
            name: "codex-seat".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: vec!["team-codex".to_owned()],
            default_model: "team-codex".to_owned(),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };

        assert_eq!(
            provider.match_specificity("team-codex"),
            Some(MatchKind::Exact)
        );
        assert_eq!(
            provider.match_specificity("codex/gpt-5-codex"),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            provider.match_specificity(koina::models::names::codex()),
            None
        );
        assert_eq!(provider.name(), "codex-seat");
    }

    #[test]
    fn codex_provider_reports_cloud_deployment_target() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
    }

    #[test]
    fn codex_provider_supports_streaming() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(1),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert!(
            provider.supports_streaming(),
            "CodexProvider must report supports_streaming=true after #3980"
        );
    }

    #[test]
    fn seat_bridged_fields() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("/usr/local/bin/codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_mins(5),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(
            provider.cli_binary(),
            &PathBuf::from("/usr/local/bin/codex")
        );
        assert_eq!(provider.subprocess_timeout(), Duration::from_mins(5));
        assert_eq!(provider.cli_product_name(), "codex");
    }

    #[test]
    fn records_cache_metrics_from_response() {
        use koina::metrics::MetricsRegistry;

        use crate::metrics::register;
        use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

        let r = MetricsRegistry::new();
        r.with_registry(register);

        let response = CompletionResponse {
            id: "codex_1".to_owned(),
            model: "codex".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "hi".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 20,
                output_tokens: 10,
                cache_read_tokens: 5,
                cache_write_tokens: 0,
            },
            cost_usd: None,
            duration_ms: None,
        };
        crate::metrics::record_cache_tokens(
            "codex",
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );

        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        assert!(
            buf.contains(
                "aletheia_llm_cache_tokens_total{provider=\"codex\",direction=\"read\"} 5"
            ),
            "missing cache read metrics: {buf}"
        );
        // WHY: Codex only reports cache reads; write direction must be absent.
        assert!(
            !buf.contains(
                "aletheia_llm_cache_tokens_total{provider=\"codex\",direction=\"write\"}"
            ),
            "codex must not emit zero cache write metrics: {buf}"
        );
    }

    /// Write an executable shell script and return its path.
    ///
    /// WHY: mirrors `cc::provider::tests::write_flaky_script` — write to a
    /// temp sibling and `sync_all` before renaming into place so a spawn
    /// immediately after this returns does not race a kernel `ETXTBSY`.
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    fn write_flaky_script(name: &str, body: &str) -> PathBuf {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let final_path = std::env::temp_dir().join(format!(
            "hermeneus_codex_retry_test_{name}_{}_{nonce}.sh",
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

    fn flaky_provider(codex_binary: PathBuf) -> CodexProvider {
        CodexProvider {
            name: "codex".to_owned(),
            codex_binary,
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(10),
            deployment_target: DeploymentTarget::Cloud,
        }
    }

    fn single_message_request() -> CompletionRequest {
        CompletionRequest {
            model: koina::models::names::codex().to_owned(),
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
            "hermeneus_codex_retry_marker_{}_{}",
            std::process::id(),
            koina::uuid::uuid_v4()
        ));
        let _ = std::fs::remove_file(&marker);
        let script = write_flaky_script(
            "recovers",
            &format!(
                "cat > /dev/null\nif [ ! -f '{m}' ]; then\n  touch '{m}'\n  exit 1\nfi\nprintf '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"ok after retry\"}}}}\\n'",
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
        let provider = flaky_provider(PathBuf::from("/nonexistent/codex-must-not-be-spawned"));
        let request = tool_bearing_request();

        match provider.execute(&request).await {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "codex"),
            Err(other) => panic!("expected Error::CapabilityMismatch, got: {other}"),
        }
    }

    #[tokio::test]
    async fn execute_streaming_hard_fails_on_tool_bearing_request() {
        // WHY(#4510): the streaming path must reject a tool-bearing request
        // before the subprocess is spawned, same as the non-streaming path.
        let provider = flaky_provider(PathBuf::from("/nonexistent/codex-must-not-be-spawned"));
        let request = tool_bearing_request();
        let mut events = Vec::new();

        match provider
            .execute_streaming(&request, &mut |event| events.push(event))
            .await
        {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "codex"),
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
        let provider = flaky_provider(PathBuf::from("/nonexistent/codex-must-not-be-spawned"));
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
