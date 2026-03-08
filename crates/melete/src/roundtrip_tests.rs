//! Roundtrip tests verifying distillation preserves critical context.

use std::sync::Mutex;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{
    CompletionRequest, CompletionResponse, Content, ContentBlock, Message, Role, StopReason, Usage,
};

use crate::distill::{DistillConfig, DistillEngine, DistillSection};

struct MockProvider {
    response: Mutex<Option<aletheia_hermeneus::error::Result<CompletionResponse>>>,
}

impl MockProvider {
    fn with_summary(summary: &str) -> Self {
        Self {
            response: Mutex::new(Some(Ok(CompletionResponse {
                id: "msg_roundtrip".to_owned(),
                model: "claude-sonnet-4-20250514".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: summary.to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 5000,
                    output_tokens: 50,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            }))),
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
            .expect("lock") // INVARIANT: test mock, panic = test bug
            .take()
            .expect("mock provider called more than once")
    }

    fn supported_models(&self) -> &[&str] {
        &["claude-sonnet-4-20250514"]
    }

    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "mock-roundtrip"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
    }
}

fn default_engine() -> DistillEngine {
    DistillEngine::new(DistillConfig::default())
}

const FULL_SUMMARY: &str = "\
## Summary
Fixed login bug and added tool-based database migration.

## Task Context
Working on auth module bug fix for nous agent \"syn\".

