#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on a known-length collection"
)]

use super::*;

/// Helper: build a default `CaptureInput` for a normal successful turn.
/// Tests override individual fields to exercise specific gate conditions.
fn good_input() -> CaptureInput<'static> {
    CaptureInput {
        session_id: "ses-1",
        nous_id: "syn",
        user_message: "Hello",
        assistant_response: "Hi there!",
        model: "test-model",
        provider: Some("test-provider"),
        tokens: 150,
        cost_usd: Some(0.001),
        provider_duration_ms: 42,
        stop_reason: CaptureStopReason::EndTurn,
        has_tool_calls: false,
        turn_type: None,
        is_correction: None,
        fact_types: None,
        tool_outcomes: None,
        recall_signals: None,
        tool_surface_hashes: &[],
        turn_id: None,
        turn_seq: 0,
        capture_policy_ref: None,
        finalization_status: Some("finalized"),
    }
}

/// Build a default `TrainingConfig` with PII filtering disabled.
///
/// WHY disabled: most of these tests use literal strings like "Hello"
/// that never match any PII pattern, but a few use values that could
/// trip the redactor. Disabling keeps assertions focused on shard /
/// manifest behaviour. Dedicated PII tests below exercise the filter
/// explicitly.
fn test_config_no_pii(path: &str, max_shard_bytes: u64) -> TrainingConfig {
    TrainingConfig {
        enabled: true,
        path: path.to_owned(),
        max_shard_bytes,
        pii_filter_enabled: false,
        decontamination_policy: DecontaminationPolicy::Disabled,
        author_classifier_threshold: 0.85,
    }
}

#[test]
fn training_config_defaults() {
    let config = TrainingConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.path, "data/training");
    assert_eq!(config.max_shard_bytes, 50 * 1024 * 1024);
    assert!(config.pii_filter_enabled);
}

// WHY(#5385): `training.path` is validated at config load
// (`taxis::validate`), but `TrainingCapture::new` re-checks independently
// so a caller that builds `TrainingConfig` directly — like these tests, or
// a future non-validated call path — cannot write outside the instance
// root just because the config-load gate did not run.

#[test]
fn new_rejects_absolute_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("/etc/aletheia-training", 50 * 1024 * 1024);

    let err = TrainingCapture::new(dir.path(), &config)
        .expect_err("absolute training.path must be rejected");
    assert!(
        matches!(err, TrainingCaptureError::PathEscapesRoot { .. }),
        "expected PathEscapesRoot, got: {err:?}"
    );
}

#[test]
fn new_rejects_dotdot_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training/../../escape", 50 * 1024 * 1024);

    let err = TrainingCapture::new(dir.path(), &config)
        .expect_err("'..' in training.path must be rejected");
    assert!(
        matches!(err, TrainingCaptureError::PathEscapesRoot { .. }),
        "expected PathEscapesRoot, got: {err:?}"
    );
}

#[test]
fn new_rejects_empty_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("", 50 * 1024 * 1024);

    let err = TrainingCapture::new(dir.path(), &config)
        .expect_err("empty training.path must be rejected");
    assert!(
        matches!(err, TrainingCaptureError::PathEscapesRoot { .. }),
        "expected PathEscapesRoot, got: {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn new_rejects_symlink_escape() {
    // WHY: a plain relative `config.path` (e.g. "training") passes the
    // pre-join string check, but if that name is a symlink pointing outside
    // the instance root, only the post-creation canonicalize re-check
    // catches it.
    let root = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");

    std::os::unix::fs::symlink(outside.path(), root.path().join("training"))
        .expect("create symlink");

    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let err = TrainingCapture::new(root.path(), &config)
        .expect_err("symlink escaping the instance root must be rejected");
    assert!(
        matches!(err, TrainingCaptureError::PathEscapesRoot { .. }),
        "expected PathEscapesRoot, got: {err:?}"
    );
}

