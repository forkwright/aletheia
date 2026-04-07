//! Correction hooks: capture operator corrections and inject them into future turns.
//!
//! Two hooks work together:
//! - [`CorrectionDetector`]: runs in `on_turn_complete` to scan for correction patterns
//!   in user messages and persist them to a JSON file.
//! - [`CorrectionInjector`]: runs in `before_query` to read persisted corrections
//!   and append them to the system prompt.
//!
//! Storage format: `<workspace>/corrections.json` — a JSON array of [`Correction`] entries.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::hooks::{HookResult, QueryContext, TurnContext, TurnHook};

/// Maximum number of corrections retained per agent.
///
/// WHY: Unbounded corrections would bloat the system prompt. 50 covers
/// realistic operator usage; oldest are evicted when the cap is reached.
const MAX_CORRECTIONS: usize = 50;

/// Filename for the corrections store within the agent workspace.
const CORRECTIONS_FILENAME: &str = "corrections.json";

/// A persisted behavioral correction from the operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Correction {
    /// The extracted correction text.
    pub text: String,
    /// ISO 8601 timestamp when the correction was recorded.
    pub created_at: String,
    /// The original user message that triggered the correction.
    pub source_message: String,
}

/// Detects correction patterns in user messages and persists them.
///
/// Runs in `on_turn_complete` (after the LLM responds) so the detector
/// sees the full user message that was just processed. Detection uses
/// keyword matching for correction-intent phrases.
pub(crate) struct CorrectionDetector {
    /// Path to the agent workspace directory.
    ///
    /// WHY: Reserved for future use when the detector gains the ability to
    /// extract corrections from multi-turn patterns in `on_turn_complete`.
    #[expect(dead_code, reason = "reserved for future on_turn_complete correction extraction")]
    workspace: PathBuf,
}

impl CorrectionDetector {
    /// Create a new correction detector that stores corrections in the given workspace.
    pub(crate) fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl TurnHook for CorrectionDetector {
    fn name(&self) -> &'static str {
        "correction_detector"
    }

    fn on_turn_complete<'a>(
        &'a self,
        context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            // NOTE: This hook is currently a no-op; correction detection is handled
            // by CorrectionInjector in before_query. This function is retained for
            // future expansion if turn-complete analysis is needed.
            // user message and persists them immediately. The detector hook is
            // kept as a secondary path for corrections embedded in multi-turn
            // context.
            //
            // For the primary detection path, see CorrectionInjector::before_query.

            debug!(
                nous_id = context.nous_id,
                "correction_detector: turn complete, no-op in on_turn_complete"
            );

            HookResult::Continue
        })
    }
}

/// Reads persisted corrections and injects them into the system prompt.
///
/// Runs in `before_query` (before the model call). Also detects new corrections
/// from the current user message and persists them before injection.
///
/// WHY: Combined detect+inject in `before_query` because:
/// 1. [`QueryContext`] has the user message ([`TurnContext`] does not)
/// 2. Corrections from this turn should apply starting from this turn
/// 3. Single file read/write per turn instead of two
pub(crate) struct CorrectionInjector {
    /// Path to the agent workspace directory.
    workspace: PathBuf,
}

impl CorrectionInjector {
    /// Create a new correction injector that reads corrections from the given workspace.
    pub(crate) fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl TurnHook for CorrectionInjector {
    fn name(&self) -> &'static str {
        "correction_injector"
    }

    fn before_query<'a>(
        &'a self,
        context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            let user_message = context.user_message;

            // Step 1: Detect and persist any new correction in the user message.
            if let Some(correction_text) = extract_correction(user_message) {
                debug!(
                    nous_id = context.nous_id,
                    correction = correction_text.as_str(),
                    "correction_injector: detected correction in user message"
                );

                let correction = Correction {
                    text: correction_text,
                    created_at: jiff::Timestamp::now().to_string(),
                    source_message: truncate_source(user_message),
                };

                if let Err(e) = append_correction(&self.workspace, correction).await {
                    warn!(
                        nous_id = context.nous_id,
                        error = %e,
                        "correction_injector: failed to persist correction"
                    );
                    // WHY: Non-fatal — continue without persisting. The correction
                    // still applies this turn via the system prompt injection below.
                }
            }

            // Step 2: Read all persisted corrections and inject into system prompt.
            let corrections = match load_corrections(&self.workspace).await {
                Ok(c) => c,
                Err(e) => {
                    debug!(
                        nous_id = context.nous_id,
                        error = %e,
                        "correction_injector: no corrections file or read error, skipping injection"
                    );
                    return HookResult::Continue;
                }
            };

