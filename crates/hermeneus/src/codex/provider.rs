//! `CodexProvider`: routes LLM calls through the Codex CLI subprocess.
//!
//! Codex handles OAuth authentication via its local CLI credential store.
//! The provider only resolves the binary, formats requests, spawns the
//! subprocess, and wraps plain-text output in Hermeneus response types.

use std::future::Future;
use std::path::PathBuf;
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

/// Models Codex can route to. The CLI itself resolves aliases and availability.
const SUPPORTED_MODELS: &[&str] = &["gpt-5-codex"];

/// Configuration for the Codex subprocess provider.
#[derive(Debug, Clone)]
pub struct CodexProviderConfig {
    /// Path to the `codex` binary. If `None`, resolved from `PATH`.
    pub codex_binary: Option<PathBuf>,
    /// Default model when the request doesn't specify one.
    pub default_model: String,
    /// Subprocess timeout (wall-clock).
    pub timeout: Duration,
}

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            codex_binary: None,
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_mins(5),
        }
    }
}

/// Codex subprocess LLM provider.
pub struct CodexProvider {
    // kanon:ignore RUST/pub-visibility
    codex_binary: PathBuf,
    default_model: String,
    timeout: Duration,
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

        info!(
            binary = %codex_binary.display(),
            default_model = %config.default_model,
            timeout_secs = config.timeout.as_secs(),
            "Codex subprocess provider initialized"
        );

        Ok(Self {
            codex_binary,
            default_model: config.default_model.clone(),
            timeout: config.timeout,
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
            "gpt-5-codex"
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
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
        let prompt = Self::format_prompt(request);

        let output = Box::pin(process::run_completion(
            &self.codex_binary,
            request.system.as_deref(),
            &prompt,
            self.timeout,
        ))
        .await?;
        let parse::CodexParsedOutput { text, usage } = parse::parse_output(&output.stdout)?;

        Ok(parse::text_to_response(&text, usage, model))
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
        let model = self.resolve_model(&request.model);
        Self::warn_dropped_tools(request.tools.len());
        let prompt = Self::format_prompt(request);

        let output = Box::pin(process::run_completion(
            &self.codex_binary,
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

        Ok(parse::text_to_response(&text, usage, model))
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
            .field("codex_binary", &self.codex_binary)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout.as_secs())
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
        SUPPORTED_MODELS
    }

    fn supports_model(&self, model: &str) -> bool {
        self.match_specificity(model).is_some()
    }

    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        if model.starts_with(CODEX_MODEL_PREFIX) {
            Some(MatchKind::Prefix)
        } else if SUPPORTED_MODELS.contains(&model) {
            Some(MatchKind::Exact)
        } else {
            None
        }
    }

    fn name(&self) -> &'static str {
        "codex"
    }

    fn deployment_target(&self) -> DeploymentTarget {
        DeploymentTarget::Cloud
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
            codex_binary: PathBuf::from("codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(provider.resolve_model("codex/gpt-5-codex"), "gpt-5-codex");
        assert_eq!(provider.resolve_model(""), "gpt-5-codex");
    }

    #[test]
    fn supports_model_with_prefix() {
        let provider = CodexProvider {
            codex_binary: PathBuf::from("codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert!(provider.supports_model("codex/gpt-5-codex"));
        assert!(provider.supports_model("gpt-5-codex"));
        assert!(!provider.supports_model("claude-sonnet-4-6"));
    }

    #[test]
    fn match_specificity_prefers_prefix_and_exact() {
        let provider = CodexProvider {
            codex_binary: PathBuf::from("codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            provider.match_specificity("codex/gpt-5-codex"),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            provider.match_specificity("gpt-5-codex"),
            Some(MatchKind::Exact)
        );
        assert_eq!(provider.match_specificity("claude-sonnet-4-6"), None);
    }

    #[test]
    fn codex_provider_reports_cloud_deployment_target() {
        let provider = CodexProvider {
            codex_binary: PathBuf::from("codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
    }

    #[test]
    fn codex_provider_supports_streaming() {
        let provider = CodexProvider {
            codex_binary: PathBuf::from("codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(1),
        };
        assert!(
            provider.supports_streaming(),
            "CodexProvider must report supports_streaming=true after #3980"
        );
    }

    #[test]
    fn seat_bridged_fields() {
        let provider = CodexProvider {
            codex_binary: PathBuf::from("/usr/local/bin/codex"),
            default_model: "codex/gpt-5-codex".to_owned(),
            timeout: Duration::from_secs(300),
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
}
