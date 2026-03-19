//! Context distillation engine.

use snafu::ResultExt;
use tracing::instrument;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};

use crate::error::{EmptySummarySnafu, LlmCallSnafu, NoMessagesSnafu, Result};
use crate::flush::{FlushItem, FlushSource, MemoryFlush};
use crate::prompt;

/// Maximum conversation turns to skip between distillation retry attempts.
const MAX_BACKOFF_TURNS: u32 = 8;

/// Bounded retry state to prevent distillation storms on repeated failures.
///
/// Tracks consecutive failures and enforces exponential backoff so that a
/// failing distillation does not trigger a retry on every subsequent turn.
#[derive(Debug, Default)]
struct RetryState {
    /// Consecutive distillation failures since the last success.
    consecutive_failures: u32,
    /// Remaining conversation turns before the next attempt is allowed.
    turns_to_skip: u32,
}

impl RetryState {
    fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        // NOTE: exponential backoff: 1, 2, 4, 8 turns; capped at MAX_BACKOFF_TURNS
        let shift = self.consecutive_failures.saturating_sub(1).min(3);
        self.turns_to_skip = (1u32 << shift).min(MAX_BACKOFF_TURNS);
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.turns_to_skip = 0;
    }
}

/// Sections that can appear in a distillation summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum DistillSection {
    /// One-sentence overview of the conversation topic.
    Summary,
    /// What was being worked on and why, including agent identity.
    TaskContext,
    /// Bullet list of concrete actions taken and their outcomes.
    CompletedWork,
    /// Decisions made with rationale that must survive distillation.
    KeyDecisions,
    /// Snapshot of where things stand: done, in-progress, half-finished.
    CurrentState,
    /// Unfinished items, pending questions, and deferred work.
    OpenThreads,
    /// Mistakes discovered and corrected to prevent repetition.
    Corrections,
    /// Custom section with a name and description.
    Custom { name: String, description: String },
}

impl DistillSection {
    /// Markdown heading for this section.
    pub fn heading(&self) -> String {
        match self {
            Self::Summary => "## Summary".to_owned(),
            Self::TaskContext => "## Task Context".to_owned(),
            Self::CompletedWork => "## Completed Work".to_owned(),
            Self::KeyDecisions => "## Key Decisions".to_owned(),
            Self::CurrentState => "## Current State".to_owned(),
            Self::OpenThreads => "## Open Threads".to_owned(),
            Self::Corrections => "## Corrections".to_owned(),
            Self::Custom { name, .. } => format!("## {name}"),
        }
    }

    /// Description text for this section (used in the system prompt).
    pub fn description(&self) -> &str {
        match self {
            Self::Summary => "One sentence describing what this conversation is about.",
            Self::TaskContext => {
                "What was being worked on and why. Include the agent/nous identity if relevant."
            }
            Self::CompletedWork => {
                "- Bullet list of concrete actions taken and their outcomes\n\
                 - Include file paths, function names, and specific details\n\
                 - Focus on results, not process"
            }
            Self::KeyDecisions => {
                "- Decisions made with their rationale — these MUST be preserved\n\
                 - Format: \"Decision: X. Reason: Y.\""
            }
            Self::CurrentState => {
                "Where things stand right now. What is done, what is in progress, what is half-finished."
            }
            Self::OpenThreads => {
                "- Unfinished items, pending questions, next steps\n\
                 - Items deferred for later"
            }
            Self::Corrections => {
                "- Anything that was wrong and corrected\n\
                 - Mistakes made and how they were fixed\n\
                 - These prevent repeating errors"
            }
            Self::Custom { description, .. } => description,
        }
    }

    /// All standard sections in default order.
    pub fn all_standard() -> Vec<Self> {
        vec![
            Self::Summary,
            Self::TaskContext,
            Self::CompletedWork,
            Self::KeyDecisions,
            Self::CurrentState,
            Self::OpenThreads,
            Self::Corrections,
        ]
    }
}

