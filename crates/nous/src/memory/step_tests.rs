//! Tests for the structured Step model and `assemble_steps`.

#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
)]

use crate::memory::step::{Observation, Step};
use crate::pipeline::{PipelineMessage, assemble_steps};

#[test]
fn step_token_estimate_sums_all_parts() {
    let step = Step {
        self_note: "a".repeat(40), // 10 tokens
        observations: vec![
            Observation::new("tool1", "b".repeat(20)), // 5 tokens
            Observation::new("tool2", "c".repeat(40)), // 10 tokens
        ],
        summary: Some("d".repeat(12)), // 3 tokens
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.token_estimate(),
        28,
        "token estimate should sum note (10) + obs (5+10) + summary (3)"
    );
}

#[test]
fn step_token_estimate_empty_is_zero() {
    let step = Step {
        self_note: String::new(),
        observations: Vec::new(),
        summary: None,
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.token_estimate(),
        0,
        "empty step should have zero tokens"
    );
}

#[test]
fn step_compact_with_observations_and_summary() {
    let step = Step {
        self_note: "plan".to_owned(),
        observations: vec![Observation::new("bash", "output")],
        summary: Some("fallback".to_owned()),
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.compact(),
        "plan | fallback",
        "compact should include self_note and summary when observations exist"
    );
}

#[test]
fn step_compact_with_observations_no_summary() {
    let step = Step {
        self_note: "plan".to_owned(),
        observations: vec![Observation::new("bash", "output")],
        summary: None,
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.compact(),
        "plan",
        "compact should return just self_note when no summary"
    );
}

#[test]
fn step_compact_without_observations() {
    let step = Step {
        self_note: "plan".to_owned(),
        observations: Vec::new(),
        summary: Some("ignored".to_owned()),
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.compact(),
        "plan",
        "compact should ignore summary when there are no observations"
    );
}

#[test]
fn step_compact_empty_everything() {
    let step = Step {
        self_note: String::new(),
        observations: Vec::new(),
        summary: None,
        index: 0,
        started_at: jiff::Timestamp::now(),
    };
    assert_eq!(
        step.compact(),
        "",
        "compact of fully empty step should be empty string"
    );
}

#[test]
fn observation_new_computes_token_estimate() {
    let obs = Observation::new("file_read", "x".repeat(100));
    assert_eq!(obs.token_estimate, 25, "100 chars / 4 = 25 tokens");
}

#[test]
fn observation_new_rounds_up() {
    let obs = Observation::new("bash", "xxx");
    assert_eq!(obs.token_estimate, 1, "3 chars / 4 rounds up to 1 token");
}

#[test]
fn assemble_steps_empty_stream() {
    let messages: Vec<PipelineMessage> = Vec::new();
    let steps = assemble_steps(&messages);
    assert!(
        steps.is_empty(),
        "empty message stream should yield no steps"
    );
}

#[test]
fn assemble_steps_single_assistant_no_tools() {
    let messages = vec![PipelineMessage::text("assistant", "hello", 10)];
    let steps = assemble_steps(&messages);
    assert_eq!(
        steps.len(),
        1,
        "single assistant message should yield one step"
    );
    assert_eq!(steps[0].self_note, "hello");
    assert!(steps[0].observations.is_empty());
}

#[test]
fn assemble_steps_groups_tools_with_preceding_assistant() {
    let messages = vec![
        PipelineMessage::text("assistant", "I will read the file", 20),
        PipelineMessage::text(
            "user",
            "[tool:file_read@2024-01-01T00:00:00Z] file content here",
            100,
        ),
        PipelineMessage::text("user", "[tool:bash@2024-01-01T00:00:01Z] ls output", 50),
        PipelineMessage::text("assistant", "Done", 10),
    ];
    let steps = assemble_steps(&messages);
    assert_eq!(steps.len(), 2, "should produce two steps");
    assert_eq!(steps[0].self_note, "I will read the file");
    assert_eq!(
        steps[0].observations.len(),
        2,
        "first step should have two observations"
    );
    assert_eq!(steps[0].observations[0].source, "file_read");
    assert_eq!(steps[0].observations[1].source, "bash");
    assert_eq!(steps[1].self_note, "Done");
    assert!(steps[1].observations.is_empty());
}

#[test]
fn assemble_steps_user_message_acts_as_boundary() {
    let messages = vec![
        PipelineMessage::text("assistant", "first", 10),
        PipelineMessage::text("user", "regular user message", 5),
        PipelineMessage::text("assistant", "second", 10),
    ];
    let steps = assemble_steps(&messages);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].self_note, "first");
    assert_eq!(steps[1].self_note, "second");
}

#[test]
fn assemble_steps_orphan_tool_attaches_to_previous_step() {
    let messages = vec![
        PipelineMessage::text("assistant", "plan", 10),
        PipelineMessage::text("user", "[tool:bash@2024-01-01T00:00:00Z] output 1", 20),
        // A regular user message acts as a boundary
        PipelineMessage::text("user", "follow-up", 5),
        // Orphan tool result with no preceding assistant in this turn
        PipelineMessage::text("user", "[tool:bash@2024-01-01T00:00:01Z] output 2", 20),
    ];
    let steps = assemble_steps(&messages);
    // First step gets the first tool, then user message finalizes it.
    // The orphan tool attaches to the most recent step (the first one).
    assert_eq!(steps.len(), 1, "only one assistant message existed");
    assert_eq!(
        steps[0].observations.len(),
        2,
        "orphan tool result should attach to previous step"
    );
    assert_eq!(steps[0].observations[1].source, "bash");
}

#[test]
fn assemble_steps_multiline_self_note_preserved() {
    let note = "line one\nline two\nline three";
    let messages = vec![PipelineMessage::text("assistant", note, 30)];
    let steps = assemble_steps(&messages);
    assert_eq!(
        steps[0].self_note, note,
        "multiline self_note should be preserved verbatim"
    );
}

#[test]
fn assemble_steps_large_observation_body_no_panic() {
    let big_body = "x".repeat(10_000_000);
    let messages = vec![
        PipelineMessage::text("assistant", "plan", 10),
        PipelineMessage::text(
            "user",
            format!("[tool:file_read@2024-01-01T00:00:00Z] {big_body}"),
            2_500_000,
        ),
    ];
    let steps = assemble_steps(&messages);
    assert!(
        steps[0].observations[0].token_estimate > 0,
        "large body token estimate should be positive and not panic"
    );
}

#[test]
fn assemble_steps_indices_are_sequential() {
    let messages = vec![
        PipelineMessage::text("assistant", "a", 1),
        PipelineMessage::text("assistant", "b", 1),
        PipelineMessage::text("assistant", "c", 1),
    ];
    let steps = assemble_steps(&messages);
    assert_eq!(steps[0].index, 0);
    assert_eq!(steps[1].index, 1);
    assert_eq!(steps[2].index, 2);
}