#[test]
fn training_capture_writes_jsonl() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let record = TrainingRecord {
        schema_version: TRAINING_RECORD_SCHEMA_VERSION,
        session_id: "ses-1".to_owned(),
        nous_id: "syn".to_owned(),
        user_message: "Hello".to_owned(),
        assistant_response: "Hi there!".to_owned(),
        model: "claude-opus-4-20250514".to_owned(),
        provider: Some("anthropic".to_owned()),
        tokens: 150,
        cost_usd: Some(0.002),
        provider_duration_ms: 500,
        timestamp: Timestamp::UNIX_EPOCH,
        turn_type: Some("discussion".to_owned()),
        is_correction: Some(false),
        fact_types: None,
        quality_score: Some(0.9),
        quality_score_formula_version: Some(QUALITY_SCORE_FORMULA_VERSION),
        quality_score_components: None,
        tool_outcomes: None,
        recall_signals: None,
        tool_surface_hashes: Vec::new(),
        pii_redacted: false,
        pii_filter_applied: false,
        pii_redaction_count: 0,
        pii_policy_ref: None,
        decontamination_policy: None,
        decontamination_verdict: None,
        classifier_version: None,
    };
    capture.write_record(&record).expect("write");

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 1);

    let parsed: TrainingRecord = serde_json::from_str(lines[0]).expect("parse");
    assert_eq!(parsed.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
    assert_eq!(parsed.session_id, "ses-1");
    assert_eq!(parsed.nous_id, "syn");
    assert_eq!(parsed.user_message, "Hello");
    assert_eq!(parsed.assistant_response, "Hi there!");
    assert_eq!(parsed.tokens, 150);
    assert_eq!(parsed.turn_type, Some("discussion".to_owned()));
    assert_eq!(parsed.quality_score, Some(0.9));
}

#[test]
fn training_capture_appends() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    for i in 0..3 {
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: format!("ses-{i}"),
            nous_id: "syn".to_owned(),
            user_message: format!("msg-{i}"),
            assistant_response: format!("resp-{i}"),
            model: "test-model".to_owned(),
            provider: None,
            tokens: 100,
            cost_usd: None,
            provider_duration_ms: 0,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            quality_score_formula_version: None,
            quality_score_components: None,
            tool_outcomes: None,
            recall_signals: None,
            tool_surface_hashes: Vec::new(),
            pii_redacted: false,
            pii_filter_applied: false,
            pii_redaction_count: 0,
            pii_policy_ref: None,
            decontamination_policy: None,
            decontamination_verdict: None,
            classifier_version: None,
        };
        capture.write_record(&record).expect("write");
    }

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);

    assert_eq!(capture.manifest().total_records, 3);
    assert_eq!(capture.manifest().shards.len(), 1);
    assert_eq!(capture.manifest().shards[0].record_count, 3);
}

// -- Shard rotation -------------------------------------------------------

#[test]
fn shard_rotation_on_size_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    // WHY: tiny limit forces rotation after ~1 record
    let config = test_config_no_pii("training", 100);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    for i in 0..5 {
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: format!("ses-{i}"),
            nous_id: "syn".to_owned(),
            user_message: format!("message number {i} with some content"),
            assistant_response: format!("response number {i} with some content"),
            model: "test-model".to_owned(),
            provider: None,
            tokens: 100,
            cost_usd: None,
            provider_duration_ms: 0,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            quality_score_formula_version: None,
            quality_score_components: None,
            tool_outcomes: None,
            recall_signals: None,
            tool_surface_hashes: Vec::new(),
            pii_redacted: false,
            pii_filter_applied: false,
            pii_redaction_count: 0,
            pii_policy_ref: None,
            decontamination_policy: None,
            decontamination_verdict: None,
            classifier_version: None,
        };
        capture.write_record(&record).expect("write");
    }

    assert!(
        capture.manifest().shards.len() > 1,
        "expected multiple shards, got {}",
        capture.manifest().shards.len()
    );
    assert_eq!(capture.manifest().total_records, 5);

    for shard in &capture.manifest().shards {
        let shard_path = dir.path().join("training").join(&shard.file_name);
        assert!(
            shard_path.exists(),
            "shard {} should exist",
            shard.file_name
        );
    }
}

// -- Backward compatibility: legacy file -----------------------------------

