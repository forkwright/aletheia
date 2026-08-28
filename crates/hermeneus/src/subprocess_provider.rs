//! Shared subprocess-provider scaffolding for the CLI-subprocess LLM
//! providers (`cc`, `kimi`, `codex`).
//!
//! Each of those providers spawns a local CLI binary, formats aletheia's
//! message history into a flat prompt, retries a failed subprocess call
//! before any output has reached the caller, and records the same
//! completion/latency/cache-token metrics — differing only in which
//! `process::run_completion`/`run_streaming` call it makes and which binary
//! it looks for. This module is that shared machinery; each provider
//! supplies only its own subprocess call as a closure.
//!
//! WHY(#7016): the three providers had copy-pasted this machinery and had
//! already drifted — `codex` labeled turns "User" where `cc`/`kimi` used
//! "Human" for the same [`Role::User`], and only `codex` rendered
//! `ContentBlock::ToolUse` in a flattened prompt (the other two silently
//! dropped tool-use turns from history — a live correctness bug, not a
//! difference in requirements). Consolidating resolves both to ONE
//! behavior: "Human" (the label two of three providers already agreed on)
//! and always rendering `ToolUse` (the fix `codex` already carried under
//! #3980, now shared instead of missing on the other two).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;

use koina::system::{Environment, RealSystem};
use tracing::{debug, info, warn};

use crate::RetryPolicy;
use crate::error::{self, Error, Result};
use crate::types::{CompletionRequest, CompletionResponse, Content, ContentBlock, Role};

/// Locate a CLI binary: search `PATH` first, then the given `$HOME`-relative
/// fallback subdirectories (covers systemd user sessions and per-tool
/// install layouts where the binary's own directory may not be on `PATH`).
pub(crate) fn find_binary(
    bin_name: &str,
    home_fallback_subdirs: &[&str],
    not_found_hint: &str,
) -> Result<PathBuf> {
    let paths = RealSystem.var_os("PATH").unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<OsString>, not Result — empty PATH is a valid fallback
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(bin_name);
        if candidate.is_file() {
            debug!(path = %candidate.display(), bin_name, "found binary in PATH");
            return Ok(candidate);
        }
    }

    if let Some(home) = RealSystem.var_os("HOME") {
        let home = PathBuf::from(home);
        for subdir in home_fallback_subdirs {
            let candidate = home.join(subdir);
            if candidate.is_file() {
                info!(
                    path = %candidate.display(),
                    bin_name,
                    "found binary outside PATH (consider adding its directory to PATH)"
                );
                return Ok(candidate);
            }
        }
    }

    Err(error::ProviderInitSnafu {
        message: not_found_hint.to_owned(),
    }
    .build())
}

/// Validate a configured subprocess working directory: `None` passes
/// through unchanged (the subprocess inherits the parent cwd); `Some` must
/// name a directory that exists.
pub(crate) fn validate_working_directory(
    path: Option<&Path>,
    product_name: &str,
) -> Result<Option<PathBuf>> {
    match path {
        Some(path) if path.is_dir() => Ok(Some(path.to_path_buf())),
        Some(path) => Err(error::ProviderInitSnafu {
            message: format!(
                "configured {product_name} working directory does not exist: {}",
                path.display()
            ),
        }
        .build()),
        None => Ok(None),
    }
}

/// Label a message role for a flattened multi-turn subprocess prompt.
///
/// WHY(#7016): the one surviving label — `codex` used "User" where
/// `cc`/`kimi` used "Human" for the same [`Role::User`]; kept as "Human"
/// since two of the three providers already agreed on it.
fn role_label(role: Role) -> &'static str {
    match role {
        Role::User => "Human",
        Role::Assistant => "Assistant",
        Role::System => "System",
    }
}

/// Format message history into a single flat prompt string for a
/// subprocess-CLI provider's `-p`/print mode.
///
/// A single message passes through unlabeled (the common case for
/// aletheia); multi-turn history is flattened into labeled sections so the
/// subprocess retains conversational context it cannot otherwise receive.
pub(crate) fn format_prompt(request: &CompletionRequest) -> String {
    if request.messages.len() == 1
        && let Some(msg) = request.messages.first()
    {
        return msg.content.text();
    }

    let mut parts = Vec::new();
    for msg in &request.messages {
        let label = role_label(msg.role);
        let text = extract_text_content(&msg.content);
        if !text.is_empty() {
            parts.push(format!("{label}: {text}"));
        }
    }
    parts.join("\n\n")
}

/// Extract plain text from message content, joining blocks if structured.
///
/// WHY(#3980, #7016): tool-use turns are rendered as a `[Tool call: ...]`
/// marker rather than dropped. `codex` fixed this for its own prompt under
/// #3980 while `cc`/`kimi` still silently dropped `ContentBlock::ToolUse` —
/// a live correctness bug (a tool-call assistant turn vanishes from the
/// history sent to the subprocess), not a difference in requirements.
/// Consolidating on the fixed behavior for all three closes that gap.
pub(crate) fn extract_text_content(content: &Content) -> String {
    match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } if !text.is_empty() => Some(text.to_owned()),
                    ContentBlock::ToolUse { name, input, .. } => {
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
                    // NOTE: thinking, server tool use, web search, and code
                    // execution blocks have no text representation in a flat
                    // subprocess prompt.
                    _ => None,
                })
                .collect();
            parts.join("\n")
        }
    }
}

