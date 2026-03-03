//! Context distillation engine.

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{CompletionRequest, Content, Message, Role};
use snafu::ResultExt;
use tracing::instrument;

use crate::error::{EmptySummarySnafu, LlmCallSnafu, NoMessagesSnafu, Result};
use crate::prompt::{self, DISTILLATION_SYSTEM_PROMPT};

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
}

impl Default for DistillConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_owned(),
            max_output_tokens: 4096,
            min_messages: 6,
            include_tool_calls: true,
        }
    }
}

/// Result of a distillation run.
#[derive(Debug, Clone)]
pub struct DistillResult {
    /// The distilled summary text.
    pub summary: String,
    /// Number of messages that were distilled.
    pub messages_distilled: usize,
    /// Estimated tokens before distillation.
    pub tokens_before: u64,
    /// Estimated tokens after distillation.
    pub tokens_after: u64,
    /// Which distillation number this is for the session.
    pub distillation_number: u32,
    /// Timestamp of distillation (ISO 8601).
    pub timestamp: String,
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
    /// Returns true when message count meets the minimum AND
    /// the token estimate exceeds the threshold ratio of the context window.
    pub fn should_distill(
        &self,
        message_count: usize,
        token_estimate: u64,
        context_window: u64,
        threshold: f64,
    ) -> bool {
        if message_count < self.config.min_messages {
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

        let user_content = format!(
            "Distill the following conversation from nous \"{nous_id}\" \
             (distillation context: {msg_count} messages).\n\n\
             ---\n\n{formatted}",
            msg_count = messages.len(),
        );

        CompletionRequest {
            model: self.config.model.clone(),
            system: Some(DISTILLATION_SYSTEM_PROMPT.to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(user_content),
            }],
            max_tokens: self.config.max_output_tokens,
            tools: vec![],
            temperature: Some(0.0),
            thinking: None,
            stop_sequences: vec![],
        }
    }

    /// Run distillation: call LLM, return the summary.
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

        let tokens_before = estimate_tokens(messages);
        let request = self.build_prompt(messages, nous_id);
        let response = provider.complete(&request).context(LlmCallSnafu)?;

        let summary = extract_summary_text(&response.content);
        if summary.is_empty() {
            return EmptySummarySnafu.fail();
        }

        let tokens_after = response.usage.output_tokens;
        let timestamp = jiff::Timestamp::now().to_string();

        Ok(DistillResult {
            summary,
            messages_distilled: messages.len(),
            tokens_before,
            tokens_after,
            distillation_number,
            timestamp,
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
            aletheia_hermeneus::types::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use aletheia_hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};

    use super::*;

    // --- Mock provider ---

    struct MockProvider {
        response: std::sync::Mutex<Option<aletheia_hermeneus::error::Result<CompletionResponse>>>,
    }

    impl MockProvider {
        fn success(summary: &str) -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
                    id: "msg_distill_1".to_owned(),
                    model: "claude-sonnet-4-20250514".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![ContentBlock::Text {
                        text: summary.to_owned(),
                    }],
                    usage: Usage {
                        input_tokens: 5000,
                        output_tokens: 200,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                    },
                }))),
            }
        }

        fn empty_response() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
                    id: "msg_empty".to_owned(),
                    model: "claude-sonnet-4-20250514".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![],
                    usage: Usage::default(),
                }))),
            }
        }

        fn empty_text_blocks() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Ok(CompletionResponse {
                    id: "msg_empty_text".to_owned(),
                    model: "claude-sonnet-4-20250514".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![
                        ContentBlock::Text {
                            text: String::new(),
                        },
                        ContentBlock::Text {
                            text: "   ".to_owned(),
                        },
                    ],
                    usage: Usage::default(),
                }))),
            }
        }

        fn failure() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Err(
                    aletheia_hermeneus::error::ApiRequestSnafu {
                        message: "network timeout".to_owned(),
                    }
                    .build(),
                ))),
            }
        }
    }

    impl LlmProvider for MockProvider {
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            self.response
                .lock()
                .expect("lock poisoned")
                .take()
                .expect("mock provider called more than once")
        }

        fn supported_models(&self) -> &[&str] {
            &["claude-sonnet-4-20250514"]
        }

        #[expect(clippy::unnecessary_literal_bound)]
        fn name(&self) -> &str {
            "mock-distill"
        }
    }

    // --- Helpers ---

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
        }
    }

    fn sample_conversation() -> Vec<Message> {
        vec![
            text_msg(Role::User, "Help me fix the login bug"),
            text_msg(Role::Assistant, "I'll look at the auth module"),
            text_msg(Role::User, "It's in src/auth/login.rs"),
            text_msg(
                Role::Assistant,
                "Found the issue — missing null check on line 42",
            ),
            text_msg(Role::User, "Great, fix it please"),
            text_msg(Role::Assistant, "Done. Added the check and a test."),
        ]
    }

    fn default_engine() -> DistillEngine {
        DistillEngine::new(DistillConfig::default())
    }

    const MOCK_SUMMARY: &str = "\
## Summary
Fixed login bug in auth module.