#[test]
fn legacy_conversations_jsonl_adopted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let training_dir = dir.path().join("training");
    fs::create_dir_all(&training_dir).expect("mkdir");

    let legacy_path = training_dir.join("conversations.jsonl");
    let record_json = r#"{"session_id":"old-1","nous_id":"syn","user_message":"hi","assistant_response":"hello","model":"test","tokens":10,"timestamp":"1970-01-01T00:00:00Z"}"#;
    {
        use std::io::Write;
        // WHY OpenOptions over fs::write: `std::fs::write` is disallowed
        // by project lint config in favour of explicit create-truncate.
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&legacy_path)
            .expect("open legacy");
        f.write_all(format!("{record_json}\n{record_json}\n").as_bytes())
            .expect("write legacy");
    }

    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let capture = TrainingCapture::new(dir.path(), &config).expect("new");

    assert!(
        capture
            .manifest()
            .shards
            .iter()
            .any(|s| s.file_name == "conversations.jsonl"),
        "legacy file should be in manifest"
    );
    assert_eq!(capture.manifest().total_records, 2);
    // Legacy records have schema v0 (missing field defaults to 0)
    assert_eq!(capture.manifest().schema_version_min, 0);
}

// -- Manifest persistence --------------------------------------------------

#[test]
fn manifest_persisted_atomically() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    capture.maybe_capture(&good_input());

    let manifest_path = dir.path().join("training").join("training-manifest.json");
    assert!(manifest_path.exists(), "manifest file should exist");

    let content = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: TrainingManifest = serde_json::from_str(&content).expect("parse manifest");
    assert_eq!(manifest.total_records, 1);
}

// -- Quality gate: empty / whitespace -----------------------------------------

#[test]
fn quality_gate_rejects_empty_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        assistant_response: "",
        ..good_input()
    });
    assert!(!captured, "empty response should be rejected");
}

#[test]
fn quality_gate_rejects_whitespace_only_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    for ws in ["  ", "\n", "\t\n  ", "   \n\n   "] {
        let captured = capture.maybe_capture(&CaptureInput {
            assistant_response: ws,
            ..good_input()
        });
        assert!(
            !captured,
            "whitespace-only response {ws:?} should be rejected"
        );
    }
}

// -- Quality gate: stop reasons -----------------------------------------------

#[test]
fn quality_gate_rejects_max_tokens_stop_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        stop_reason: CaptureStopReason::MaxTokens,
        ..good_input()
    });
    assert!(!captured, "max_tokens stop reason should be rejected");
}

#[test]
fn quality_gate_rejects_degraded_stop_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        stop_reason: CaptureStopReason::Degraded,
        ..good_input()
    });
    assert!(!captured, "degraded stop reason should be rejected");
}

#[test]
fn quality_gate_rejects_unknown_stop_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        stop_reason: CaptureStopReason::Unknown,
        ..good_input()
    });
    assert!(!captured, "unknown stop reason should be rejected");
}

#[test]
fn quality_gate_rejects_content_filtered_stop_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        stop_reason: CaptureStopReason::ContentFiltered,
        ..good_input()
    });
    assert!(
        !captured,
        "content_filtered stop reason should be rejected from training capture"
    );
}

// -- Quality gate: tool-use-only ----------------------------------------------

#[test]
fn quality_gate_rejects_tool_use_only_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        assistant_response: "Let me check that.",
        stop_reason: CaptureStopReason::ToolUse,
        has_tool_calls: true,
        ..good_input()
    });
    assert!(
        !captured,
        "tool-use-only turn (tool_use stop + has_tool_calls) should be rejected"
    );
}

#[test]
fn quality_gate_accepts_tool_use_with_end_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        assistant_response: "Based on the file contents, here is the answer.",
        stop_reason: CaptureStopReason::EndTurn,
        has_tool_calls: true,
        ..good_input()
    });
    assert!(
        captured,
        "tool-using turn that ended with text should be accepted"
    );
}

#[test]
fn quality_gate_rejects_correction_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "Actually that's incorrect, the value is 42.",
        assistant_response: "You are right, I apologize for the error.",
        is_correction: Some(true),
        ..good_input()
    });
    assert!(
        !captured,
        "a correction turn pairs correction-shaped input with an acknowledgement; \
         capturing it trains sycophancy"
    );
}

#[test]
fn quality_gate_accepts_non_correction_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        is_correction: Some(false),
        ..good_input()
    });
    assert!(
        captured,
        "an explicitly non-correction turn must still be captured: the gate keys on \
         Some(true), not on the flag being present"
    );
}

