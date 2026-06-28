//! `CodexProvider`: routes LLM calls through the Codex CLI subprocess.
//!
//! Codex handles OAuth authentication via its local CLI credential store.
//! The provider only resolves the binary, formats requests, spawns the
//! subprocess, and wraps plain-text output in Hermeneus response types.

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
            find_codex_binary()?
        };

        let working_directory = validate_working_directory(config.working_directory.as_deref())?;

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

    /// Format message history into a single prompt string for Codex.
    fn format_prompt(request: &CompletionRequest) -> String {
        if request.messages.len() == 1
            && let Some(msg) = request.messages.first()
        {
            return msg.content.text();
        }

        let mut parts = Vec::new();

        for msg in &request.messages {
            let label = match msg.role {
                Role::User => "User",
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
                provider = "codex",
                dropped_tools,
                "codex dropped {dropped_tools} tool definitions; this seat-bridged CLI runs its own agentic loop so aletheia's tools are not invoked. Use a native API provider for aletheia's tool-loop"
            );
        }

        warned
    }

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let retry_policy = crate::retry::subprocess_retry_policy();
        let mut last_error = None;

        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                warn!(
                    provider = %self.name,
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying Codex subprocess completion after transient error"
                );
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }

            match self.execute_once(request).await {
                Ok(response) => return Ok(response),
                Err(err) if err.is_retryable() && attempt < retry_policy.max_retries => {
                    warn!(
                        provider = %self.name,
                        attempt,
                        error = %err,
                        "Codex subprocess completion failed with retryable error"
                    );
                    last_error = Some(err);
                }
                Err(err) => return Err(err),
            }
        }

        Err(error::ApiRequestSnafu {
            message: "Codex subprocess completion failed after retry loop".to_owned(),
        }
        .build())
    }

    async fn execute_once(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
        let prompt = Self::format_prompt(request);

        let output = Box::pin(process::run_completion(
            &self.codex_binary,
            self.working_directory.as_deref(),
            request.system.as_deref(),
            &prompt,
            self.timeout,
        ))
        .await?;
        let parse::CodexParsedOutput { text, usage } = parse::parse_output(&output.stdout)?;
        let response = parse::text_to_response(&text, usage, model);
        // WHY(#4658): Codex reports cached_input_tokens as cache reads; emit
        // them so prompt-cache activity shows in provider metrics.
        crate::metrics::record_cache_tokens(
            self.name(),
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );
        Ok(response)
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
        let retry_policy = crate::retry::subprocess_retry_policy();
        let mut last_error = None;

        for attempt in 0..=retry_policy.max_retries {
            if attempt > 0 {
                warn!(
                    provider = %self.name,
                    attempt,
                    max = retry_policy.max_retries,
                    "retrying Codex streaming subprocess after transient error"
                );
                tokio::time::sleep(retry_policy.delay(attempt, last_error.as_ref())).await;
            }

            match self.execute_streaming_once(request, on_event).await {
                Ok(response) => return Ok(response),
                Err(err) if err.is_retryable() && attempt < retry_policy.max_retries => {
                    warn!(
                        provider = %self.name,
                        attempt,
                        error = %err,
                        "Codex streaming subprocess failed with retryable error"
                    );
                    last_error = Some(err);
                }
                Err(err) => return Err(err),
            }
        }

        Err(error::ApiRequestSnafu {
            message: "Codex streaming subprocess failed after retry loop".to_owned(),
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

        let output = Box::pin(process::run_completion(
            &self.codex_binary,
            self.working_directory.as_deref(),
            request.system.as_deref(),
            &prompt,
            self.timeout,
        ))
        .await?;
        let parse::CodexParsedOutput { text, usage } = parse::parse_output(&output.stdout)?;

        // WHY: Codex's CLI does not support line-by-line streaming; we emit the
        // full response as a single TextDelta so callers that consume
        // complete_streaming see consistent event-based output regardless of
        // which seat-bridged provider they're talking to.
        on_event(StreamEvent::TextDelta { text: text.clone() });

        let response = parse::text_to_response(&text, usage, model);
        // WHY(#4658): The streaming path uses the same CLI output as the
        // non-streaming path; record cache reads for observability parity.
        crate::metrics::record_cache_tokens(
            self.name(),
            response.usage.cache_read_tokens,
            response.usage.cache_write_tokens,
        );
        Ok(response)
    }
}

/// Render `content` as a flat text string suitable for Codex's plain-text stdin.
///
/// Tool-use blocks are serialized as `[Tool call: name(json_input)]` markers so
/// multi-turn conversations that include tool-call turns are not silently
/// truncated. Without this, `ContentBlock::ToolUse` falls through to the
/// wildcard arm and is dropped, causing the codex subprocess to receive a
/// conversation with entire assistant turns missing — the live correctness bug
/// fixed by #3980.
fn extract_text_content(content: &Content) -> String {
    match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } if !text.is_empty() => Some(text.to_owned()),
                    ContentBlock::ToolUse { name, input, .. } => {
                        // WHY(#3980): render tool-use calls textually so Codex sees
                        // the full conversation. Compact JSON keeps it readable while
                        // fitting in the flat prompt format.
                        Some(format!("[Tool call: {name}({input})]"))
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let summary = content.text_summary();
                        if summary.is_empty() {
                            None
                        } else {
                            Some(summary)
                        }
                    }
                    // Thinking, server tool use, web search have no meaningful
                    // text representation for Codex's flat prompt format.
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
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

fn validate_working_directory(path: Option<&Path>) -> Result<Option<PathBuf>> {
    match path {
        Some(path) if path.is_dir() => Ok(Some(path.to_path_buf())),
        Some(path) => Err(error::ProviderInitSnafu {
            message: format!(
                "configured codex working directory does not exist: {}",
                path.display()
            ),
        }
        .build()),
        None => Ok(None),
    }
}

fn find_codex_binary() -> Result<PathBuf> {
    let paths = RealSystem.var_os("PATH").unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<OsString>, not Result — empty PATH is a valid fallback
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("codex");
        if candidate.is_file() {
            debug!(path = %candidate.display(), "found codex binary in PATH");
            return Ok(candidate);
        }
    }

    if let Some(home) = RealSystem.var_os("HOME") {
        let home = PathBuf::from(home);
        for subdir in &[".local/bin/codex", ".codex/bin/codex"] {
            let candidate = home.join(subdir);
            if candidate.is_file() {
                info!(
                    path = %candidate.display(),
                    "found codex binary outside PATH (consider adding its directory to PATH)"
                );
                return Ok(candidate);
            }
        }
    }

    Err(error::ProviderInitSnafu {
        message: "codex CLI binary not found in PATH or ~/.local/bin. Install Codex CLI before enabling codex-provider"
            .to_owned(),
    }
    .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CompletionRequest, Content, ContentBlock, Message, Role, ToolResultContent,
    };

    fn retry_test_script_path(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hermeneus_codex_provider_{name}_{}_{nonce}.sh",
            std::process::id()
        ))
    }

    fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let script = format!("#!/bin/sh\n{body}\n");
        let mut file = std::fs::File::create(path)?;
        file.write_all(script.as_bytes())?;
        file.sync_all()?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }

    fn remove_if_exists(path: &Path) -> std::io::Result<()> {
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
    fn format_prompt_single_message() {
        let request = CompletionRequest {
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello world".to_owned()),
                cache_breakpoint: false,
            }],
            ..Default::default()
        };
        let prompt = CodexProvider::format_prompt(&request);
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
        let prompt = CodexProvider::format_prompt(&request);
        assert!(prompt.contains("User: What is 2+2?"));
        assert!(prompt.contains("Assistant: 4"));
        assert!(prompt.contains("User: And 3+3?"));
    }

    // WHY(#3980): ToolUse blocks must be rendered so assistant turns with tool
    // calls do not disappear from the Codex prompt.
    #[test]
    fn extract_text_content_renders_tool_use_blocks() {
        let content = Content::Blocks(vec![
            ContentBlock::Text {
                text: "Let me check that.".to_owned(),
                citations: None,
            },
            ContentBlock::ToolUse {
                id: "toolu_01".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::json!({"path": "/etc/hosts"}),
            },
        ]);
        let text = extract_text_content(&content);
        assert!(
            text.contains("Let me check that."),
            "text block must be present: {text}"
        );
        assert!(
            text.contains("[Tool call: read_file("),
            "tool-use block must be rendered, not dropped: {text}"
        );
        assert!(
            text.contains("/etc/hosts"),
            "tool input must appear in rendered marker: {text}"
        );
    }

    // WHY(#3980): tool-use assistant turns must round-trip through format_prompt
    // so a later tool-result still has matching call context.
    #[test]
    fn format_prompt_preserves_tool_use_turns() {
        let request = CompletionRequest {
            messages: vec![
                Message {
                    role: Role::User,
                    content: Content::Text("What is in /etc/hosts?".to_owned()),
                    cache_breakpoint: false,
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Blocks(vec![
                        ContentBlock::Text {
                            text: "I will read the file.".to_owned(),
                            citations: None,
                        },
                        ContentBlock::ToolUse {
                            id: "toolu_01".to_owned(),
                            name: "read_file".to_owned(),
                            input: serde_json::json!({"path": "/etc/hosts"}),
                        },
                    ]),
                    cache_breakpoint: false,
                },
                Message {
                    role: Role::User,
                    content: Content::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_01".to_owned(),
                        content: ToolResultContent::text("127.0.0.1 localhost"),
                        is_error: None,
                    }]),
                    cache_breakpoint: false,
                },
            ],
            ..Default::default()
        };
        let prompt = CodexProvider::format_prompt(&request);
        // All three turns must appear.
        assert!(
            prompt.contains("User: What is in /etc/hosts?"),
            "first user turn missing: {prompt}"
        );
        assert!(
            prompt.contains("I will read the file."),
            "assistant text missing: {prompt}"
        );
        assert!(
            prompt.contains("[Tool call: read_file("),
            "tool-use marker missing: {prompt}"
        );
        assert!(
            prompt.contains("127.0.0.1 localhost"),
            "tool result missing: {prompt}"
        );
    }

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

    #[tokio::test]
    async fn retries_fluke_spawn_failure_before_returning_error()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        const SCRIPT: &str = r#"cat > /dev/null