            if corrections.is_empty() {
                return HookResult::Continue;
            }

            // Build the corrections section for the system prompt.
            let section = format_corrections_section(&corrections);
            let token_estimate = section.len() / 4; // conservative: ~4 chars per token

            // WHY: Check remaining token budget before injecting. Corrections
            // should not crowd out conversation history.
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "usize→i64: token estimate fits in i64 for practical prompt sizes"
            )]
            let estimate_i64 = token_estimate as i64; // kanon:ignore RUST/as-cast
            if context.pipeline.remaining_tokens < estimate_i64 * 2 {
                debug!(
                    nous_id = context.nous_id,
                    remaining = context.pipeline.remaining_tokens,
                    correction_tokens = token_estimate,
                    "correction_injector: skipping injection, insufficient token budget"
                );
                return HookResult::Continue;
            }

            // Append corrections to the system prompt.
            if let Some(ref mut prompt) = context.pipeline.system_prompt {
                prompt.push_str("\n\n");
                prompt.push_str(&section);
            }

            context.pipeline.remaining_tokens -= estimate_i64;

            debug!(
                nous_id = context.nous_id,
                correction_count = corrections.len(),
                token_estimate,
                "correction_injector: injected corrections into system prompt"
            );

            HookResult::Continue
        })
    }
}

// -- Correction detection --

/// Phrases that indicate the user is issuing a behavioral correction.
///
/// WHY: Simple keyword matching is intentionally conservative. False negatives
/// (missed corrections) are preferable to false positives (storing random
/// sentences as corrections). The operator can always re-state a correction.
const CORRECTION_PREFIXES: &[&str] = &[
    "don't ",
    "do not ",
    "stop ",
    "never ",
    "always ",
    "from now on",
    "remember to ",
    "make sure to ",
    "please don't ",
    "please do not ",
    "please always ",
    "please never ",
    "you should always ",
    "you should never ",
    "you must always ",
    "you must never ",
    "i need you to always ",
    "i need you to never ",
];

/// Extract a correction from a user message, if one is detected.
///
/// Returns `Some(correction_text)` if the message contains a correction pattern,
/// or `None` if no correction is detected.
fn extract_correction(message: &str) -> Option<String> {
    let lower = message.to_lowercase();

    // WHY: Check each sentence independently. A multi-sentence message might
    // contain a correction in one sentence and a question in another.
    for sentence in split_sentences(message) {
        let sentence_lower = sentence.to_lowercase();
        let trimmed = sentence_lower.trim();

        for prefix in CORRECTION_PREFIXES {
            if trimmed.starts_with(prefix) {
                // Use the original-case sentence as the correction text.
                return Some(sentence.trim().to_owned());
            }
        }

        // WHY: Also check for mid-sentence correction patterns like
        // "I want you to never X" or "going forward, always Y".
        if (lower.contains("going forward") || lower.contains("in the future"))
            && (trimmed.contains("always ") || trimmed.contains("never "))
        {
            return Some(sentence.trim().to_owned());
        }
    }

    None
}

/// Split text into sentences on common delimiters.
///
/// WHY: Lightweight sentence splitting — no NLP dependency. Handles `.`, `!`, `?`
/// followed by whitespace or end-of-string. Good enough for correction detection.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;

    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            let end = i + c.len_utf8();
            // WHY: char_indices yields valid byte boundaries, so these slices
            // are guaranteed safe. get() satisfies clippy's indexing lint.
            let rest = text.get(end..).unwrap_or("");
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                if let Some(sentence) = text.get(start..end)
                    && !sentence.trim().is_empty()
                {
                    sentences.push(sentence);
                }
                start = end;
            }
        }
    }

    // Capture trailing text without terminal punctuation.
    if let Some(remainder) = text.get(start..)
        && !remainder.trim().is_empty()
    {
        sentences.push(remainder);
    }

    sentences
}

/// Truncate the source message for storage. Keeps approximately the first 200 characters.
///
/// WHY: Uses `char_indices` to find a char boundary near 200 bytes to avoid
/// panicking on multi-byte UTF-8 characters.
fn truncate_source(message: &str) -> String {
    if message.len() <= 200 {
        return message.to_owned();
    }

    // Find the last char boundary at or before byte 200.
    let boundary = message
        .char_indices()
        .take_while(|(i, _)| *i <= 200)
        .last()
        .map_or(0, |(i, _)| i);

    let mut s = message.get(..boundary).unwrap_or(message).to_owned();
    s.push_str("...");
    s
}

