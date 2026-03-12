//! Context distillation engine.

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{CompletionRequest, Content, Message, Role};
use snafu::ResultExt;
use tracing::instrument;

use crate::error::{EmptySummarySnafu, LlmCallSnafu, NoMessagesSnafu, Result};
use crate::prompt;

/// Sections that can appear in a distillation summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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
}

/// The distillation engine.
#[derive(Debug)]
pub struct DistillEngine {
    config: DistillConfig,
}

impl DistillEngine {
    /// Create a new distillation engine.
    pub fn new(config: DistillConfig) -> Self {
        Self { config }
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
        // Need enough messages to summarize beyond the verbatim tail.
        let required = self.config.min_messages + self.config.verbatim_tail;
        if message_count < required {
            return false;
        }
        if context_window == 0 {
            return false;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "token counts fit in f64 mantissa"
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

        let user_content = format!(
            "Distill the following conversation from nous \"{nous_id}\" \
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
        let to_summarize = &messages[..split_at];
        let verbatim = &messages[split_at..];

        let tokens_before = estimate_tokens(messages);
        let request = self.build_prompt(to_summarize, nous_id);
        let response = provider.complete(&request).await.context(LlmCallSnafu)?;

        let summary = extract_summary_text(&response.content);
        if summary.is_empty() {
            return EmptySummarySnafu.fail();
        }

        let tokens_after = response.usage.output_tokens;
        let timestamp = jiff::Timestamp::now().to_string();

        Ok(DistillResult {
            summary,
            messages_distilled: to_summarize.len(),
            tokens_before,
            tokens_after,
            distillation_number,
            timestamp,
            verbatim_messages: verbatim.to_vec(),
        })
    }

    /// Access the engine configuration.
    pub fn config(&self) -> &DistillConfig {
        &self.config
    }
}

/// Estimate token count from messages using chars/4 heuristic.
fn estimate_tokens(messages: &[Message]) -> u64 {
    let total_chars: usize = messages.iter().map(|m| m.content.text().len()).sum();
    (total_chars as u64).div_ceil(4)
}

/// Extract plain text from response content blocks.
fn extract_summary_text(content: &[aletheia_hermeneus::types::ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            aletheia_hermeneus::types::ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
#[path = "distill_tests.rs"]
mod tests;
