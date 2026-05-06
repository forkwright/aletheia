//! (See parent module for full rationale.)

use super::super::*;
use super::test_timestamp;
use crate::id::{EmbeddingId, FactId};

// Split: Entity / Fact / CompactFact / related tests.

#[test]
fn forget_reason_serde_roundtrip() {
    for reason in [
        ForgetReason::UserRequested,
        ForgetReason::Outdated,
        ForgetReason::Incorrect,
        ForgetReason::Privacy,
        ForgetReason::Stale,
        ForgetReason::Superseded,
    ] {
        let json =
            serde_json::to_string(&reason).expect("ForgetReason serialization is infallible");
        let back: ForgetReason =
            serde_json::from_str(&json).expect("ForgetReason should deserialize from its own JSON");
        assert_eq!(reason, back, "ForgetReason should survive serde roundtrip");
    }
}

#[test]
fn forget_reason_as_str_matches_serde() {
    for reason in [
        ForgetReason::UserRequested,
        ForgetReason::Outdated,
        ForgetReason::Incorrect,
        ForgetReason::Privacy,
        ForgetReason::Stale,
        ForgetReason::Superseded,
    ] {
        let json =
            serde_json::to_string(&reason).expect("ForgetReason serialization is infallible");
        let expected = format!("\"{}\"", reason.as_str());
        assert_eq!(
            json, expected,
            "ForgetReason json should match as_str representation"
        );
    }
}

#[test]
fn forget_reason_from_str_roundtrip() {
    for reason in [
        ForgetReason::UserRequested,
        ForgetReason::Outdated,
        ForgetReason::Incorrect,
        ForgetReason::Privacy,
        ForgetReason::Stale,
        ForgetReason::Superseded,
    ] {
        let parsed: ForgetReason = reason
            .as_str()
            .parse()
            .expect("ForgetReason as_str() should round-trip through FromStr");
        assert_eq!(
            reason, parsed,
            "ForgetReason should survive as_str/parse roundtrip"
        );
    }
}

#[test]
fn forget_reason_from_str_unknown() {
    assert!(
        "bogus".parse::<ForgetReason>().is_err(),
        "unrecognized string should fail to parse as ForgetReason"
    );
}

#[test]
fn forget_reason_display() {
    assert_eq!(
        ForgetReason::UserRequested.to_string(),
        "user_requested",
        "UserRequested should display as 'user_requested'"
    );
    assert_eq!(
        ForgetReason::Outdated.to_string(),
        "outdated",
        "Outdated should display as 'outdated'"
    );
    assert_eq!(
        ForgetReason::Incorrect.to_string(),
        "incorrect",
        "Incorrect should display as 'incorrect'"
    );
    assert_eq!(
        ForgetReason::Privacy.to_string(),
        "privacy",
        "Privacy should display as 'privacy'"
    );
}

#[test]
fn epistemic_tier_display_roundtrip() {
    for tier in [
        EpistemicTier::Verified,
        EpistemicTier::Inferred,
        EpistemicTier::Assumed,
    ] {
        let s = tier.as_str();
        let json_str = format!("\"{s}\"");
        let parsed: EpistemicTier = serde_json::from_str(&json_str)
            .expect("EpistemicTier should deserialize from its as_str() representation");
        assert_eq!(tier, parsed, "roundtrip failed for {s}");
    }
}

#[test]
fn fact_default_stability_hours_known_types() {
    assert!(
        (default_stability_hours("identity") - 17520.0).abs() < f64::EPSILON,
        "identity stability should be 17520 hours"
    );
    assert!(
        (default_stability_hours("preference") - 8760.0).abs() < f64::EPSILON,
        "preference stability should be 8760 hours"
    );
    assert!(
        (default_stability_hours("skill") - 4380.0).abs() < f64::EPSILON,
        "skill stability should be 4380 hours"
    );
    assert!(
        (default_stability_hours("relationship") - 2190.0).abs() < f64::EPSILON,
        "relationship stability should be 2190 hours"
    );
    assert!(
        (default_stability_hours("task") - 168.0).abs() < f64::EPSILON,
        "task stability should be 168 hours"
    );
    assert!(
        (default_stability_hours("completely_unknown_type") - 72.0).abs() < f64::EPSILON,
        "fallback for unknown fact types should be 72 hours (Observation)"
    );
}

#[test]
fn forget_reason_all_variants_as_str() {
    let all = [
        ForgetReason::UserRequested,
        ForgetReason::Outdated,
        ForgetReason::Incorrect,
        ForgetReason::Privacy,
    ];
    for reason in all {
        let s = reason.as_str();
        assert!(!s.is_empty(), "as_str() must be non-empty for {reason:?}");
    }
}

#[test]
fn fact_diff_empty() {
    let diff = FactDiff {
        added: vec![],
        modified: vec![],
        removed: vec![],
    };
    assert!(diff.added.is_empty(), "added list should be empty");
    assert!(diff.modified.is_empty(), "modified list should be empty");
    assert!(diff.removed.is_empty(), "removed list should be empty");
    let json = serde_json::to_string(&diff).expect("FactDiff serialization is infallible");
    let back: FactDiff =
        serde_json::from_str(&json).expect("FactDiff should deserialize from its own JSON");
    assert!(
        back.added.is_empty(),
        "added list should be empty after roundtrip"
    );
    assert!(
        back.modified.is_empty(),
        "modified list should be empty after roundtrip"
    );
    assert!(
        back.removed.is_empty(),
        "removed list should be empty after roundtrip"
    );
}