/// Configuration for a distillation run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DistillConfig {
    /// Model to use for distillation.
    pub model: String,
    /// Maximum output tokens for the summary.
    pub max_output_tokens: u32,
    /// Minimum messages before distillation is worthwhile.
    pub min_messages: usize,
    /// Whether to include tool call details in the summary.
    pub include_tool_calls: bool,
    /// If set, use this model for distillation instead of the primary model.
    /// Enables cost reduction (e.g., Opus primary -> Sonnet for distillation).
    pub distillation_model: Option<String>,
    /// Number of recent messages to preserve verbatim (not summarized).
    pub verbatim_tail: usize,
    /// Sections to include in the structured summary.
    pub sections: Vec<DistillSection>,
}

impl Default for DistillConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_owned(),
            max_output_tokens: 4096,
            min_messages: 6,
            include_tool_calls: true,
            distillation_model: None,
            verbatim_tail: 3,
            sections: DistillSection::all_standard(),
        }
    }
}

/// Result of a distillation run.
#[derive(Debug, Clone)]
pub struct DistillResult {
    /// The distilled summary text.
    pub summary: String,
    /// Number of messages that were distilled (excluding verbatim tail).
    pub messages_distilled: usize,
    /// Estimated tokens before distillation.
    pub tokens_before: u64,
    /// Estimated tokens after distillation.
    pub tokens_after: u64,
    /// Which distillation number this is for the session.
    pub distillation_number: u32,
    /// Timestamp of distillation (ISO 8601).
    pub timestamp: String,
    /// Messages preserved verbatim (not summarized).
    pub verbatim_messages: Vec<Message>,
    /// Structured memory items extracted from the summary for long-term persistence.
    pub memory_flush: MemoryFlush,
}

/// The distillation engine.
#[derive(Debug)]
pub struct DistillEngine {
    config: DistillConfig,
    // WHY: std::sync::Mutex: retry counter check/increment is O(1), never crosses an await point.
    retry_state: std::sync::Mutex<RetryState>,
}

impl DistillEngine {
    /// Create a new distillation engine.
    pub fn new(config: DistillConfig) -> Self {
        Self {
            config,
            retry_state: std::sync::Mutex::new(RetryState::default()),
        }
    }

    /// Acquire the retry state lock, recovering from a poisoned mutex.
    ///
    /// WHY: If a thread panicked while holding this lock, we recover the state
    /// rather than propagating the panic. The retry counters are non-critical
    /// bookkeeping: using a slightly stale value is preferable to crashing.
    fn lock_retry_state(&self) -> std::sync::MutexGuard<'_, RetryState> {
        self.retry_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Advance the backoff counter by one conversation turn.
    ///
    /// Call once at the start of each conversation turn, before calling
    /// [`should_distill`][Self::should_distill]. Returns `true` if the engine is
    /// still in a backoff period and distillation should be skipped this turn.
    pub fn tick_turn(&self) -> bool {
        let mut state = self.lock_retry_state();
        if state.turns_to_skip > 0 {
            state.turns_to_skip -= 1;
            return true;
        }
        false
    }

    /// Returns `true` if the engine is in an active backoff period.
    ///
    /// Does not advance state. Use [`tick_turn`][Self::tick_turn] to advance.
    pub fn in_backoff(&self) -> bool {
        self.lock_retry_state().turns_to_skip > 0
    }

