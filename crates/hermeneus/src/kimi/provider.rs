//! `KimiProvider`: routes LLM calls through the Kimi CLI subprocess.
//!
//! Kimi handles OAuth authentication through its local CLI credential store.
//! The provider only formats prompts, spawns the CLI, and parses its output.
//!
//! # Errors
//!
//! Spawn failures produce [`Error::ProviderInit`](crate::error::Error::ProviderInit);
//! subprocess errors and timeouts produce
//! [`Error::ApiRequest`](crate::error::Error::ApiRequest).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Duration, Instant};

use koina::system::{Environment, RealSystem};
use tracing::{debug, info, warn};

use crate::RetryPolicy;
use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::{LlmProvider, MatchKind};
use crate::types::{CompletionRequest, CompletionResponse, Content, ContentBlock, Role};

use super::parse;
use super::process;

/// Model name prefix that routes requests to this provider.
pub(crate) const KIMI_MODEL_PREFIX: &str = "kimi/";

/// Configuration for the Kimi subprocess provider.
#[derive(Debug, Clone)]
pub struct KimiProviderConfig {
    /// Path to the `kimi` binary. If `None`, resolved from `PATH`.
    pub kimi_binary: Option<PathBuf>,
    /// Working directory passed to `kimi -w`.
    pub working_directory: Option<PathBuf>,
    /// Default model when the request does not specify one.
    pub default_model: String,
    /// Subprocess timeout (wall-clock).
    pub timeout: Duration,
}

impl Default for KimiProviderConfig {
    fn default() -> Self {
        Self {
            kimi_binary: None,
            working_directory: None,
            default_model: koina::models::names::kimi().to_owned(),
            timeout: Duration::from_mins(5),
        }
    }
}

/// Kimi subprocess LLM provider.
///
/// Delegates completions to the `kimi` CLI binary via
/// `--print --afk --yolo --thinking`.
pub struct KimiProvider {
    kimi_binary: PathBuf,
    working_directory: PathBuf,
    default_model: String,
    timeout: Duration,
}

impl KimiProvider {
    /// Create a new Kimi provider, locating the `kimi` binary and worktree.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ProviderInit`](crate::error::Error::ProviderInit) if
    /// the binary or working directory cannot be resolved.
    pub fn new(config: &KimiProviderConfig) -> Result<Self> {
        let kimi_binary = if let Some(ref path) = config.kimi_binary {
            if path.exists() {
                path.clone()
            } else {
                return Err(error::ProviderInitSnafu {
                    message: format!(
                        "configured kimi CLI path does not exist: {}",
                        path.display()
                    ),
                }
                .build());
            }
        } else {
            find_kimi_binary()?
        };

        let working_directory = if let Some(ref path) = config.working_directory {
            if path.is_dir() {
                path.clone()
            } else {
                return Err(error::ProviderInitSnafu {
                    message: format!(
                        "configured kimi working directory does not exist: {}",
                        path.display()
                    ),
                }
                .build());
            }
        } else {
            std::env::current_dir().map_err(|e| {
                error::ProviderInitSnafu {
                    message: format!("failed to resolve current directory for kimi: {e}"),
                }
                .build()
            })?
        };

        info!(
            binary = %kimi_binary.display(),
            cwd = %working_directory.display(),
            default_model = %config.default_model,
            timeout_secs = config.timeout.as_secs(),
            "Kimi subprocess provider initialized"
        );

        Ok(Self {
            kimi_binary,
            working_directory,
            default_model: config.default_model.clone(),
            timeout: config.timeout,
        })
    }

    /// Resolve the model name, falling back to the configured default.
    fn resolve_model<'a>(&'a self, model: &'a str) -> &'a str {
        let selected = if model.is_empty() {
            &self.default_model
        } else {
            model
        };
        let stripped = selected.strip_prefix(KIMI_MODEL_PREFIX).unwrap_or(selected);
        if stripped.is_empty() {
            koina::models::names::kimi()
        } else {
            stripped
        }
    }

