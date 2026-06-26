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
        tokens: 150,
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
        author_classifier_enabled: false,
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
        tokens: 150,
        timestamp: Timestamp::UNIX_EPOCH,
        turn_type: Some("discussion".to_owned()),
        is_correction: Some(false),
        fact_types: None,
        quality_score: Some(0.9),
        tool_outcomes: None,
        recall_signals: None,
        tool_surface_hashes: Vec::new(),
        pii_redacted: false,
        pii_filter_applied: false,
        pii_redaction_count: 0,
        pii_policy_ref: None,
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
            tokens: 100,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            tool_outcomes: None,
            recall_signals: None,
            tool_surface_hashes: Vec::new(),
            pii_redacted: false,
            pii_filter_applied: false,
            pii_redaction_count: 0,
            pii_policy_ref: None,
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
            tokens: 100,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            tool_outcomes: None,
            recall_signals: None,
            tool_surface_hashes: Vec::new(),
            pii_redacted: false,
            pii_filter_applied: false,
            pii_redaction_count: 0,
            pii_policy_ref: None,
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

    capture.maybe_capture(good_input());

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

    let captured = capture.maybe_capture(CaptureInput {
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
        let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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

// -- Quality gate: happy path -------------------------------------------------

#[test]
fn quality_gate_accepts_good_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(good_input());
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    assert_eq!(content.lines().count(), 1);
}

#[test]
fn quality_gate_accepts_stop_sequence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_no_pii("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
        turn_type: Some("correction".to_owned()),
        is_correction: Some(true),
        fact_types: Some(vec!["preference".to_owned(), "identity".to_owned()]),
        ..good_input()
    });
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let parsed: TrainingRecord =
        serde_json::from_str(content.lines().next().expect("line")).expect("parse");
    assert_eq!(parsed.turn_type, Some("correction".to_owned()));
    assert_eq!(parsed.is_correction, Some(true));
    assert_eq!(
        parsed.fact_types,
        Some(vec!["preference".to_owned(), "identity".to_owned()])
    );
    // WHY: a correction turn supplies an is_correction signal, so a
    // quality score must be present.
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

// -- PII redaction --------------------------------------------------------

#[test]
fn pii_filter_redacts_user_message_when_enabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: true,
        author_classifier_enabled: false,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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
        author_classifier_enabled: false,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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
        author_classifier_enabled: false,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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
        author_classifier_enabled: false,
        author_classifier_threshold: 0.85,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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
        tokens: 200,
        timestamp: Timestamp::UNIX_EPOCH,
        turn_type: Some("planning".to_owned()),
        is_correction: None,
        fact_types: Some(vec!["skill".to_owned()]),
        quality_score: None,
        tool_outcomes: None,
        recall_signals: None,
        tool_surface_hashes: Vec::new(),
        pii_redacted: false,
        pii_filter_applied: false,
        pii_redaction_count: 0,
        pii_policy_ref: None,
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
        author_classifier_enabled: true,
        author_classifier_threshold: 0.85,
    }
}

#[test]
fn authorship_gate_rejects_agent_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = test_config_with_classifier("training", 50 * 1024 * 1024);
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
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

    let captured = capture.maybe_capture(CaptureInput {
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
    let captured = capture.maybe_capture(CaptureInput {
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
            let captured = capture.maybe_capture(CaptureInput {
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

        assert!(capture.maybe_capture(CaptureInput {
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

        let captured = capture.maybe_capture(CaptureInput {
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

        assert!(capture.maybe_capture(CaptureInput {
            turn_id: Some("turn-dup-001"),
            ..good_input()
        }));
        assert!(
            !capture.maybe_capture(CaptureInput {
                turn_id: Some("turn-dup-001"),
                ..good_input()
            }),
            "duplicate turn id must not append a second training row"
        );
    }
}