    /// Check if the given messages warrant distillation.
    ///
    /// Returns true when message count meets the minimum (accounting for
    /// verbatim tail) AND the token estimate exceeds the threshold ratio
    /// of the context window.
    pub fn should_distill(
        &self,
        message_count: usize,
        token_estimate: u64,
        context_window: u64,
        threshold: f64,
    ) -> bool {
        // NOTE: need enough messages to exceed the verbatim tail to trigger summarization
        let required = self.config.min_messages + self.config.verbatim_tail;
        if message_count < required {
            return false;
        }
        if context_window == 0 {
            return false;
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "u64→f64: token counts fit in f64 mantissa"
        )]
        let ratio = token_estimate as f64 / context_window as f64;
        ratio >= threshold
    }

    /// Build the distillation prompt for the given messages.
    pub fn build_prompt(&self, messages: &[Message], nous_id: &str) -> CompletionRequest {
        let formatted = prompt::format_messages(messages, self.config.include_tool_calls);
        let system_prompt = prompt::build_system_prompt(&self.config.sections);

        let model = self
            .config
            .distillation_model
            .as_deref()
            .unwrap_or(&self.config.model)
            .to_owned();

        let safe_nous_id = sanitize_nous_id(nous_id);
        let user_content = format!(
            "Distill the following conversation from nous \"{safe_nous_id}\" \
             (distillation context: {msg_count} messages).\n\n\
             ---\n\n{formatted}",
            msg_count = messages.len(),
        );

        CompletionRequest {
            model,
            system: Some(system_prompt),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_content),
            }],
            max_tokens: self.config.max_output_tokens,
            tools: vec![],
            temperature: Some(0.0),
            thinking: None,
            stop_sequences: vec![],
            ..Default::default()
        }
    }

    /// Run distillation: call LLM, return the summary.
    ///
    /// Splits messages into a summarization group and a verbatim tail.
    /// Only the summarization group is sent to the LLM.
    ///
    /// Records success or failure into the backoff state so that
    /// [`tick_turn`][Self::tick_turn] gates subsequent retry attempts.
    #[instrument(skip(self, messages, provider), fields(nous_id, distillation_number))]
    pub async fn distill(
        &self,
        messages: &[Message],
        nous_id: &str,
        provider: &dyn LlmProvider,
        distillation_number: u32,
    ) -> Result<DistillResult> {
        if messages.is_empty() {
            return NoMessagesSnafu.fail();
        }

        let tail = self.config.verbatim_tail.min(messages.len());
        let split_at = messages.len() - tail;
        // split_at == messages.len() - tail where tail <= messages.len(), so split_at <= messages.len()
        #[expect(
            clippy::indexing_slicing,
            reason = "split_at = messages.len() - tail where tail ≤ messages.len()"
        )]
        let to_summarize = &messages[..split_at];
        #[expect(
            clippy::indexing_slicing,
            reason = "split_at ≤ messages.len() by construction"
        )]
        let verbatim = &messages[split_at..];

        let tokens_before = estimate_tokens(messages);
        let request = self.build_prompt(to_summarize, nous_id);

        let response = match provider.complete(&request).await.context(LlmCallSnafu) {
            Ok(r) => {
                self.lock_retry_state().record_success();
                r
            }
            Err(e) => {
                self.lock_retry_state().record_failure();
                return Err(e);
            }
        };

        let summary = extract_summary_text(&response.content);
        if summary.is_empty() {
            self.lock_retry_state().record_failure();
            return EmptySummarySnafu.fail();
        }

        let tokens_after = response.usage.output_tokens;
        let timestamp = jiff::Timestamp::now().to_string();
        let memory_flush = parse_summary_to_flush(&summary, &timestamp);

        Ok(DistillResult {
            summary,
            messages_distilled: to_summarize.len(),
            tokens_before,
            tokens_after,
            distillation_number,
            timestamp,
            verbatim_messages: verbatim.to_vec(),
            memory_flush,
        })
    }

    /// Access the engine configuration.
    pub fn config(&self) -> &DistillConfig {
        &self.config
    }
}

/// Strip characters that could alter prompt semantics from a `nous_id`.
///
/// Removes backticks, newlines, and all control characters so that an
/// untrusted ID cannot inject prompt fragments.
fn sanitize_nous_id(id: &str) -> String {
    id.chars()
        .filter(|c| !c.is_control() && *c != '`')
        .collect()
}

/// Estimate token count for a slice of messages using the chars/4 heuristic.
///
/// Includes text, tool use inputs, and tool result content so that
/// messages containing tool calls are not underestimated.
fn estimate_tokens(messages: &[Message]) -> u64 {
    let total_chars: usize = messages.iter().map(estimate_single_message_chars).sum();
    #[expect(
        clippy::as_conversions,
        reason = "usize→u64: widening cast, always valid"
    )]
    (total_chars as u64).div_ceil(4)
}

/// Character count for a single message across all content types.
fn estimate_single_message_chars(msg: &Message) -> usize {
    content_char_len(&msg.content)
}

/// Character count for a Content value.
fn content_char_len(content: &Content) -> usize {
    match content {
        Content::Text(s) => s.len(),
        Content::Blocks(blocks) => blocks.iter().map(block_char_len).sum(),
        _ => 0,
    }
}

