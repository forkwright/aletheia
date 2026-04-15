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
        quality_score: None,
    }
}

#[test]
fn training_config_defaults() {
    let config = TrainingConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.path, "data/training");
    assert_eq!(config.max_shard_bytes, 50 * 1024 * 1024);
}

#[test]
fn training_capture_writes_jsonl() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
        };
        capture.write_record(&record).expect("write");
    }

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);

    // Manifest should reflect 3 records
    assert_eq!(capture.manifest().total_records, 3);
    assert_eq!(capture.manifest().shards.len(), 1);
    assert_eq!(capture.manifest().shards[0].record_count, 3);
}

// -- Shard rotation -------------------------------------------------------

#[test]
fn shard_rotation_on_size_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        // Tiny limit to force rotation after ~1 record
        max_shard_bytes: 100,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // Write enough records to trigger rotation
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
        };
        capture.write_record(&record).expect("write");
    }

    // Should have created multiple shards
    assert!(
        capture.manifest().shards.len() > 1,
        "expected multiple shards, got {}",
        capture.manifest().shards.len()
    );
    assert_eq!(capture.manifest().total_records, 5);

    // All shard files should exist
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

    // Write a legacy file with 2 records
    let legacy_path = training_dir.join("conversations.jsonl");
    let record_json = r#"{"session_id":"old-1","nous_id":"syn","user_message":"hi","assistant_response":"hello","model":"test","tokens":10,"timestamp":"1970-01-01T00:00:00Z"}"#;
    fs::write(&legacy_path, format!("{record_json}\n{record_json}\n")).expect("write legacy");

    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // Manifest should include the legacy file
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    capture.maybe_capture(good_input());

    // Manifest file should exist
    let manifest_path = dir.path().join("training").join("training-manifest.json");
    assert!(manifest_path.exists(), "manifest file should exist");

    // Should be valid JSON
    let content = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: TrainingManifest = serde_json::from_str(&content).expect("parse manifest");
    assert_eq!(manifest.total_records, 1);
}

// -- Quality gate: empty / whitespace -----------------------------------------

#[test]
fn quality_gate_rejects_empty_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
        stop_reason: CaptureStopReason::Unknown,
        ..good_input()
    });
    assert!(!captured, "unknown stop reason should be rejected");
}

// -- Quality gate: tool-use-only ----------------------------------------------

#[test]
fn quality_gate_rejects_tool_use_only_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    // Turn that used tools but ended with text (end_turn)
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(good_input());
    assert!(captured);

    let content = std::fs::read_to_string(capture.file_path()).expect("read");
    assert_eq!(content.lines().count(), 1);
}

#[test]
fn quality_gate_accepts_stop_sequence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
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
    let config = TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
    };
    let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

    let captured = capture.maybe_capture(CaptureInput {
        turn_type: Some("correction".to_owned()),
        is_correction: Some(true),
        fact_types: Some(vec!["preference".to_owned(), "identity".to_owned()]),
        quality_score: Some(0.95),
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
    assert_eq!(parsed.quality_score, Some(0.95));
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
    };

    let json = serde_json::to_string(&record).expect("serialize");
    let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
    assert_eq!(back.session_id, record.session_id);
    assert_eq!(back.tokens, record.tokens);
    assert_eq!(back.turn_type, Some("planning".to_owned()));
    assert!(back.is_correction.is_none());
}