#[test]
fn correction_turn_writes_no_record_to_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // WHY capture an ordinary turn first: it proves the corpus file exists and
    // the writer is live, so the absence of the correction record below cannot be
    // an artifact of nothing having been written at all.
    capture.maybe_capture(&CaptureInput {
        assistant_response: "The value is 41.",
        ..good_input()
    });
    capture.maybe_capture(&CaptureInput {
        user_message: "No, that's wrong.",
        assistant_response: "You are right, I apologize.",
        is_correction: Some(true),
        ..good_input()
    });

    // WHY assert on the corpus rather than the return value: the defect was that
    // the penalty was recorded in metadata while the record was written anyway,
    // so the only honest check is what actually reached the corpus.
    let written = std::fs::read_to_string(capture.file_path()).expect("read corpus");
    assert!(
        written.contains("The value is 41."),
        "precondition: the ordinary turn should be in the corpus: {written}"
    );
    assert!(
        !written.contains("I apologize"),
        "correction turn reached the training corpus: {written}"
    );
}

// -- Quality gate: happy path -------------------------------------------------

#[test]
fn quality_gate_accepts_good_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&good_input());
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    assert_eq!(content.lines().count(), 1);
}

#[test]
fn quality_gate_accepts_stop_sequence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        stop_reason: CaptureStopReason::StopSequence,
        ..good_input()
    });
    assert!(captured, "stop_sequence with content should be accepted");
}

// -- Episteme labels ----------------------------------------------------------