/// Character count for a `ContentBlock`, including tool payloads.
fn block_char_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text, .. } => text.len(),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => name.len() + input.to_string().len(),
        ContentBlock::ToolResult { content, .. } => content.text_summary().len(),
        ContentBlock::Thinking { thinking, .. } => thinking.len(),
        ContentBlock::WebSearchToolResult { content, .. } => content.to_string().len(),
        ContentBlock::CodeExecutionResult {
            code,
            stdout,
            stderr,
            ..
        } => code.len() + stdout.len() + stderr.len(),
        // WHY: ContentBlock is #[non_exhaustive]; future variants default to zero.
        _ => 0,
    }
}

/// Drop the oldest messages until the token estimate fits within `context_window`.
///
/// Returns the number of messages dropped. Always keeps at least one message
/// even when the remaining context still exceeds the window: dropping
/// everything would leave the conversation unrecoverable.
///
/// Logs at `ERROR` level when any messages are dropped, because this is a
/// last-resort fallback indicating that distillation has failed to keep the
/// context in bounds.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "callable by the session management layer; no call site exists within this crate"
    )
)]
pub(crate) fn enforce_context_limit(messages: &mut Vec<Message>, context_window: u64) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let initial = estimate_tokens(messages);
    if initial <= context_window {
        return 0;
    }
    tracing::error!(
        context_tokens = initial,
        context_window,
        message_count = messages.len(),
        "context exceeds window; dropping oldest messages as last-resort fallback"
    );
    let mut dropped = 0;
    while messages.len() > 1 && estimate_tokens(messages) > context_window {
        messages.remove(0);
        dropped += 1;
    }
    dropped
}

/// Extract plain text from response content blocks.
fn extract_summary_text(content: &[aletheia_hermeneus::types::ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            aletheia_hermeneus::types::ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(s);
            acc
        })
        .trim()
        .to_owned()
}

/// Parse a distillation summary into structured memory items.
///
/// Extracts key decisions, corrections, and task context from the markdown
/// sections of the summary, populating a [`MemoryFlush`] for the caller to
/// persist to long-term storage.
fn parse_summary_to_flush(summary: &str, timestamp: &str) -> MemoryFlush {
    let mut decisions: Vec<FlushItem> = Vec::new();
    let mut corrections: Vec<FlushItem> = Vec::new();
    let mut task_state: Option<String> = None;
    let mut current_section = "";
    let mut section_lines: Vec<&str> = Vec::new();

    for line in summary.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            collect_flush_section(
                current_section,
                &section_lines,
                timestamp,
                &mut decisions,
                &mut corrections,
                &mut task_state,
            );
            current_section = heading.trim();
            section_lines.clear();
        } else {
            section_lines.push(line);
        }
    }
    collect_flush_section(
        current_section,
        &section_lines,
        timestamp,
        &mut decisions,
        &mut corrections,
        &mut task_state,
    );

    MemoryFlush {
        decisions,
        corrections,
        facts: vec![],
        task_state,
    }
}

/// Process one markdown section of a distillation summary into flush items.
fn collect_flush_section(
    section: &str,
    lines: &[&str],
    timestamp: &str,
    decisions: &mut Vec<FlushItem>,
    corrections: &mut Vec<FlushItem>,
    task_state: &mut Option<String>,
) {
    let content: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if content.is_empty() {
        return;
    }
    match section {
        "Key Decisions" => {
            for &line in &content {
                let text = line.trim_start_matches('-').trim();
                if !text.is_empty() {
                    decisions.push(FlushItem {
                        content: text.to_owned(),
                        timestamp: timestamp.to_owned(),
                        source: FlushSource::Extracted,
                    });
                }
            }
        }
        "Corrections" => {
            for &line in &content {
                let text = line.trim_start_matches('-').trim();
                if !text.is_empty() {
                    corrections.push(FlushItem {
                        content: text.to_owned(),
                        timestamp: timestamp.to_owned(),
                        source: FlushSource::Extracted,
                    });
                }
            }
        }
        "Task Context" => {
            *task_state = Some(content.join("\n"));
        }
        _ => {
            // NOTE: unrecognized sections are not extracted
        }
    }
}

#[cfg(test)]
#[path = "distill_tests/mod.rs"]
mod tests;