    /// Format message history into a single prompt string for Kimi.
    fn format_prompt(request: &CompletionRequest) -> String {
        if request.messages.len() == 1
            && let Some(msg) = request.messages.first()
        {
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

    /// Run the Kimi subprocess for a non-streaming completion, retrying the
    /// subprocess itself (not just the surrounding call) on a transient
    /// spawn or timeout failure.
    ///
    /// WHY(#5763): a single-shot spawn race (binary mid-update) or a
    /// transient OS resource exhaustion previously propagated to the caller
    /// immediately with no self-healing, unlike `AnthropicProvider`'s HTTP
    /// retry loop and `CcProvider`'s subprocess retry. Safe to retry
    /// unconditionally on this path: no output has reached the caller
    /// before `run_completion` resolves.
    async fn run_completion_with_retry(
        &self,
        process_config: &process::KimiProcessConfig<'_>,
        system: Option<&str>,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<process::KimiOutput> {
        let retry_policy = RetryPolicy::default();
        let mut last_error = None;
        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }
            match process::run_completion(process_config, system, prompt, max_tokens).await {
                Ok(output) => return Ok(output),
                Err(err) => {
                    if attempt == retry_policy.max_retries || !err.is_retryable() {
                        return Err(err);
                    }
                    warn!(
                        provider = "kimi",
                        attempt,
                        error = %err,
                        "Kimi subprocess call failed; retrying"
                    );
                    last_error = Some(err);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "Kimi subprocess retry loop exhausted with no recorded error".to_owned(),
            }
            .build()
        }))
    }

    /// Run the Kimi subprocess for a streaming completion, retrying a
    /// transient spawn or timeout failure that occurs before any content
    /// delta has reached the caller.
    ///
    /// WHY(#5763, matching `CcProvider::run_streaming_with_retry`): once
    /// `on_event` has received a delta, the caller has partial output;
    /// retrying at that point would duplicate it, so `content_started`
    /// latches permanently once true.
    async fn run_streaming_with_retry(
        &self,
        process_config: &process::KimiProcessConfig<'_>,
        system: Option<&str>,
        prompt: &str,
        max_tokens: u32,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<process::KimiOutput> {
        let retry_policy = RetryPolicy::default();
        let mut last_error = None;
        let mut content_started = false;
        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }
            let mut on_delta = |text: &str| {
                content_started = true;
                on_event(StreamEvent::TextDelta {
                    text: text.to_owned(),
                });
            };
            match process::run_streaming(process_config, system, prompt, max_tokens, &mut on_delta)
                .await
            {
                Ok(output) => return Ok(output),
                Err(err) => {
                    if content_started {
                        warn!(
                            provider = "kimi",
                            error = %err,
                            "Kimi subprocess streaming failed after content started; cannot retry"
                        );
                        return Err(err);
                    }
                    if attempt == retry_policy.max_retries || !err.is_retryable() {
                        return Err(err);
                    }
                    warn!(
                        provider = "kimi",
                        attempt,
                        error = %err,
                        "Kimi subprocess streaming call failed before any content; retrying"
                    );
                    last_error = Some(err);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "Kimi subprocess streaming retry loop exhausted with no recorded error"
                    .to_owned(),
            }
            .build()
        }))
    }

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(self.name(), "kimi", request.tools.len())?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = Self::format_prompt(request);
        let system = request.system.as_deref();
        let process_config = process::KimiProcessConfig {
            kimi_binary: &self.kimi_binary,
            cwd: &self.working_directory,
            // WHY(#4880): pass the resolved model explicitly so the subprocess
            // CLI uses the same model that response attribution records.
            model: Some(model),
            timeout: self.timeout,
        };

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_completion_with_retry(&process_config, system, &prompt, request.max_tokens)
                .await?;

            let response = parse::result_to_response(
                &output.result_text,
                output.usage,
                model,
                output.message_id.as_deref(),
            )?;
            // WHY(#4658): Kimi reports input_cache_read and input_cache_creation;
            // emit them so prompt-cache usage is visible in provider metrics.
            crate::metrics::record_cache_tokens(
                self.name(),
                response.usage.cache_read_tokens,
                response.usage.cache_write_tokens,
            );
            Ok(response)
        }
        .await;

        match &outcome {
            Ok(response) => {
                crate::metrics::record_completion(
                    self.name(),
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                    response.cost_usd.unwrap_or(0.0),
                    true,
                );
                crate::metrics::record_latency(model, "ok", start.elapsed().as_secs_f64());
            }
            Err(e) => {
                let status = if e.is_retryable() {
                    "rate_limited"
                } else {
                    "error"
                };
                crate::metrics::record_completion(self.name(), 0, 0, 0.0, false);
                crate::metrics::record_latency(model, status, start.elapsed().as_secs_f64());
            }
        }
        outcome
    }

    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        crate::seat_bridged::reject_tool_bearing_request(self.name(), "kimi", request.tools.len())?;
        let start = Instant::now();
        let model = self.resolve_model(&request.model);
        let prompt = Self::format_prompt(request);
        let system = request.system.as_deref();
        let process_config = process::KimiProcessConfig {
            kimi_binary: &self.kimi_binary,
            cwd: &self.working_directory,
            // WHY(#4880): pass the resolved model so the streaming subprocess
            // uses the same model recorded in response attribution.
            model: Some(model),
            timeout: self.timeout,
        };

        let outcome: Result<CompletionResponse> = async {
            let output = self
                .run_streaming_with_retry(
                    &process_config,
                    system,
                    &prompt,
                    request.max_tokens,
                    on_event,
                )
                .await?;

            let response = parse::result_to_response(
                &output.result_text,
                output.usage,
                model,
                output.message_id.as_deref(),
            )?;
            // WHY(#4658): Streaming Kimi output preserves cache tokens; emit them
            // for metrics parity with the non-streaming path.
            crate::metrics::record_cache_tokens(
                self.name(),
                response.usage.cache_read_tokens,
                response.usage.cache_write_tokens,
            );
            Ok(response)
        }
        .await;

        match &outcome {
            Ok(response) => {
                crate::metrics::record_completion(
                    self.name(),
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                    response.cost_usd.unwrap_or(0.0),
                    true,
                );
                crate::metrics::record_latency(model, "ok", start.elapsed().as_secs_f64());
            }
            Err(e) => {
                let status = if e.is_retryable() {
                    "rate_limited"
                } else {
                    "error"
                };
                crate::metrics::record_completion(self.name(), 0, 0, 0.0, false);
                crate::metrics::record_latency(model, status, start.elapsed().as_secs_f64());
            }
        }
        outcome
    }
}

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
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
    }
}

