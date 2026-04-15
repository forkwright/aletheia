#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]

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
    assert!(
        result.is_none(),
        "should not detect correction in empty string"
    );
}

#[test]
fn detects_correction_in_multi_sentence() {
    let result = extract_correction("That looks good. Always use snake_case for variable names.");
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
    assert!(
        corrections.is_empty(),
        "should return empty vec for missing file"
    );
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
    let max_corrections = AgentBehaviorDefaults::default().corrections_max_corrections;

    // Write max_corrections + 5 corrections.
    for i in 0..max_corrections + 5 {
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
        max_corrections,
        "should cap at max_corrections"
    );
    // Oldest corrections (0-4) should be evicted; newest should remain.
    assert_eq!(loaded[0].text, "Correction 5");
    assert_eq!(
        loaded[max_corrections - 1].text,
        format!("Correction {}", max_corrections + 4)
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