/// Run a fallible subprocess attempt under the standard hermeneus retry
/// policy: retry while `Error::is_retryable`, sleep per
/// [`RetryPolicy::delay`] between attempts, give up once
/// [`RetryPolicy`]'s `max_retries` is exhausted.
///
/// `attempt` performs one full subprocess call. `abort` runs on every
/// failed attempt before the standard retry check; when it returns `true`
/// the loop ends immediately without consuming a retry attempt or logging a
/// "retrying" line — the caller is responsible for logging its own abort
/// reason before returning `true`. This is how a streaming caller stops
/// retrying once a content delta has already reached its consumer
/// (retrying then would duplicate output the caller already emitted); a
/// non-streaming caller passes `|_| false`.
/// Drives the shared retry policy for a caller that owns its own attempt loop.
///
/// WHY this exists beside `run_with_retry`: a streaming attempt borrows a delta
/// callback, and a `FnMut() -> Fut` closure cannot return a future that borrows
/// its own locals -- rustc rejects it with "captured variable cannot escape
/// `FnMut` closure body". The streaming callers therefore keep their own loop and
/// share the *policy* instead: the delay schedule, the retryable/exhausted
/// decision, and the warn line. That was the duplicated part; the loop shape was
/// only ever incidental to it.
pub(crate) struct RetryLoop<'a> {
    provider_name: &'a str,
    label: &'a str,
    policy: RetryPolicy,
    last_error: Option<Error>,
    attempt: u32,
    started: bool,
}

impl<'a> RetryLoop<'a> {
    pub(crate) fn new(provider_name: &'a str, label: &'a str) -> Self {
        Self {
            provider_name,
            label,
            policy: RetryPolicy::default(),
            last_error: None,
            attempt: 0,
            started: false,
        }
    }

    /// Wait out this attempt's backoff and report whether another attempt is
    /// permitted. The first call returns immediately; later ones sleep.
    pub(crate) async fn next_attempt(&mut self) -> bool {
        if !self.started {
            self.started = true;
            return true;
        }
        self.attempt += 1;
        if self.attempt > self.policy.max_retries {
            return false;
        }
        tokio::time::sleep(self.policy.delay(self.attempt, self.last_error.as_ref())).await;
        true
    }

    /// Record a failed attempt. Returns `Some(err)` when the caller must stop and
    /// return it: the caller vetoed a retry, the error is not retryable, or the
    /// budget is spent.
    pub(crate) fn record(&mut self, err: Error, abort: bool) -> Option<Error> {
        if abort || self.attempt >= self.policy.max_retries || !err.is_retryable() {
            return Some(err);
        }
        let label = self.label;
        warn!(
            provider = %self.provider_name,
            attempt = self.attempt,
            error = %err,
            "{label} call failed; retrying"
        );
        self.last_error = Some(err);
        None
    }

    /// The error to return when the loop ends with neither a success nor a
    /// decisive failure.
    pub(crate) fn exhausted(self) -> Error {
        let label = self.label;
        self.last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: format!("{label} retry loop exhausted with no recorded error"),
            }
            .build()
        })
    }
}

pub(crate) async fn run_with_retry<T, Attempt, Fut>(
    provider_name: &str,
    label: &str,
    mut attempt: Attempt,
    mut abort: impl FnMut(&Error) -> bool,
) -> Result<T>
where
    Attempt: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let retry_policy = RetryPolicy::default();
    let mut last_error = None;
    for n in 0..=retry_policy.max_retries {
        if n > 0 {
            tokio::time::sleep(retry_policy.delay(n, last_error.as_ref())).await;
        }
        match attempt().await {
            Ok(output) => return Ok(output),
            Err(err) => {
                if abort(&err) {
                    return Err(err);
                }
                if n == retry_policy.max_retries || !err.is_retryable() {
                    return Err(err);
                }
                warn!(
                    provider = %provider_name,
                    attempt = n,
                    error = %err,
                    "{label} call failed; retrying"
                );
                last_error = Some(err);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        error::ApiRequestSnafu {
            message: format!("{label} retry loop exhausted with no recorded error"),
        }
        .build()
    }))
}

/// Record the completion/latency/cache-token metrics every CLI-subprocess
/// provider emits identically for both its streaming and non-streaming
/// path.
pub(crate) fn record_completion_metrics(
    provider_name: &str,
    model: &str,
    start: Instant,
    outcome: &Result<CompletionResponse>,
) {
    match outcome {
        Ok(response) => {
            // WHY(#4658): every subprocess-CLI provider reports at least one
            // cache-token direction (`cc`/`kimi` both, `codex` reads only);
            // recording both unconditionally keeps prompt-cache usage
            // visible in provider metrics regardless of which applies.
            crate::metrics::record_cache_tokens(
                provider_name,
                response.usage.cache_read_tokens,
                response.usage.cache_write_tokens,
            );
            crate::metrics::record_completion(
                provider_name,
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
            crate::metrics::record_completion(provider_name, 0, 0, 0.0, false);
            crate::metrics::record_latency(model, status, start.elapsed().as_secs_f64());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, ToolResultContent};

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
        assert_eq!(format_prompt(&request), "hello world");
    }

    #[test]
    fn format_prompt_multi_turn_uses_human_label() {
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
        let prompt = format_prompt(&request);
        assert!(prompt.contains("Human: What is 2+2?"));
        assert!(prompt.contains("Assistant: 4"));
        assert!(prompt.contains("Human: And 3+3?"));
    }

    // WHY(#3980, #7016): ToolUse blocks must be rendered for every
    // subprocess provider now, not just codex — this is the fixed behavior
    // all three share post-consolidation.
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
        let prompt = format_prompt(&request);
        assert!(
            prompt.contains("Human: What is in /etc/hosts?"),
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
}