impl std::fmt::Debug for KimiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimiProvider")
            .field("kimi_binary", &self.kimi_binary)
            .field("working_directory", &self.working_directory)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout.as_secs())
            .finish_non_exhaustive()
    }
}

impl LlmProvider for KimiProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute(request))
    }

    fn supported_models(&self) -> &[&str] {
        koina::models::provider_models(koina::models::ModelProvider::Kimi)
    }

    fn supports_model(&self, model: &str) -> bool {
        self.match_specificity(model).is_some()
    }

    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        if self.supported_models().contains(&model) {
            Some(MatchKind::Exact)
        } else if model.starts_with(KIMI_MODEL_PREFIX) {
            Some(MatchKind::Prefix)
        } else {
            None
        }
    }

    fn name(&self) -> &'static str {
        "kimi"
    }

    fn capabilities(&self) -> crate::provider::ProviderCapabilities {
        // WHY(#5253, #4510): Kimi delegates to the `kimi` CLI's own agentic
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

/// Find the `kimi` binary in `PATH`.
fn find_kimi_binary() -> Result<PathBuf> {
    let paths = RealSystem.var_os("PATH").unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<OsString>, not Result — empty PATH is a valid fallback
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("kimi");
        if candidate.is_file() {
            debug!(path = %candidate.display(), "found kimi binary in PATH");
            return Ok(candidate);
        }
    }

    if let Some(home) = RealSystem.var_os("HOME") {
        let home = PathBuf::from(home);
        for subdir in &[".local/bin/kimi", ".cargo/bin/kimi"] {
            let candidate = home.join(subdir);
            if candidate.is_file() {
                tracing::info!(
                    path = %candidate.display(),
                    "found kimi binary outside PATH (consider adding its directory to PATH)"
                );
                return Ok(candidate);
            }
        }
    }

    Err(error::ProviderInitSnafu {
        message: "kimi CLI binary not found in PATH or ~/.local/bin. Install kimi-cli with `uv tool install kimi-cli`"
            .to_owned(),
    }
    .build())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::types::ToolDefinition;

    #[test]
    fn match_specificity_prefers_prefix_and_exact() {
        let provider = KimiProvider {
            kimi_binary: PathBuf::from("kimi"),
            working_directory: PathBuf::from("."),
            default_model: koina::models::names::kimi().to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            provider.match_specificity("kimi/experimental"),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            provider.match_specificity(koina::models::names::kimi()),
            Some(MatchKind::Exact)
        );
        assert_eq!(provider.match_specificity("claude-sonnet-4-6"), None);
    }

    #[test]
    fn kimi_provider_declares_no_tool_loop_capability() {
        // WHY(#5253): routing/fallback consult this so a tool-bearing turn
        // is never selected for kimi in the first place — the #4510
        // `reject_tool_bearing_request` hard-fail is the backstop for
        // anything that slips through, not the primary mechanism.
        let provider = KimiProvider {
            kimi_binary: PathBuf::from("kimi"),
            working_directory: PathBuf::from("."),
            default_model: koina::models::names::kimi().to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert!(!provider.capabilities().tool_loop);
    }

    #[test]
    fn records_cache_metrics_from_response() {
        use koina::metrics::MetricsRegistry;

        use crate::metrics::register;
        use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

        let r = MetricsRegistry::new();
        r.with_registry(register);

        let response = CompletionResponse {
            id: "kimi_1".to_owned(),
            model: "kimi".to_owned(),
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
            "kimi",
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );

        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        assert!(
            buf.contains("aletheia_llm_cache_tokens_total{provider=\"kimi\",direction=\"read\"} 5"),
            "missing cache read metrics: {buf}"
        );
        assert!(
            buf.contains(
                "aletheia_llm_cache_tokens_total{provider=\"kimi\",direction=\"write\"} 2"
            ),
            "missing cache write metrics: {buf}"
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
            "hermeneus_kimi_retry_test_{name}_{}_{nonce}.sh",
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

    fn flaky_provider(kimi_binary: PathBuf) -> KimiProvider {
        KimiProvider {
            kimi_binary,
            working_directory: std::env::temp_dir(),
            default_model: koina::models::names::kimi().to_owned(),
            timeout: Duration::from_secs(10),
        }
    }

    fn single_message_request() -> CompletionRequest {
        use crate::types::Message;

        CompletionRequest {
            model: koina::models::names::kimi().to_owned(),
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
            "hermeneus_kimi_retry_marker_{}_{}",
            std::process::id(),
            koina::uuid::uuid_v4()
        ));
        let _ = std::fs::remove_file(&marker);
        let script = write_flaky_script(
            "recovers",
            &format!(
                "cat > /dev/null\nif [ ! -f '{m}' ]; then\n  touch '{m}'\n  exit 1\nfi\nprintf '{{\"role\":\"assistant\",\"content\":\"ok after retry\"}}\\n'",
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
        let provider = flaky_provider(PathBuf::from("/nonexistent/kimi-must-not-be-spawned"));
        let request = tool_bearing_request();

        match provider.execute(&request).await {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "kimi"),
            Err(other) => panic!("expected Error::CapabilityMismatch, got: {other}"),
        }
    }

    #[tokio::test]
    async fn execute_streaming_hard_fails_on_tool_bearing_request() {
        // WHY(#4510): the streaming path must reject a tool-bearing request
        // before the subprocess is spawned, same as the non-streaming path.
        let provider = flaky_provider(PathBuf::from("/nonexistent/kimi-must-not-be-spawned"));
        let request = tool_bearing_request();
        let mut events = Vec::new();

        match provider
            .execute_streaming(&request, &mut |event| events.push(event))
            .await
        {
            Ok(response) => {
                panic!("expected a capability-mismatch hard failure, got: {response:?}")
            }
            Err(Error::CapabilityMismatch { provider, .. }) => assert_eq!(provider, "kimi"),
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
        let provider = flaky_provider(PathBuf::from("/nonexistent/kimi-must-not-be-spawned"));
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