#[test]
fn capture_preserves_episteme_labels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // WHY not a correction turn: this test covers episteme-label round-tripping,
    // and correction turns are rejected outright by the quality gate (#5822), so
    // they can never reach the corpus to have their labels checked. `Some(false)`
    // still supplies the is_correction signal the quality score needs.
    let captured = capture.maybe_capture(&CaptureInput {
        turn_type: Some("fact_capture".to_owned()),
        is_correction: Some(false),
        fact_types: Some(vec!["preference".to_owned(), "identity".to_owned()]),
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let parsed: TrainingRecord =
        serde_json::from_str(content.lines().next().expect("line")).expect("parse");
    assert_eq!(parsed.turn_type, Some("fact_capture".to_owned()));
    assert_eq!(parsed.is_correction, Some(false));
    assert_eq!(
        parsed.fact_types,
        Some(vec!["preference".to_owned(), "identity".to_owned()])
    );
    // WHY: the turn supplies an is_correction signal, so a quality score
    // must be present.
    assert!(parsed.quality_score.is_some());
}

// -- Quality score computation ------------------------------------------------

#[test]
fn quality_score_computed_from_tool_success() {
    let input = CaptureInput {
        tool_outcomes: Some(vec![
            ToolOutcome {
                name: "file_read".to_owned(),
                success: true,
                duration_ms: 10,
                error_kind: None,
            },
            ToolOutcome {
                name: "file_write".to_owned(),
                success: true,
                duration_ms: 5,
                error_kind: None,
            },
        ]),
        ..good_input()
    };
    let score = input.compute_quality_score().expect("some score");
    // All tools succeeded → tool component contributes 0.40. Stop
    // reason EndTurn contributes 0.10. Substance for "Hi there!"
    // contributes a small amount (~0.0045 at 9 chars / 400).
    assert!((0.50..=0.60).contains(&score), "score = {score}");
}

#[test]
fn quality_score_penalises_tool_failure() {
    let success_input = CaptureInput {
        tool_outcomes: Some(vec![ToolOutcome {
            name: "shell".to_owned(),
            success: true,
            duration_ms: 10,
            error_kind: None,
        }]),
        ..good_input()
    };
    let failure_input = CaptureInput {
        tool_outcomes: Some(vec![ToolOutcome {
            name: "shell".to_owned(),
            success: false,
            duration_ms: 10,
            error_kind: Some("timeout".to_owned()),
        }]),
        ..good_input()
    };
    let s = success_input.compute_quality_score().expect("some");
    let f = failure_input.compute_quality_score().expect("some");
    assert!(s > f, "success ({s}) should score above failure ({f})");
}

#[test]
fn quality_score_none_when_no_signals() {
    // Trivial text with no signals at all → None.
    let input = CaptureInput {
        assistant_response: "ok",
        ..good_input()
    };
    assert!(input.compute_quality_score().is_none());
}

#[test]
fn quality_score_rewards_recall_reference() {
    let base_recall = RecallSignals {
        candidates_found: 3,
        results_injected: 2,
        tokens_consumed: 50,
        facts: vec![
            RecalledFact {
                source_id: "f1".to_owned(),
                source_type: "fact".to_owned(),
                score: 0.9,
                was_referenced: true,
            },
            RecalledFact {
                source_id: "f2".to_owned(),
                source_type: "fact".to_owned(),
                score: 0.8,
                was_referenced: true,
            },
        ],
    };
    let mut unref = base_recall.clone();
    for f in &mut unref.facts {
        f.was_referenced = false;
    }

    let referenced = CaptureInput {
        recall_signals: Some(base_recall),
        ..good_input()
    };
    let unreferenced = CaptureInput {
        recall_signals: Some(unref),
        ..good_input()
    };
    let r = referenced.compute_quality_score().expect("some");
    let u = unreferenced.compute_quality_score().expect("some");
    assert!(r > u, "referenced ({r}) should exceed unreferenced ({u})");
}

/// Build the `RecallSignals` the pipeline actually emits today.
///
/// WHY: every other recall test in this file populates `facts` by hand,
/// so none of them reaches the shape production runs on — the pipeline
/// hardcodes `facts: Vec::new()` under a `WHY(#3418)` comment. The two
/// tests below pin the empty-`facts` path specifically.
fn pipeline_shaped_recall() -> RecallSignals {
    RecallSignals {
        candidates_found: 3,
        results_injected: 2,
        tokens_consumed: 50,
        facts: Vec::new(),
    }
}

#[test]
fn quality_score_none_when_recall_has_no_per_fact_data() {
    // Recall fired and injected results, but `facts` is empty, so the
    // utilization rate has nothing to measure. It must not count as a
    // signal: with no other signal present the score is None, not a
    // Some(_) whose recall component is a structural zero.
    let input = CaptureInput {
        recall_signals: Some(pipeline_shaped_recall()),
        ..good_input()
    };
    assert!(
        input.compute_quality_score().is_none(),
        "results_injected > 0 with empty facts must not claim a recall signal"
    );
}

#[test]
fn quality_score_ignores_empty_recall_beside_a_real_signal() {
    // The guard must suppress only the recall component, not the record.
    // A turn carrying a genuine signal still scores, and scores exactly
    // as it would with no recall attached at all.
    let tool_outcomes = Some(vec![ToolOutcome {
        name: "shell".to_owned(),
        success: true,
        duration_ms: 10,
        error_kind: None,
    }]);
    let with_empty_recall = CaptureInput {
        tool_outcomes: tool_outcomes.clone(),
        recall_signals: Some(pipeline_shaped_recall()),
        ..good_input()
    };
    let without_recall = CaptureInput {
        tool_outcomes,
        ..good_input()
    };

    let with = with_empty_recall.compute_quality_score().expect("some");
    let without = without_recall.compute_quality_score().expect("some");
    assert!(
        (with - without).abs() < f32::EPSILON,
        "empty recall facts must contribute nothing ({with} vs {without})"
    );
}

// -- PII redaction --------------------------------------------------------

#[test]
fn pii_filter_redacts_user_message_when_enabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: true,
        decontamination_policy: DecontaminationPolicy::Disabled,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "my email is leaky@example.com please help",
        assistant_response: "Sure, I'll help.",
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let parsed: TrainingRecord =
        serde_json::from_str(content.lines().next().expect("line")).expect("parse");
    assert!(!parsed.user_message.contains("leaky@example.com"));
    assert!(parsed.user_message.contains("[REDACTED:email]"));
    assert!(parsed.pii_redacted);
    assert!(parsed.pii_filter_applied);
    assert_eq!(parsed.pii_redaction_count, 1);
    assert_eq!(parsed.pii_policy_ref.as_deref(), Some(pii::POLICY_REF));
}

#[test]
fn pii_filter_preserves_clean_content_with_screening_provenance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: true,
        decontamination_policy: DecontaminationPolicy::Disabled,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "tell me a joke",
        assistant_response: "Why did the Rust compiler cross the road? To borrow check.",
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let parsed: TrainingRecord =
        serde_json::from_str(content.lines().next().expect("line")).expect("parse");
    assert!(!parsed.pii_redacted);
    assert!(parsed.pii_filter_applied);
    assert_eq!(parsed.pii_redaction_count, 0);
    assert_eq!(parsed.pii_policy_ref.as_deref(), Some(pii::POLICY_REF));
    assert_eq!(parsed.user_message, "tell me a joke");
}

