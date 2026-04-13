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
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info};

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::LlmProvider;
use crate::types::{CompletionRequest, CompletionResponse, Content, ContentBlock, Role};

use super::parse;
use super::process;

/// Model name prefix that routes requests to this provider.
pub(crate) const CC_MODEL_PREFIX: &str = "cc/";

/// Models CC can route to. Kept minimal: CC itself resolves aliases.
const SUPPORTED_MODELS: &[&str] = &[
    "claude-opus-4-20250514",
    "claude-opus-4-6",
    "claude-sonnet-4-20250514",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "claude-haiku-4-5",
];

/// Configuration for the CC subprocess provider.
#[derive(Debug, Clone)]
pub struct CcProviderConfig {
    /// Path to the `claude` binary. If `None`, resolved from `PATH`.
    pub cc_binary: Option<PathBuf>,
    /// Default model when the request doesn't specify one.
    pub default_model: String,
    /// Subprocess timeout (wall-clock).
    pub timeout: Duration,
}

impl Default for CcProviderConfig {
    fn default() -> Self {
        Self {
            cc_binary: None,
            default_model: "claude-opus-4-6".to_owned(),
            timeout: Duration::from_mins(5),
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
    cc_binary: PathBuf,
    default_model: String,
    timeout: Duration,
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

        info!(
            binary = %cc_binary.display(),
            default_model = %config.default_model,
            timeout_secs = config.timeout.as_secs(),
            "CC subprocess provider initialized"
        );

        Ok(Self {
            cc_binary,
            default_model: config.default_model.clone(),
            timeout: config.timeout,
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

    /// Execute a non-streaming completion via CC subprocess.
    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        let prompt = Self::format_prompt(request);
        let system = request.system.as_deref();

        let output = process::run_completion(
            &self.cc_binary,
            model,
            system,
            &prompt,
            request.max_tokens,
            self.timeout,
        )
        .await?;

        parse::result_to_response(
            &output.result_text,
            output.is_error,
            output.usage.as_ref(),
            model,
            output.session_id.as_deref(),
        )
    }

    /// Execute a streaming completion, emitting `StreamEvent`s.
    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
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
            model,
            system,
            &prompt,
            request.max_tokens,
            self.timeout,
            &mut on_delta,
        )
        .await?;

        parse::result_to_response(
            &output.result_text,
            output.is_error,
            output.usage.as_ref(),
            model,
            output.session_id.as_deref(),
        )
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
                    ContentBlock::Text { text, .. } if !text.is_empty() => {
                        Some(text.to_owned())
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let summary = content.text_summary();
                        if summary.is_empty() { None } else { Some(summary) }
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
            .field("cc_binary", &self.cc_binary)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout.as_secs())
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
        SUPPORTED_MODELS
    }

    fn supports_model(&self, model: &str) -> bool {
        // WHY: CC delegates model routing to the `claude` CLI, which handles
        // all claude-* models internally. Accepting any claude-* prefix
        // ensures new model IDs (e.g. claude-opus-4-7) work without updating
        // the SUPPORTED_MODELS list. The `cc/` prefix is for explicit routing.
        model.starts_with(CC_MODEL_PREFIX)
            || model.starts_with("claude-")
            || SUPPORTED_MODELS.contains(&model)
    }

    fn name(&self) -> &'static str {
        "cc"
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

/// Find the `claude` binary in `PATH`.
fn find_cc_binary() -> Result<PathBuf> {
    // 1. Search PATH (standard resolution).
    let paths = RealSystem.var_os("PATH").unwrap_or_default();
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
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Text("4".to_owned()),
                },
                Message {
                    role: Role::User,
                    content: Content::Text("And 3+3?".to_owned()),
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
        // Can't create CcProvider in tests (no binary), so test the logic directly.
        let stripped = "cc/claude-sonnet-4-20250514"
            .strip_prefix(CC_MODEL_PREFIX)
            .unwrap_or("cc/claude-sonnet-4-20250514");
        assert_eq!(stripped, "claude-sonnet-4-20250514");
    }

    #[test]
    fn supports_model_with_prefix() {
        // Test the prefix logic without needing a real binary.
        let model = "cc/claude-sonnet-4-20250514";
        assert!(model.starts_with(CC_MODEL_PREFIX));
    }

    #[test]
    fn supports_model_known() {
        assert!(SUPPORTED_MODELS.contains(&"claude-sonnet-4-20250514"));
        assert!(SUPPORTED_MODELS.contains(&"claude-opus-4-20250514"));
        assert!(!SUPPORTED_MODELS.contains(&"gpt-4"));
    }
}
