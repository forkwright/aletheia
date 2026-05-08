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
    let messages = vec![PipelineMessage {
        role: "assistant".to_owned(),
        content: "hello".to_owned(),
        token_estimate: 10,
        cache_breakpoint: false,
    }];
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
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "I will read the file".to_owned(),
            token_estimate: 20,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "user".to_owned(),
            content: "[tool:file_read@2024-01-01T00:00:00Z] file content here".to_owned(),
            token_estimate: 100,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "user".to_owned(),
            content: "[tool:bash@2024-01-01T00:00:01Z] ls output".to_owned(),
            token_estimate: 50,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "Done".to_owned(),
            token_estimate: 10,
            cache_breakpoint: false,
        },
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
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "first".to_owned(),
            token_estimate: 10,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "user".to_owned(),
            content: "regular user message".to_owned(),
            token_estimate: 5,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "second".to_owned(),
            token_estimate: 10,
            cache_breakpoint: false,
        },
    ];
    let steps = assemble_steps(&messages);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].self_note, "first");
    assert_eq!(steps[1].self_note, "second");
}

#[test]
fn assemble_steps_orphan_tool_attaches_to_previous_step() {
    let messages = vec![
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "plan".to_owned(),
            token_estimate: 10,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "user".to_owned(),
            content: "[tool:bash@2024-01-01T00:00:00Z] output 1".to_owned(),
            token_estimate: 20,
            cache_breakpoint: false,
        },
        // A regular user message acts as a boundary
        PipelineMessage {
            role: "user".to_owned(),
            content: "follow-up".to_owned(),
            token_estimate: 5,
            cache_breakpoint: false,
        },
        // Orphan tool result with no preceding assistant in this turn
        PipelineMessage {
            role: "user".to_owned(),
            content: "[tool:bash@2024-01-01T00:00:01Z] output 2".to_owned(),
            token_estimate: 20,
            cache_breakpoint: false,
        },
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
    let messages = vec![PipelineMessage {
        role: "assistant".to_owned(),
        content: note.to_owned(),
        token_estimate: 30,
        cache_breakpoint: false,
    }];
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
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "plan".to_owned(),
            token_estimate: 10,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "user".to_owned(),
            content: format!("[tool:file_read@2024-01-01T00:00:00Z] {big_body}"),
            token_estimate: 2_500_000,
            cache_breakpoint: false,
        },
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
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "a".to_owned(),
            token_estimate: 1,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "b".to_owned(),
            token_estimate: 1,
            cache_breakpoint: false,
        },
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "c".to_owned(),
            token_estimate: 1,
            cache_breakpoint: false,
        },
    ];
    let steps = assemble_steps(&messages);
    assert_eq!(steps[0].index, 0);
    assert_eq!(steps[1].index, 1);
    assert_eq!(steps[2].index, 2);
}