#[test]
fn pii_filter_disabled_passes_through() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: false,
        decontamination_policy: DecontaminationPolicy::Disabled,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "contact: risky@example.com",
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let parsed: TrainingRecord =
        serde_json::from_str(content.lines().next().expect("line")).expect("parse");
    assert!(parsed.user_message.contains("risky@example.com"));
    assert!(!parsed.pii_redacted);
    assert!(!parsed.pii_filter_applied);
    assert_eq!(parsed.pii_redaction_count, 0);
    assert!(parsed.pii_policy_ref.is_none());
}

#[test]
fn pii_policy_ref_serializes_when_filter_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: true,
        decontamination_policy: DecontaminationPolicy::Disabled,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "plain text",
        assistant_response: "plain response",
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let line = content.lines().next().expect("line");
    assert!(line.contains("\"pii_filter_applied\":true"));
    assert!(line.contains("\"pii_redaction_count\":0"));
    assert!(line.contains("\"pii_policy_ref\":\"nous-training-pii-v1\""));
}

// -- CaptureStopReason parsing ------------------------------------------------

#[test]
fn capture_stop_reason_from_str() {
    assert_eq!(
        CaptureStopReason::parse("end_turn"),
        CaptureStopReason::EndTurn
    );
    assert_eq!(
        CaptureStopReason::parse("tool_use"),
        CaptureStopReason::ToolUse
    );
    assert_eq!(
        CaptureStopReason::parse("max_tokens"),
        CaptureStopReason::MaxTokens
    );
    assert_eq!(
        CaptureStopReason::parse("stop_sequence"),
        CaptureStopReason::StopSequence
    );
    assert_eq!(
        CaptureStopReason::parse("degraded"),
        CaptureStopReason::Degraded
    );
    assert_eq!(
        CaptureStopReason::parse("content_filtered"),
        CaptureStopReason::ContentFiltered
    );
    assert_eq!(
        CaptureStopReason::parse("error"),
        CaptureStopReason::Unknown
    );
    assert_eq!(
        CaptureStopReason::parse("anything_else"),
        CaptureStopReason::Unknown
    );
}

// -- Serde roundtrip ----------------------------------------------------------

#[test]
fn training_record_serde_roundtrip() {
    let record = TrainingRecord {
        schema_version: TRAINING_RECORD_SCHEMA_VERSION,
        session_id: "ses-1".to_owned(),
        nous_id: "syn".to_owned(),
        user_message: "test input".to_owned(),
        assistant_response: "test output".to_owned(),
        model: "claude-opus-4-20250514".to_owned(),
        provider: Some("anthropic".to_owned()),
        tokens: 200,
        cost_usd: Some(0.05),
        provider_duration_ms: 1200,
        timestamp: Timestamp::UNIX_EPOCH,
        turn_type: Some("planning".to_owned()),
        is_correction: None,
        fact_types: Some(vec!["skill".to_owned()]),
        quality_score: None,
        quality_score_formula_version: None,
        quality_score_components: None,
        tool_outcomes: None,
        recall_signals: None,
        tool_surface_hashes: Vec::new(),
        pii_redacted: false,
        pii_filter_applied: false,
        pii_redaction_count: 0,
        pii_policy_ref: None,
        decontamination_policy: None,
        decontamination_verdict: None,
        classifier_version: None,
    };

    let json = serde_json::to_string(&record).expect("serialize");
    let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
    assert_eq!(back.session_id, record.session_id);
    assert_eq!(back.tokens, record.tokens);
    assert_eq!(back.turn_type, Some("planning".to_owned()));
    assert!(back.is_correction.is_none());
}

// -- Authorship gate ---------------------------------------------------------

fn test_config_with_classifier(path: &str, max_shard_bytes: u64) -> TrainingConfig {
    TrainingConfig {
        enabled: true,
        path: path.to_owned(),
        max_shard_bytes,
        pii_filter_enabled: false,
        decontamination_policy: DecontaminationPolicy::FailClosed,
        author_classifier_threshold: 0.85,
    }
}

