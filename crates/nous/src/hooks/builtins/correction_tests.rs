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
    let corrections = vec![CorrectionRecord::new(
        "test-agent",
        "ses-test",
        1,
        "Always use snafu",
        "Always use snafu",
    )];
    let section = format_corrections_section(&corrections);
    assert!(section.contains("Operator Corrections"));
    assert!(section.contains("1. Always use snafu"));
}

#[test]
fn formats_multiple_corrections() {
    let corrections = vec![
        CorrectionRecord::new(
            "test-agent",
            "ses-test",
            1,
            "Never use unwrap",
            "source one",
        ),
        CorrectionRecord::new(
            "test-agent",
            "ses-test",
            1,
            "Always run tests",
            "source two",
        ),
    ];
    let section = format_corrections_section(&corrections);
    assert!(section.contains("1. Never use unwrap"));
    assert!(section.contains("2. Always run tests"));
}

// -- Persistence tests --

#[tokio::test]
async fn load_returns_empty_when_no_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let corrections = load_corrections(dir.path(), "test-agent", "ses-test")
        .await
        .expect("load");
    assert!(
        corrections.is_empty(),
        "should return empty vec for missing file"
    );
}

#[tokio::test]
async fn append_and_load_roundtrip() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let correction = CorrectionRecord::new(
        "test-agent",
        "ses-test",
        1,
        "Always use snafu",
        "Always use snafu for errors",
    );

    persist_correction(dir.path(), correction)
        .await
        .expect("persist");

    let loaded = load_corrections(dir.path(), "test-agent", "ses-test")
        .await
        .expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text, "Always use snafu");
    assert_eq!(loaded[0].nous_id, "test-agent");
    assert_eq!(loaded[0].session_id, "ses-test");
    assert!(!loaded[0].source_hash.is_empty());
}

#[tokio::test]
async fn append_multiple_corrections() {
    let dir = tempfile::tempdir().expect("create temp dir");

    for i in 0_u64..3 {
        let correction = CorrectionRecord::new(
            "test-agent",
            "ses-test",
            i,
            format!("Correction {i}"),
            format!("source {i}"),
        );
        persist_correction(dir.path(), correction)
            .await
            .expect("persist");
    }

    let loaded = load_corrections(dir.path(), "test-agent", "ses-test")
        .await
        .expect("load");
    assert_eq!(loaded.len(), 3);
}

#[tokio::test]
async fn evicts_oldest_when_over_cap() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let max_corrections = AgentBehaviorDefaults::default().corrections_max_corrections;

    // Write max_corrections + 5 corrections.
    for i in 0..max_corrections + 5 {
        let correction = CorrectionRecord::new(
            "test-agent",
            "ses-test",
            1,
            format!("Correction {i}"),
            format!("source {i}"),
        );
        persist_correction(dir.path(), correction)
            .await
            .expect("persist");
    }

    let loaded = load_corrections(dir.path(), "test-agent", "ses-test")
        .await
        .expect("load");
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

#[tokio::test]
async fn replay_dedupe_by_source_hash_and_scope() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let source = "Always use jiff for time";

    let first = CorrectionRecord::new("nous-a", "ses-1", 1, "Use jiff", source);
    let replay_same_scope = CorrectionRecord::new("nous-a", "ses-1", 2, "Use jiff", source);
    let other_scope = CorrectionRecord::new("nous-b", "ses-1", 1, "Use jiff", source);

    persist_correction(dir.path(), first)
        .await
        .expect("persist first");
    persist_correction(dir.path(), replay_same_scope)
        .await
        .expect("persist replay");
    persist_correction(dir.path(), other_scope)
        .await
        .expect("persist other scope");

    let all = load_all_records(dir.path()).await.expect("load all");
    assert_eq!(all.len(), 2, "duplicate source hash/scope is skipped");

    let scoped = load_corrections(dir.path(), "nous-a", "ses-1")
        .await
        .expect("load scoped");
    assert_eq!(scoped.len(), 1);
}

#[tokio::test]
async fn scope_filtering_loads_only_matching_records() {
    let dir = tempfile::tempdir().expect("create temp dir");

    let a = CorrectionRecord::new("nous-a", "ses-1", 1, "Text A", "source A");
    let b = CorrectionRecord::new("nous-a", "ses-2", 1, "Text B", "source B");
    let c = CorrectionRecord::new("nous-b", "ses-1", 1, "Text C", "source C");

    persist_correction(dir.path(), a).await.expect("persist a");
    persist_correction(dir.path(), b).await.expect("persist b");
    persist_correction(dir.path(), c).await.expect("persist c");

    let loaded = load_corrections(dir.path(), "nous-a", "ses-1")
        .await
        .expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text, "Text A");
    assert_eq!(loaded[0].nous_id, "nous-a");
    assert_eq!(loaded[0].session_id, "ses-1");
}

