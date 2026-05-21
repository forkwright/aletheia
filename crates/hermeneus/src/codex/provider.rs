//! `CodexProvider`: routes LLM calls through the Codex CLI subprocess.
//!
//! Codex handles OAuth authentication via its local CLI credential store.
//! The provider only resolves the binary, formats requests, spawns the
//! subprocess, and wraps plain-text output in Hermeneus response types.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info};

use crate::error::{self, Result};
use crate::provider::LlmProvider;
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

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let model = self.resolve_model(&request.model);
        let prompt = Self::format_prompt(request);

        let output = Box::pin(process::run_completion(
            &self.codex_binary,
            request.system.as_deref(),
            &prompt,
            self.timeout,
        ))
        .await?;
        let text = parse::parse_output(&output.stdout)?;

        Ok(parse::text_to_response(&text, model))
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
        model.starts_with(CODEX_MODEL_PREFIX) || SUPPORTED_MODELS.contains(&model)
    }

    fn name(&self) -> &'static str {
        "codex"
    }
}

fn find_codex_binary() -> Result<PathBuf> {
    let paths = RealSystem.var_os("PATH").unwrap_or_default();
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
}