#[test]
fn authorship_gate_rejects_agent_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_with_classifier("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "As an AI language model, I don't have personal experiences.",
        assistant_response: "Understood.",
        ..good_input()
    });
    assert!(
        !captured,
        "agent-authored user message should be rejected by authorship gate"
    );
}

#[test]
fn authorship_gate_accepts_human_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_with_classifier("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "lol thanks for the help! can you check this?",
        assistant_response: "Sure, I'll take a look.",
        ..good_input()
    });
    assert!(
        captured,
        "human-authored user message should pass authorship gate"
    );

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    assert_eq!(content.lines().count(), 1);
}

#[test]
fn authorship_gate_disabled_is_noop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // Even agent-looking text is captured when the gate is disabled.
    let captured = capture.maybe_capture(&CaptureInput {
        user_message: "As an AI language model, I don't have personal experiences.",
        assistant_response: "Understood.",
        ..good_input()
    });
    assert!(
        captured,
        "authorship gate disabled: agent text should be captured"
    );

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    assert_eq!(content.lines().count(), 1);
}

// -- Training capture is ML corpus, not audit ledger -------------------------

#[cfg(test)]
mod audit_separation_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn training_capture_does_not_represent_failure_modes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config_no_pii("training", 50 * 1024 * 1024);

        for (label, stop_reason, has_tool_calls) in [
            ("max_tokens", CaptureStopReason::MaxTokens, false),
            ("degraded", CaptureStopReason::Degraded, false),
            (
                "content_filtered",
                CaptureStopReason::ContentFiltered,
                false,
            ),
            ("tool_use_only", CaptureStopReason::ToolUse, true),
        ] {
            let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");
            let captured = capture.maybe_capture(&CaptureInput {
                stop_reason,
                has_tool_calls,
                assistant_response: if has_tool_calls {
                    "Let me check that."
                } else {
                    "A truncated or filtered response."
                },
                ..good_input()
            });
            assert!(
                !captured,
                "{label} must not be captured as training evidence"
            );
        }

        // The training directory should contain no rows for any failure mode.
        let training_dir = dir.path().join("training");
        for entry in std::fs::read_dir(&training_dir).expect("read dir") {
            let entry = entry.expect("entry");
            if entry.path().extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let content = std::fs::read_to_string(entry.path()).expect("read");
                assert!(
                    content.trim().is_empty(),
                    "no training rows should exist for failure modes: {content}"
                );
            }
        }
    }

    #[test]
    fn finalized_turn_records_finalization_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config_no_pii("training", 50 * 1024 * 1024);
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        assert!(capture.maybe_capture(&CaptureInput {
            turn_id: Some("turn-final-001"),
            turn_seq: 5,
            finalization_status: Some("finalized"),
            ..good_input()
        }));

        let content = std::fs::read_to_string(capture.file_path()).expect("read");
        let value: Value =
            serde_json::from_str(content.lines().next().expect("line")).expect("parse");
        assert_eq!(value["finalization_status"], "finalized");
        assert_eq!(value["turn_id"], "turn-final-001");
        assert_eq!(value["turn_seq"], 5);
    }

    #[test]
    fn unfinalized_turn_is_not_captured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config_no_pii("training", 50 * 1024 * 1024);
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(&CaptureInput {
            finalization_status: Some("pending"),
            ..good_input()
        });
        assert!(
            !captured,
            "unfinalized turn must not enter the training corpus"
        );
    }

    #[test]
    fn duplicate_turn_id_is_not_captured_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config_no_pii("training", 50 * 1024 * 1024);
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        assert!(capture.maybe_capture(&CaptureInput {
            turn_id: Some("turn-dup-001"),
            ..good_input()
        }));
        assert!(
            !capture.maybe_capture(&CaptureInput {
                turn_id: Some("turn-dup-001"),
                ..good_input()
            }),
            "duplicate turn id must not append a second training row"
        );
    }
}

// -- Decontamination policy (#5382) ------------------------------------------

mod decontamination_policy {
    use super::*;

    /// Longer than the classifier's internal `MAX_TEXT_LENGTH`, so
    /// `classify` returns `TextTooLong`. This is the reachable
    /// classifier-failure path a live corpus hits on an oversized paste.
    fn unclassifiable_message() -> String {
        "x".repeat(200_000)
    }