// -- Persistence --

/// Path to the corrections file within a workspace.
fn corrections_path(workspace: &Path) -> PathBuf {
    workspace.join(CORRECTIONS_FILENAME)
}

/// Load corrections from the workspace file.
///
/// Returns an empty vec if the file does not exist. Returns an error only
/// on actual I/O or parse failures.
async fn load_corrections(workspace: &Path) -> Result<Vec<Correction>, std::io::Error> {
    let path = corrections_path(workspace);

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let corrections: Vec<Correction> =
                serde_json::from_str(&content).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid corrections JSON: {e}"),
                    )
                })?;
            Ok(corrections)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Append a correction to the workspace file, enforcing the max cap.
async fn append_correction(
    workspace: &Path,
    correction: Correction,
) -> Result<(), std::io::Error> {
    let path = corrections_path(workspace);

    // Ensure the workspace directory exists.
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut corrections = match load_corrections(workspace).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };

    corrections.push(correction);

    // WHY: Evict oldest corrections when over the cap. Operator's most recent
    // corrections are more likely to be relevant.
    if corrections.len() > MAX_CORRECTIONS {
        let excess = corrections.len() - MAX_CORRECTIONS;
        corrections.drain(..excess);
    }

    let json = serde_json::to_string_pretty(&corrections).map_err(|e| {
        std::io::Error::other(format!("failed to serialize corrections: {e}"))
    })?;

    tokio::fs::write(&path, json).await
}

// -- System prompt formatting --