printf '{"type":"item.completed","item":{"type":"agent_message","text":"retried ok"}}\n'
printf '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}\n'"#;

        let script_path = retry_test_script_path("spawn_retry");
        write_executable_script(&script_path, SCRIPT)?;
        let provider = CodexProvider::new(&CodexProviderConfig {
            codex_binary: Some(script_path.clone()),
            timeout: Duration::from_secs(5),
            ..CodexProviderConfig::default()
        })?;
        remove_if_exists(&script_path)?;

        let restore_path = script_path.clone();
        let restore = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            write_executable_script(&restore_path, SCRIPT)
        });

        let request = one_message_request(format!(
            "{CODEX_MODEL_PREFIX}{}",
            koina::models::names::codex()
        ));
        let result = provider.execute(&request).await;
        restore.await??;
        let response = result?;

        assert_text_response(&response, "retried ok");
        remove_if_exists(&script_path)?;
        Ok(())
    }

    #[test]
    fn seat_bridged_fields() {
        let provider = CodexProvider {
            name: "codex".to_owned(),
            codex_binary: PathBuf::from("/usr/local/bin/codex"),
            working_directory: None,
            models: Vec::new(),
            default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
            timeout: Duration::from_secs(300),
            deployment_target: DeploymentTarget::Cloud,
        };
        assert_eq!(
            provider.cli_binary(),
            &PathBuf::from("/usr/local/bin/codex")
        );
        assert_eq!(provider.subprocess_timeout(), Duration::from_secs(300));
        assert_eq!(provider.cli_product_name(), "codex");
    }

    #[test]
    fn warns_once_for_dropped_tools() {
        assert!(!CodexProvider::warn_dropped_tools(0));
        assert!(CodexProvider::warn_dropped_tools(1));
        assert!(!CodexProvider::warn_dropped_tools(2));
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
}