## Completed Work
- Fixed null check on line 42 of src/auth/login.rs
- Ran database migration tool: migrate_db({\"version\": \"v2\"})
- Added regression test for login flow

## Key Decisions
- Decision: Add null check rather than restructure auth flow. Reason: Minimal invasive fix.
- Decision: Use v2 schema for migration. Reason: Backwards compatible.

## Current State
Bug is fixed, migration applied, all tests passing.

## Open Threads
- Performance audit of login endpoint deferred to next sprint

## Corrections
- CORRECTION: Initially looked at wrong file (session.rs), actually the bug was in login.rs";

#[tokio::test]
async fn distill_preserves_tool_results() {
    let messages = vec![
        text_msg(Role::User, "Run the database migration tool"),
        text_msg(
            Role::Assistant,
            "I'll run the migration tool: migrate_db({\"version\": \"v2\"})",
        ),
        text_msg(Role::User, "What was the result?"),
        text_msg(
            Role::Assistant,
            "Migration completed successfully. 3 tables updated.",
        ),
        text_msg(Role::User, "Great, verify it"),
        text_msg(Role::Assistant, "Verification passed."),
        text_msg(Role::User, "Thanks"),
        text_msg(Role::Assistant, "You're welcome."),
        text_msg(Role::User, "Any issues?"),
        text_msg(Role::Assistant, "None found."),
    ];
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    assert!(result.summary.contains("migrate_db"));
    assert!(result.summary.contains("database migration"));
}

#[tokio::test]
async fn distill_preserves_decisions() {
    let messages = vec![
        text_msg(Role::User, "Should we restructure auth or just patch it?"),
        text_msg(
            Role::Assistant,
            "Decision: Add null check rather than restructure. Reason: Minimal fix.",
        ),
        text_msg(Role::User, "Ok do it"),
        text_msg(Role::Assistant, "Done."),
        text_msg(Role::User, "What about schema version?"),
        text_msg(
            Role::Assistant,
            "Decision: Use v2 schema. Reason: Backwards compatible.",
        ),
        text_msg(Role::User, "Apply it"),
        text_msg(Role::Assistant, "Applied."),
        text_msg(Role::User, "Verify"),
        text_msg(Role::Assistant, "Verified."),
    ];
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    assert!(result.summary.contains("Decision: Add null check"));
    assert!(result.summary.contains("Decision: Use v2 schema"));
}

#[tokio::test]
async fn distill_preserves_corrections() {
    let messages = vec![
        text_msg(Role::User, "Check session.rs for the bug"),
        text_msg(
            Role::Assistant,
            "Looking at session.rs... actually the bug is in login.rs. CORRECTION: wrong file.",
        ),
        text_msg(Role::User, "Fix it in login.rs then"),
        text_msg(Role::Assistant, "Fixed in login.rs."),
        text_msg(Role::User, "Good"),
        text_msg(Role::Assistant, "All done."),
        text_msg(Role::User, "Test it"),
        text_msg(Role::Assistant, "Tests pass."),
        text_msg(Role::User, "Ship it"),
        text_msg(Role::Assistant, "Shipped."),
    ];
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    assert!(result.summary.contains("CORRECTION"));
    assert!(result.summary.contains("login.rs"));
}

#[tokio::test]
async fn distill_reduces_token_count() {
    let messages = vec![
        text_msg(
            Role::User,
            "Help me fix this long complicated bug in the authentication system that spans multiple files",
        ),
        text_msg(
            Role::Assistant,
            "I'll investigate the authentication system. Let me check the auth module, session handler, and login flow for potential issues.",
        ),
        text_msg(Role::User, "The error is in the null check path"),
        text_msg(
            Role::Assistant,
            "Found it — there's a missing null check on line 42 of src/auth/login.rs. The session token can be null when the user's cookie expires mid-request.",
        ),
        text_msg(Role::User, "Fix it and add a test"),
        text_msg(
            Role::Assistant,
            "Done. Added the null check and wrote a regression test that verifies the login flow handles expired cookies gracefully.",
        ),
        text_msg(Role::User, "Run the tests"),
        text_msg(
            Role::Assistant,
            "All tests pass including the new regression test.",
        ),
        text_msg(Role::User, "Great work"),
        text_msg(
            Role::Assistant,
            "Thanks! The fix is minimal and backwards compatible.",
        ),
    ];
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    assert!(
        result.tokens_after < result.tokens_before,
        "tokens_after ({}) should be less than tokens_before ({})",
        result.tokens_after,
        result.tokens_before
    );
}

#[tokio::test]
async fn distill_handles_empty_session() {
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine.distill(&[], "syn", &provider, 1).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no messages"));
}

#[tokio::test]
async fn distill_handles_single_turn() {
    let messages = vec![
        text_msg(Role::User, "Hello"),
        text_msg(Role::Assistant, "Hi there!"),
    ];

    let summary = "## Summary\nGreeting exchange.";
    let provider = MockProvider::with_summary(summary);

    let config = DistillConfig {
        verbatim_tail: 3,
        min_messages: 1,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    // With 2 messages and verbatim_tail=3, all go to verbatim (split_at=0)
    assert_eq!(result.verbatim_messages.len(), 2);
    assert_eq!(result.messages_distilled, 0);
}

#[tokio::test]
async fn distill_handles_long_input() {
    let mut messages = Vec::new();
    for i in 0..24 {
        messages.push(text_msg(
            if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            &format!(
                "Message {i} with some content to make it longer for token estimation purposes."
            ),
        ));
    }

    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    // 24 messages - 3 verbatim_tail = 21 distilled
    assert_eq!(result.messages_distilled, 21);
    assert_eq!(result.verbatim_messages.len(), 3);
    assert!(result.tokens_before > 0);
}

#[tokio::test]
async fn distill_verbatim_tail_preserves_recent() {
    let messages = vec![
        text_msg(Role::User, "First message"),
        text_msg(Role::Assistant, "Second message"),
        text_msg(Role::User, "Third message"),
        text_msg(Role::Assistant, "Fourth message"),
        text_msg(Role::User, "Fifth message"),
        text_msg(Role::Assistant, "Sixth message"),
        text_msg(Role::User, "Seventh — recent"),
        text_msg(Role::Assistant, "Eighth — recent"),
        text_msg(Role::User, "Ninth — recent"),
        text_msg(Role::Assistant, "Tenth — most recent"),
    ];

    let config = DistillConfig {
        verbatim_tail: 3,
        ..DistillConfig::default()
    };
    let engine = DistillEngine::new(config);
    let provider = MockProvider::with_summary(FULL_SUMMARY);

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    assert_eq!(result.verbatim_messages.len(), 3);
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Eighth — recent"
    );
    assert_eq!(result.verbatim_messages[1].content.text(), "Ninth — recent");
    assert_eq!(
        result.verbatim_messages[2].content.text(),
        "Tenth — most recent"
    );
}

#[tokio::test]
async fn distill_summary_contains_all_sections() {
    let messages = vec![
        text_msg(Role::User, "Help me fix the bug"),
        text_msg(Role::Assistant, "Working on it"),
        text_msg(Role::User, "Status?"),
        text_msg(Role::Assistant, "Almost done"),
        text_msg(Role::User, "Ship it"),
        text_msg(Role::Assistant, "Done"),
        text_msg(Role::User, "Verify"),
        text_msg(Role::Assistant, "Verified"),
        text_msg(Role::User, "Thanks"),
        text_msg(Role::Assistant, "Welcome"),
    ];
    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    for section in DistillSection::all_standard() {
        let heading = section.heading();
        assert!(
            result.summary.contains(&heading),
            "summary missing section: {heading}"
        );
    }
}

#[tokio::test]
async fn distill_roundtrip_message_content_integrity() {
    let messages = vec![
        text_msg(Role::User, "Alpha"),
        text_msg(Role::Assistant, "Bravo"),
        text_msg(Role::User, "Charlie"),
        text_msg(Role::Assistant, "Delta"),
        text_msg(Role::User, "Echo"),
        text_msg(Role::Assistant, "Foxtrot"),
        text_msg(Role::User, "Golf — preserved"),
        text_msg(Role::Assistant, "Hotel — preserved"),
        text_msg(Role::User, "India — preserved"),
    ];

    let provider = MockProvider::with_summary(FULL_SUMMARY);
    let engine = default_engine();

    let result = engine
        .distill(&messages, "syn", &provider, 1)
        .await
        .expect("distill");

    // Last 3 messages are verbatim tail
    assert_eq!(result.verbatim_messages.len(), 3);
    assert_eq!(
        result.verbatim_messages[0].content.text(),
        "Golf — preserved"
    );
    assert_eq!(
        result.verbatim_messages[1].content.text(),
        "Hotel — preserved"
    );
    assert_eq!(
        result.verbatim_messages[2].content.text(),
        "India — preserved"
    );

    // Verify roles are preserved
    assert_eq!(result.verbatim_messages[0].role, Role::User);
    assert_eq!(result.verbatim_messages[1].role, Role::Assistant);
    assert_eq!(result.verbatim_messages[2].role, Role::User);
}