#[tokio::test]
async fn atomic_write_uses_temp_and_rename() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let records = vec![CorrectionRecord::new("nous", "ses", 1, "Text", "source")];
    let path = corrections_path(dir.path());

    write_corrections_atomic(&path, &records)
        .await
        .expect("atomic write");

    let content = tokio::fs::read_to_string(&path).await.expect("read");
    let parsed: Vec<CorrectionRecord> = serde_json::from_str(&content).expect("parse");
    assert_eq!(parsed.len(), 1);

    // NOTE: atomic writes should leave only the final corrections file behind.
    let tmp_files: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{CORRECTIONS_FILENAME}.tmp"))
        })
        .collect();
    assert!(tmp_files.is_empty(), "temp files must be cleaned up");
}

#[test]
fn status_transition_bumps_revision() {
    let mut record = CorrectionRecord::new("nous", "ses", 1, "Text", "source");
    assert_eq!(record.status, CorrectionStatus::Active);
    assert_eq!(record.revision, 0);

    record.transition_to(CorrectionStatus::Dismissed);
    assert_eq!(record.status, CorrectionStatus::Dismissed);
    assert_eq!(record.revision, 1);

    record.transition_to(CorrectionStatus::Active);
    assert_eq!(record.status, CorrectionStatus::Active);
    assert_eq!(record.revision, 2);

    // NOTE: transition_to is idempotent for the current status.
    record.transition_to(CorrectionStatus::Active);
    assert_eq!(record.revision, 2);
}

// -- Hook integration tests --

#[tokio::test]
async fn injector_appends_to_system_prompt() {
    let dir = tempfile::tempdir().expect("create temp dir");

    // Pre-populate corrections file.
    let correction =
        CorrectionRecord::new("test-agent", "ses-test", 1, "Always use snafu", "source");
    persist_correction(dir.path(), correction)
        .await
        .expect("persist");

    let hook = CorrectionInjector::new(dir.path().to_path_buf());

    let mut pipeline = crate::pipeline::PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..crate::pipeline::PipelineContext::default()
    };
    let mut ctx = crate::hooks::QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-test",
        turn_number: 1,
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
        session_id: "ses-test",
        turn_number: 1,
        user_message: "Never use unwrap in production code",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue);

    // Verify the correction was persisted.
    let loaded = load_corrections(dir.path(), "test-agent", "ses-test")
        .await
        .expect("load");
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
        session_id: "ses-test",
        turn_number: 1,
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
    let correction =
        CorrectionRecord::new("test-agent", "ses-test", 1, "Always use snafu", "source");
    persist_correction(dir.path(), correction)
        .await
        .expect("persist");

    let hook = CorrectionInjector::new(dir.path().to_path_buf());

    let mut pipeline = crate::pipeline::PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 1, // barely any budget
        ..crate::pipeline::PipelineContext::default()
    };
    let mut ctx = crate::hooks::QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-test",
        turn_number: 1,
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
async fn injection_skips_dismissed_records() {
    let dir = tempfile::tempdir().expect("create temp dir");

    let active = CorrectionRecord::new("nous-a", "ses-1", 1, "Active rule", "source active");
    let mut dismissed =
        CorrectionRecord::new("nous-a", "ses-1", 1, "Dismissed rule", "source dismissed");
    dismissed.transition_to(CorrectionStatus::Dismissed);

    persist_correction(dir.path(), active)
        .await
        .expect("persist active");
    persist_correction(dir.path(), dismissed)
        .await
        .expect("persist dismissed");

    let hook = CorrectionInjector::new(dir.path().to_path_buf());

    let mut pipeline = crate::pipeline::PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..crate::pipeline::PipelineContext::default()
    };
    let mut ctx = crate::hooks::QueryContext {
        pipeline: &mut pipeline,
        nous_id: "nous-a",
        session_id: "ses-1",
        turn_number: 2,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue);

    let prompt = ctx.pipeline.system_prompt.as_ref().expect("prompt");
    assert!(prompt.contains("Active rule"));
    assert!(
        !prompt.contains("Dismissed rule"),
        "dismissed corrections must not be injected"
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
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        provider_used: None,
        tool_surface_hashes: Vec::new(),
    };
    let ctx = crate::hooks::TurnContext {
        result: &turn_result,
        nous_id: "test-agent",
        session_id: "ses-test",
        turn_number: 1,
        session_tokens: 0,
        reinject_identity: false,
    };

    let result = hook.on_turn_complete(&ctx).await;
    assert_eq!(result, HookResult::Continue);
}
