//! `KimiProvider`: routes LLM calls through the Kimi CLI subprocess.
//!
//! Kimi handles OAuth authentication through its local CLI credential store.
//! The provider only formats prompts, spawns the CLI, and parses its output.
//!
//! # Errors
//!
//! Runtime spawn failures, subprocess exits, and timeouts produce
//! [`Error::SubprocessFailure`](crate::error::Error::SubprocessFailure).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info, warn};

use crate::anthropic::StreamEvent;
use crate::circuit_breaker::CircuitBreaker;
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
    circuit_breaker: CircuitBreaker,
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
            circuit_breaker: CircuitBreaker::with_defaults("kimi"),
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
                provider = "kimi",
                dropped_tools,
                "kimi dropped {dropped_tools} tool definitions; this seat-bridged CLI runs its own agentic loop so aletheia's tools are not invoked. Use a native API provider for aletheia's tool-loop"
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

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let retry_policy = crate::retry::subprocess_retry_policy();
        let mut last_error = None;

        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                warn!(
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying Kimi subprocess completion after transient error"
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
                        attempt,
                        error = %err,
                        "Kimi subprocess completion failed with retryable error"
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
            message: "Kimi subprocess completion failed after retry loop".to_owned(),
        }
        .build())
    }

    async fn execute_once(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
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

        let output =
            process::run_completion(&process_config, system, &prompt, request.max_tokens).await?;

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
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying Kimi streaming subprocess after transient error"
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
                        attempt,
                        error = %err,
                        "Kimi streaming subprocess failed with retryable error"
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
            message: "Kimi streaming subprocess failed after retry loop".to_owned(),
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
        let process_config = process::KimiProcessConfig {
            kimi_binary: &self.kimi_binary,
            cwd: &self.working_directory,
            // WHY(#4880): pass the resolved model so the streaming subprocess
            // uses the same model recorded in response attribution.
            model: Some(model),
            timeout: self.timeout,
        };

        let mut on_delta = |text: &str| {
            on_event(StreamEvent::TextDelta {
                text: text.to_owned(),
            });
        };

        let output = process::run_streaming(
            &process_config,
            system,
            &prompt,
            request.max_tokens,
            &mut on_delta,
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
    use crate::types::{CompletionRequest, Content, Message, Role};

    fn retry_test_script_path(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hermeneus_kimi_provider_{name}_{}_{nonce}.sh",
            std::process::id()
        ))
    }

    fn write_executable_script(path: &std::path::Path, body: &str) -> std::io::Result<()> {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let script = format!("#!/bin/sh\n{body}\n");
        let mut file = std::fs::File::create(path)?;
        file.write_all(script.as_bytes())?;
        file.sync_all()?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }

    fn remove_if_exists(path: &std::path::Path) -> std::io::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn one_message_request(model: String) -> CompletionRequest {
        CompletionRequest {
            model,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
                cache_breakpoint: false,
            }],
            ..Default::default()
        }
    }

    fn assert_text_response(response: &CompletionResponse, expected: &str) {
        assert!(
            response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::Text { text, .. } if text == expected)),
            "response should contain text {expected:?}: {response:?}"
        );
    }

    #[test]
    fn warns_once_for_dropped_tools() {
        assert!(!KimiProvider::warn_dropped_tools(0));
        assert!(KimiProvider::warn_dropped_tools(1));
        assert!(!KimiProvider::warn_dropped_tools(2));
    }

    #[test]
    fn match_specificity_prefers_prefix_and_exact() {
        let provider = KimiProvider {
            kimi_binary: PathBuf::from("kimi"),
            working_directory: PathBuf::from("."),
            default_model: koina::models::names::kimi().to_owned(),
            timeout: Duration::from_secs(1),
            circuit_breaker: CircuitBreaker::with_defaults("kimi"),
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

    #[tokio::test]
    async fn retries_fluke_spawn_failure_before_returning_error() {
        const SCRIPT: &str = r#"cat > /dev/null
printf '{"role":"assistant","content":[{"type":"text","text":"retried ok"}]}\n'"#;

        let script_path = retry_test_script_path("spawn_retry");
        write_executable_script(&script_path, SCRIPT).expect("write script");
        let provider = KimiProvider::new(&KimiProviderConfig {
            kimi_binary: Some(script_path.clone()),
            timeout: Duration::from_secs(5),
            ..KimiProviderConfig::default()
        })
        .expect("provider init");
        remove_if_exists(&script_path).expect("remove script");

        let restore_path = script_path.clone();
        let restore = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await; // kanon:ignore TESTING/sleep-in-test WHY: tests real subprocess retry timing; cannot use deterministic mock
            write_executable_script(&restore_path, SCRIPT)
        });

        let request = one_message_request(format!(
            "{KIMI_MODEL_PREFIX}{}",
            koina::models::names::kimi()
        ));
        let result = provider.execute(&request).await;
        restore
            .await
            .expect("restore task join")
            .expect("restore script write");
        let response = result.expect("provider execute");

        assert_text_response(&response, "retried ok");
        remove_if_exists(&script_path).expect("cleanup");
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
}