## Task Context
Working on a null pointer crash in the login flow.

## Completed Work
- Fixed null check on line 42 of src/auth/login.rs
- Added regression test

## Key Decisions
- Decision: Add null check rather than restructure. Reason: Minimal change for the fix.

## Current State
Bug is fixed, test passes.

## Open Threads
- None

## Corrections
- Initially looked at wrong file before finding the issue in login.rs";

    // --- should_distill tests ---

    #[test]
    fn should_distill_below_threshold_returns_false() {
        let engine = default_engine();
        assert!(!engine.should_distill(10, 50_000, 200_000, 0.8));
    }

    #[test]
    fn should_distill_at_threshold_returns_true() {
        let engine = default_engine();
        assert!(engine.should_distill(10, 160_000, 200_000, 0.8));
    }

    #[test]
    fn should_distill_above_threshold_returns_true() {
        let engine = default_engine();
        assert!(engine.should_distill(10, 190_000, 200_000, 0.8));
    }

    #[test]
    fn should_distill_too_few_messages_returns_false() {
        let engine = default_engine();
        // 5 messages < min_messages (6), even though tokens exceed threshold
        assert!(!engine.should_distill(5, 190_000, 200_000, 0.8));
    }

    #[test]
    fn should_distill_zero_context_window_returns_false() {
        let engine = default_engine();
        assert!(!engine.should_distill(10, 100, 0, 0.8));
    }

    #[test]
    fn should_distill_exact_min_messages() {
        let engine = default_engine();
        // Exactly 6 messages (min_messages) with tokens above threshold
        assert!(engine.should_distill(6, 180_000, 200_000, 0.8));
    }

    // --- build_prompt tests ---

    #[test]
    fn build_prompt_has_system_prompt() {
        let engine = default_engine();
        let messages = sample_conversation();
        let request = engine.build_prompt(&messages, "test-nous");

        assert!(request.system.is_some());
        let system = request.system.unwrap();
        assert!(system.contains("## Summary"));
        assert!(system.contains("## Key Decisions"));
        assert!(system.contains("## Corrections"));
    }

    #[test]
    fn build_prompt_includes_nous_id() {
        let engine = default_engine();
        let messages = sample_conversation();
        let request = engine.build_prompt(&messages, "my-agent");

        let user_text = request.messages[0].content.text();
        assert!(user_text.contains("my-agent"));
    }

    #[test]
    fn build_prompt_formats_messages_with_roles() {
        let engine = default_engine();
        let messages = sample_conversation();
        let request = engine.build_prompt(&messages, "test-nous");

        let user_text = request.messages[0].content.text();
        assert!(user_text.contains("[USER]"));
        assert!(user_text.contains("[ASSISTANT]"));
    }

    #[test]
    fn build_prompt_uses_config_model() {
        let config = DistillConfig {
            model: "claude-haiku-4-5-20251001".to_owned(),
            ..DistillConfig::default()
        };
        let engine = DistillEngine::new(config);
        let request = engine.build_prompt(&sample_conversation(), "test");
        assert_eq!(request.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn build_prompt_uses_config_max_tokens() {
        let config = DistillConfig {
            max_output_tokens: 2048,
            ..DistillConfig::default()
        };
        let engine = DistillEngine::new(config);
        let request = engine.build_prompt(&sample_conversation(), "test");
        assert_eq!(request.max_tokens, 2048);
    }

    #[test]
    fn build_prompt_no_tools() {
        let engine = default_engine();
        let request = engine.build_prompt(&sample_conversation(), "test");
        assert!(request.tools.is_empty());
    }

    // --- distill tests ---

    #[tokio::test]
    async fn distill_success_returns_result() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::success(MOCK_SUMMARY);

        let result = engine.distill(&messages, "test-nous", &provider, 1).await;
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.summary.contains("Fixed login bug"));
        assert_eq!(result.messages_distilled, 6);
        assert_eq!(result.distillation_number, 1);
    }

    #[tokio::test]
    async fn distill_token_estimates_populated() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::success(MOCK_SUMMARY);

        let result = engine
            .distill(&messages, "test-nous", &provider, 1)
            .await
            .unwrap();

        assert!(result.tokens_before > 0);
        assert_eq!(result.tokens_after, 200); // from mock Usage
    }

    #[tokio::test]
    async fn distill_distillation_number_passed_through() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::success(MOCK_SUMMARY);

        let result = engine
            .distill(&messages, "test-nous", &provider, 42)
            .await
            .unwrap();
        assert_eq!(result.distillation_number, 42);
    }

    #[tokio::test]
    async fn distill_timestamp_is_valid() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::success(MOCK_SUMMARY);

        let result = engine
            .distill(&messages, "test-nous", &provider, 1)
            .await
            .unwrap();

        // jiff::Timestamp::to_string() produces RFC 3339 / ISO 8601
        assert!(
            result.timestamp.contains('T'),
            "timestamp should be ISO 8601: {}",
            result.timestamp
        );
    }

    // --- error cases ---

    #[tokio::test]
    async fn distill_empty_messages_returns_no_messages_error() {
        let engine = default_engine();
        let provider = MockProvider::success(MOCK_SUMMARY);

        let result = engine.distill(&[], "test-nous", &provider, 1).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("no messages"));
    }

    #[tokio::test]
    async fn distill_llm_failure_returns_llm_call_error() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::failure();

        let result = engine.distill(&messages, "test-nous", &provider, 1).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("LLM call failed"));
    }

    #[tokio::test]
    async fn distill_empty_response_returns_empty_summary_error() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::empty_response();

        let result = engine.distill(&messages, "test-nous", &provider, 1).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty summary"));
    }

    #[tokio::test]
    async fn distill_whitespace_only_response_returns_empty_summary_error() {
        let engine = default_engine();
        let messages = sample_conversation();
        let provider = MockProvider::empty_text_blocks();

        let result = engine.distill(&messages, "test-nous", &provider, 1).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty summary"));
    }

    // --- config defaults ---

    #[test]
    fn config_default_model() {
        let config = DistillConfig::default();
        assert_eq!(config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn config_default_values() {
        let config = DistillConfig::default();
        assert_eq!(config.max_output_tokens, 4096);
        assert_eq!(config.min_messages, 6);
        assert!(config.include_tool_calls);
    }

    // --- estimate_tokens ---

    #[test]
    fn estimate_tokens_chars_div_4() {
        let messages = vec![text_msg(Role::User, "abcdefgh")]; // 8 chars → 2 tokens
        assert_eq!(estimate_tokens(&messages), 2);
    }

    #[test]
    fn estimate_tokens_rounds_up() {
        let messages = vec![text_msg(Role::User, "abcde")]; // 5 chars → ceil(5/4) = 2
        assert_eq!(estimate_tokens(&messages), 2);
    }

    #[test]
    fn estimate_tokens_empty_messages() {
        let messages: Vec<Message> = vec![];
        assert_eq!(estimate_tokens(&messages), 0);
    }

    // --- extract_summary_text ---

    #[test]
    fn extract_summary_from_text_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Part 1".to_owned(),
            },
            ContentBlock::Text {
                text: "Part 2".to_owned(),
            },
        ];
        let text = extract_summary_text(&blocks);
        assert_eq!(text, "Part 1\nPart 2");
    }

    #[test]
    fn extract_summary_skips_non_text_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Summary text".to_owned(),
            },
            ContentBlock::Thinking {
                thinking: "internal thought".to_owned(),
            },
        ];
        let text = extract_summary_text(&blocks);
        assert_eq!(text, "Summary text");
    }

    #[test]
    fn extract_summary_trims_whitespace() {
        let blocks = vec![ContentBlock::Text {
            text: "  summary  ".to_owned(),
        }];
        let text = extract_summary_text(&blocks);
        assert_eq!(text, "summary");
    }
}