/// Format corrections into a system prompt section.
fn format_corrections_section(corrections: &[Correction]) -> String {
    let mut section = String::from(
        "## Operator Corrections\n\n\
         The following behavioral corrections have been recorded by the operator. \
         Follow these instructions exactly:\n\n",
    );

    for (i, correction) in corrections.iter().enumerate() {
        use std::fmt::Write as _;
        let _ = writeln!(section, "{}. {}", i + 1, correction.text);
    }

    section
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions on known-length collections")]
mod tests {
    use super::*;

    // -- extract_correction tests --

    #[test]
    fn detects_dont_prefix() {
        let result = extract_correction("Don't use emojis in commit messages");
        assert!(result.is_some(), "should detect 'don't' prefix");
        assert_eq!(
            result.expect("correction"),
            "Don't use emojis in commit messages"
        );
    }

    #[test]
    fn detects_do_not_prefix() {
        let result = extract_correction("Do not create README files");
        assert!(result.is_some(), "should detect 'do not' prefix");
    }

    #[test]
    fn detects_always_prefix() {
        let result = extract_correction("Always use snafu for error handling");
        assert!(result.is_some(), "should detect 'always' prefix");
    }

    #[test]
    fn detects_never_prefix() {
        let result = extract_correction("Never push directly to main");
        assert!(result.is_some(), "should detect 'never' prefix");
    }

    #[test]
    fn detects_from_now_on() {
        let result = extract_correction("From now on, use jiff instead of chrono");
        assert!(result.is_some(), "should detect 'from now on' prefix");
    }

    #[test]
    fn detects_stop_prefix() {
        let result = extract_correction("Stop adding comments to every line");
        assert!(result.is_some(), "should detect 'stop' prefix");
    }

    #[test]
    fn detects_please_dont() {
        let result = extract_correction("Please don't use unwrap in production code");
        assert!(result.is_some(), "should detect 'please don't' prefix");
    }

    #[test]
    fn detects_remember_to() {
        let result = extract_correction("Remember to run clippy before committing");
        assert!(result.is_some(), "should detect 'remember to' prefix");
    }

    #[test]
    fn ignores_non_correction() {
        let result = extract_correction("What does this function do?");
        assert!(result.is_none(), "should not detect correction in question");
    }

    #[test]
    fn ignores_empty_message() {
        let result = extract_correction("");
        assert!(result.is_none(), "should not detect correction in empty string");
    }

    #[test]
    fn detects_correction_in_multi_sentence() {
        let result =
            extract_correction("That looks good. Always use snake_case for variable names.");
        assert!(
            result.is_some(),
            "should detect correction in second sentence"
        );
        assert_eq!(
            result.expect("correction"),
            "Always use snake_case for variable names."
        );
    }

    #[test]
    fn detects_going_forward_pattern() {
        let result = extract_correction("Going forward, always validate inputs before processing");
        assert!(result.is_some(), "should detect 'going forward' + 'always'");
    }

    // -- split_sentences tests --

    #[test]
    fn splits_on_period() {
        let sentences = split_sentences("First sentence. Second sentence.");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn splits_on_question_mark() {
        let sentences = split_sentences("Is this a test? Yes it is.");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn handles_no_punctuation() {
        let sentences = split_sentences("No punctuation here");
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0], "No punctuation here");
    }

    #[test]
    fn handles_trailing_text() {
        let sentences = split_sentences("First. Second without period");
        assert_eq!(sentences.len(), 2);
    }

    // -- truncate_source tests --

    #[test]
    fn short_message_unchanged() {
        let msg = "short message";
        assert_eq!(truncate_source(msg), msg);
    }

    #[test]
    fn long_message_truncated() {
        let msg = "a".repeat(300);
        let result = truncate_source(&msg);
        assert_eq!(result.len(), 203); // 200 + "..."
        assert!(result.ends_with("..."));
    }

    // -- format_corrections_section tests --

    #[test]
    fn formats_single_correction() {
        let corrections = vec![Correction {
            text: "Always use snafu".to_owned(),
            created_at: "2026-04-06T00:00:00Z".to_owned(),
            source_message: "Always use snafu".to_owned(),
        }];
        let section = format_corrections_section(&corrections);
        assert!(section.contains("Operator Corrections"));
        assert!(section.contains("1. Always use snafu"));
    }

    #[test]
    fn formats_multiple_corrections() {
        let corrections = vec![
            Correction {
                text: "Never use unwrap".to_owned(),
                created_at: "2026-04-06T00:00:00Z".to_owned(),
                source_message: "source".to_owned(),
            },
            Correction {
                text: "Always run tests".to_owned(),
                created_at: "2026-04-06T00:01:00Z".to_owned(),
                source_message: "source".to_owned(),
            },
        ];
        let section = format_corrections_section(&corrections);
        assert!(section.contains("1. Never use unwrap"));
        assert!(section.contains("2. Always run tests"));
    }

    // -- Persistence tests --

    #[tokio::test]
    async fn load_returns_empty_when_no_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let corrections = load_corrections(dir.path()).await.expect("load");
        assert!(corrections.is_empty(), "should return empty vec for missing file");
    }

    #[tokio::test]
    async fn append_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let correction = Correction {
            text: "Always use snafu".to_owned(),
            created_at: "2026-04-06T00:00:00Z".to_owned(),
            source_message: "Always use snafu for errors".to_owned(),
        };

        append_correction(dir.path(), correction)
            .await
            .expect("append");

        let loaded = load_corrections(dir.path()).await.expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text, "Always use snafu");
    }

    #[tokio::test]
    async fn append_multiple_corrections() {
        let dir = tempfile::tempdir().expect("create temp dir");

        for i in 0..3 {
            let correction = Correction {
                text: format!("Correction {i}"),
                created_at: format!("2026-04-06T00:0{i}:00Z"),
                source_message: format!("source {i}"),
            };
            append_correction(dir.path(), correction)
                .await
                .expect("append");
        }

        let loaded = load_corrections(dir.path()).await.expect("load");
        assert_eq!(loaded.len(), 3);
    }

    #[tokio::test]
    async fn evicts_oldest_when_over_cap() {
        let dir = tempfile::tempdir().expect("create temp dir");

        // Write MAX_CORRECTIONS + 5 corrections.
        for i in 0..MAX_CORRECTIONS + 5 {
            let correction = Correction {
                text: format!("Correction {i}"),
                created_at: format!("2026-04-06T00:00:{i:02}Z"),
                source_message: format!("source {i}"),
            };
            append_correction(dir.path(), correction)
                .await
                .expect("append");
        }

        let loaded = load_corrections(dir.path()).await.expect("load");
        assert_eq!(
            loaded.len(),
            MAX_CORRECTIONS,
            "should cap at MAX_CORRECTIONS"
        );
        // Oldest corrections (0-4) should be evicted; newest should remain.
        assert_eq!(loaded[0].text, "Correction 5");
        assert_eq!(
            loaded[MAX_CORRECTIONS - 1].text,
            format!("Correction {}", MAX_CORRECTIONS + 4)
        );
    }

    // -- Hook integration tests --

    #[tokio::test]
    async fn injector_appends_to_system_prompt() {
        let dir = tempfile::tempdir().expect("create temp dir");

        // Pre-populate corrections file.
        let correction = Correction {
            text: "Always use snafu".to_owned(),
            created_at: "2026-04-06T00:00:00Z".to_owned(),
            source_message: "source".to_owned(),
        };
        append_correction(dir.path(), correction)
            .await
            .expect("append");

        let hook = CorrectionInjector::new(dir.path().to_path_buf());

        let mut pipeline = crate::pipeline::PipelineContext {
            system_prompt: Some("Base prompt.".to_owned()),
            remaining_tokens: 100_000,
            ..crate::pipeline::PipelineContext::default()
        };
        let mut ctx = crate::hooks::QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
            user_message: "hello",
        };

        let result = hook.before_query(&mut ctx).await;
        assert_eq!(result, HookResult::Continue);

        let prompt = ctx.pipeline.system_prompt.as_ref().expect("system prompt");
        assert!(
            prompt.contains("Operator Corrections"),
            "system prompt should contain corrections section"
        );
        assert!(
            prompt.contains("Always use snafu"),
            "system prompt should contain the correction text"
        );
    }

    #[tokio::test]
    async fn injector_detects_and_persists_new_correction() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let hook = CorrectionInjector::new(dir.path().to_path_buf());

        let mut pipeline = crate::pipeline::PipelineContext {
            system_prompt: Some("Base prompt.".to_owned()),
            remaining_tokens: 100_000,
            ..crate::pipeline::PipelineContext::default()
        };
        let mut ctx = crate::hooks::QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
            user_message: "Never use unwrap in production code",
        };

        let result = hook.before_query(&mut ctx).await;
        assert_eq!(result, HookResult::Continue);

        // Verify the correction was persisted.
        let loaded = load_corrections(dir.path()).await.expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text, "Never use unwrap in production code");

        // Verify it was also injected into the system prompt.
        let prompt = ctx.pipeline.system_prompt.as_ref().expect("system prompt");
        assert!(prompt.contains("Never use unwrap in production code"));
    }

    #[tokio::test]
    async fn injector_skips_when_no_corrections() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let hook = CorrectionInjector::new(dir.path().to_path_buf());

        let mut pipeline = crate::pipeline::PipelineContext {
            system_prompt: Some("Base prompt.".to_owned()),
            remaining_tokens: 100_000,
            ..crate::pipeline::PipelineContext::default()
        };
        let mut ctx = crate::hooks::QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
            user_message: "What does this function do?",
        };

        let result = hook.before_query(&mut ctx).await;
        assert_eq!(result, HookResult::Continue);

        // System prompt should be unchanged.
        assert_eq!(
            ctx.pipeline.system_prompt.as_ref().expect("prompt"),
            "Base prompt."
        );
    }

    #[tokio::test]
    async fn injector_skips_when_insufficient_budget() {
        let dir = tempfile::tempdir().expect("create temp dir");

        // Pre-populate with a correction.
        let correction = Correction {
            text: "Always use snafu".to_owned(),
            created_at: "2026-04-06T00:00:00Z".to_owned(),
            source_message: "source".to_owned(),
        };
        append_correction(dir.path(), correction)
            .await
            .expect("append");

        let hook = CorrectionInjector::new(dir.path().to_path_buf());

        let mut pipeline = crate::pipeline::PipelineContext {
            system_prompt: Some("Base prompt.".to_owned()),
            remaining_tokens: 1, // barely any budget
            ..crate::pipeline::PipelineContext::default()
        };
        let mut ctx = crate::hooks::QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
            user_message: "hello",
        };

        let result = hook.before_query(&mut ctx).await;
        assert_eq!(result, HookResult::Continue);

        // System prompt should be unchanged due to insufficient budget.
        assert_eq!(
            ctx.pipeline.system_prompt.as_ref().expect("prompt"),
            "Base prompt."
        );
    }

    #[tokio::test]
    async fn detector_returns_continue() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let hook = CorrectionDetector::new(dir.path().to_path_buf());

        let turn_result = crate::pipeline::TurnResult {
            content: "test response".to_owned(),
            tool_calls: Vec::new(),
            usage: crate::pipeline::TurnUsage::default(),
            signals: Vec::new(),
            stop_reason: "end_turn".to_owned(),
            degraded: None,
        };
        let ctx = crate::hooks::TurnContext {
            result: &turn_result,
            nous_id: "test-agent",
            session_tokens: 0,
        };

        let result = hook.on_turn_complete(&ctx).await;
        assert_eq!(result, HookResult::Continue);
    }
}