    fn config_with(policy: DecontaminationPolicy) -> TrainingConfig {
        TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
            max_shard_bytes: 50 * 1024 * 1024,
            pii_filter_enabled: false,
            decontamination_policy: policy,
            author_classifier_threshold: 0.85,
        }
    }

    /// Rows in the active corpus shard. A shard file that was never written
    /// counts as zero rather than failing the test.
    fn corpus_rows(capture: &TrainingCapture) -> usize {
        std::fs::read_to_string(capture.file_path())
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
    }

    fn quarantine_rows(dir: &std::path::Path) -> usize {
        match std::fs::read_to_string(dir.join("training").join("quarantine.jsonl")) {
            Ok(s) => s.lines().filter(|l| !l.trim().is_empty()).count(),
            Err(_) => 0,
        }
    }

    // WHY: this is the regression test for #5382. On the pre-fix code the
    // classifier error arm returned "allow", so this turn entered the corpus.
    #[test]
    fn fail_closed_keeps_unclassifiable_turns_out_of_the_corpus() {
        let dir = tempfile::tempdir().expect("tempdir");
        let long = unclassifiable_message();
        let mut capture =
            TrainingCapture::new(dir.path(), &config_with(DecontaminationPolicy::FailClosed))
                .expect("new");

        let captured = capture.maybe_capture(&CaptureInput {
            user_message: long.as_str(),
            turn_id: Some("turn-fc-001"),
            ..good_input()
        });

        assert!(!captured, "fail_closed must not admit an unclassified turn");
        assert_eq!(corpus_rows(&capture), 0, "corpus must stay empty");
        assert_eq!(
            quarantine_rows(dir.path()),
            0,
            "fail_closed drops rather than quarantines"
        );
    }

    #[test]
    fn quarantine_diverts_unclassifiable_turns_out_of_the_corpus() {
        let dir = tempfile::tempdir().expect("tempdir");
        let long = unclassifiable_message();
        let mut capture =
            TrainingCapture::new(dir.path(), &config_with(DecontaminationPolicy::Quarantine))
                .expect("new");

        let captured = capture.maybe_capture(&CaptureInput {
            user_message: long.as_str(),
            turn_id: Some("turn-q-001"),
            ..good_input()
        });

        assert!(!captured, "a quarantined turn is not a corpus row");
        assert_eq!(corpus_rows(&capture), 0, "corpus must stay empty");
        assert_eq!(
            quarantine_rows(dir.path()),
            1,
            "the turn must be inspectable in quarantine"
        );
    }

    #[test]
    fn warn_admits_unclassifiable_turns_but_records_the_verdict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let long = unclassifiable_message();
        let mut capture =
            TrainingCapture::new(dir.path(), &config_with(DecontaminationPolicy::Warn))
                .expect("new");

        let captured = capture.maybe_capture(&CaptureInput {
            user_message: long.as_str(),
            turn_id: Some("turn-w-001"),
            ..good_input()
        });

        assert!(captured, "warn admits the turn");
        let raw = std::fs::read_to_string(capture.file_path()).expect("shard");
        let value: serde_json::Value =
            serde_json::from_str(raw.lines().next().expect("one row")).expect("json");
        assert_eq!(value["decontamination_policy"], "warn");
        assert_eq!(value["decontamination_verdict"], "classifier_error");
        assert_eq!(value["schema_version"], TRAINING_RECORD_SCHEMA_VERSION);
    }

    #[test]
    fn disabled_policy_records_that_the_row_was_not_screened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut capture =
            TrainingCapture::new(dir.path(), &config_with(DecontaminationPolicy::Disabled))
                .expect("new");

        assert!(capture.maybe_capture(&CaptureInput {
            turn_id: Some("turn-d-001"),
            ..good_input()
        }));
        let raw = std::fs::read_to_string(capture.file_path()).expect("shard");
        let value: serde_json::Value =
            serde_json::from_str(raw.lines().next().expect("one row")).expect("json");
        assert_eq!(value["decontamination_policy"], "disabled");
        assert_eq!(value["decontamination_verdict"], "not_screened");
        assert!(
            value.get("classifier_version").is_none(),
            "an unscreened row must not claim a classifier version"
        );
    }
}