#[test]
fn embedded_chunk_fields() {
    let chunk = EmbeddedChunk {
        id: EmbeddingId::new("emb-42").expect("valid test id"),
        content: "test content".to_owned(),
        source_type: "note".to_owned(),
        source_id: "note-7".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: vec![1.0, 2.0, 3.0, 4.0],
        created_at: test_timestamp("2026-03-01T00:00:00Z"),
    };
    assert_eq!(chunk.id.as_str(), "emb-42", "chunk id should be set");
    assert_eq!(chunk.content, "test content", "chunk content should be set");
    assert_eq!(chunk.source_type, "note", "chunk source_type should be set");
    assert_eq!(chunk.source_id, "note-7", "chunk source_id should be set");
    assert_eq!(chunk.nous_id, "syn", "chunk nous_id should be set");
    assert_eq!(
        chunk.embedding.len(),
        4,
        "chunk embedding should have four dimensions"
    );
}

#[test]
fn epistemic_tier_ordering() {
    let verified_score = match EpistemicTier::Verified {
        EpistemicTier::Verified | EpistemicTier::Reflected => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
        EpistemicTier::Training => 4,
    };
    let inferred_score = match EpistemicTier::Inferred {
        EpistemicTier::Verified | EpistemicTier::Reflected => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
        EpistemicTier::Training => 4,
    };
    let assumed_score = match EpistemicTier::Assumed {
        EpistemicTier::Verified | EpistemicTier::Reflected => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
        EpistemicTier::Training => 4,
    };
    assert!(
        verified_score > inferred_score,
        "Verified must rank higher than Inferred"
    );
    assert!(
        inferred_score > assumed_score,
        "Inferred must rank higher than Assumed"
    );
}

#[test]
fn fact_with_supersession() {
    let fact = Fact {
        id: FactId::new("f-old").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "outdated claim".to_owned(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-01-01"),
            valid_to: test_timestamp("2026-02-01"),
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.7,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: Some(FactId::new("f-new").expect("valid test id")),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: FactSensitivity::Public,
    };
    assert_eq!(
        fact.lifecycle.superseded_by.as_ref().map(FactId::as_str),
        Some("f-new"),
        "superseded_by should reference the new fact id"
    );
    let json = serde_json::to_string(&fact)
        .expect("Fact with superseded_by field serializes successfully");
    let back: Fact = serde_json::from_str(&json)
        .expect("Fact with superseded_by should deserialize from its own JSON");
    assert_eq!(
        back.lifecycle.superseded_by.as_ref().map(FactId::as_str),
        Some("f-new"),
        "superseded_by should survive serde roundtrip"
    );
}

#[test]
fn fact_with_session_source() {
    let fact = Fact {
        id: FactId::new("f-src").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "extracted from conversation".to_owned(),
        fact_type: "relationship".to_owned(),
        scope: Some(MemoryScope::Project),
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-03-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.85,
            tier: EpistemicTier::Verified,
            source_session_id: Some("ses-abc-123".to_owned()),
            stability_hours: 4380.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 3,
            last_accessed_at: Some(test_timestamp("2026-03-05T12:00:00Z")),
        },
        sensitivity: FactSensitivity::Public,
    };
    assert_eq!(
        fact.provenance.source_session_id.as_deref(),
        Some("ses-abc-123"),
        "source_session_id should be set"
    );
    let json =
        serde_json::to_string(&fact).expect("Fact with source_session_id serializes successfully");
    let back: Fact = serde_json::from_str(&json)
        .expect("Fact with source_session_id should deserialize from its own JSON");
    assert_eq!(
        back.provenance.source_session_id.as_deref(),
        Some("ses-abc-123"),
        "source_session_id should survive serde roundtrip"
    );
}

#[test]
fn parse_timestamp_full() {
    let ts = parse_timestamp("2026-03-01T12:30:00Z");
    assert!(
        ts.is_some(),
        "full ISO 8601 timestamp should parse successfully"
    );
}

#[test]
fn parse_timestamp_date_only() {
    let ts = parse_timestamp("2026-03-01");
    assert!(
        ts.is_some(),
        "date-only timestamp should parse successfully"
    );
}

#[test]
fn parse_timestamp_empty() {
    assert!(
        parse_timestamp("").is_none(),
        "empty string should not parse as timestamp"
    );
}

#[test]
fn parse_timestamp_invalid() {
    assert!(
        parse_timestamp("not-a-date").is_none(),
        "non-date string should not parse as timestamp"
    );
}

#[test]
fn format_timestamp_roundtrip() {
    let ts =
        parse_timestamp("2026-03-01T12:30:00Z").expect("valid ISO 8601 timestamp should parse");
    let s = format_timestamp(&ts);
    assert_eq!(
        s, "2026-03-01T12:30:00Z",
        "timestamp should format to expected string"
    );
    let back = parse_timestamp(&s).expect("formatted timestamp should parse back");
    assert_eq!(ts, back, "timestamp should survive format/parse roundtrip");
}

#[test]
fn far_future_is_year_9999() {
    let ts = far_future();
    let s = format_timestamp(&ts);
    assert!(
        s.starts_with("9999-01-01"),
        "far future should be year 9999"
    );
    assert!(
        is_far_future(&ts),
        "far_future() should be recognized by is_far_future"
    );
}
