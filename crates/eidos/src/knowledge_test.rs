#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
use std::path::{Path, PathBuf};

use super::*;
use crate::id::FactId;

fn test_timestamp(s: &str) -> jiff::Timestamp {
    parse_timestamp(s).expect("valid test timestamp")
}

#[test]
fn entity_id_from_str() {
    let id = EntityId::new("alice").expect("valid test id");
    assert_eq!(id.as_str(), "alice", "as_str should return inner value");
    assert_eq!(
        id.to_string(),
        "alice",
        "to_string should return inner value"
    );
}

#[test]
fn entity_id_from_string() {
    let id = EntityId::new("bob".to_owned()).expect("valid test id");
    assert_eq!(
        id.as_str(),
        "bob",
        "EntityId from owned String should store inner value"
    );
}

#[test]
fn entity_id_serde_transparent() {
    let id = EntityId::new("e-123").expect("valid test id");
    let json = serde_json::to_string(&id).expect("EntityId serialization is infallible");
    assert_eq!(
        json, r#""e-123""#,
        "EntityId must serialize as plain string"
    );
    let back: EntityId =
        serde_json::from_str(&json).expect("EntityId should deserialize from its own JSON");
    assert_eq!(id, back, "EntityId should survive serde roundtrip");
}

#[test]
fn entity_id_prevents_mixing_with_plain_string() {
    let eid = EntityId::new("nous-1").expect("valid test id");
    let plain: String = "nous-1".to_owned();
    assert_eq!(
        eid.as_str(),
        plain.as_str(),
        "EntityId and plain string with same value should compare equal"
    );
}

#[test]
fn entity_id_display_matches_inner_string() {
    let id = EntityId::new("project-aletheia").expect("valid test id");
    assert_eq!(
        format!("{id}"),
        "project-aletheia",
        "Display should render inner string value"
    );
}

#[test]
fn entity_id_clone_equality() {
    let a = EntityId::new("e-42").expect("valid test id");
    let b = a.clone();
    assert_eq!(a, b, "cloned EntityId must equal original");
    assert_eq!(
        a.as_str(),
        b.as_str(),
        "cloned EntityId as_str should equal original"
    );
}

#[test]
fn epistemic_tier_serde_roundtrip() {
    for tier in [
        EpistemicTier::Verified,
        EpistemicTier::Inferred,
        EpistemicTier::Assumed,
    ] {
        let json = serde_json::to_string(&tier).expect("EpistemicTier serialization is infallible");
        let back: EpistemicTier = serde_json::from_str(&json)
            .expect("EpistemicTier should deserialize from its own JSON");
        assert_eq!(tier, back, "EpistemicTier should survive serde roundtrip");
    }
}

#[test]
fn fact_serde_roundtrip() {
    let fact = Fact {
        id: FactId::new("fact-1").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "The researcher published findings on memory consolidation".to_owned(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-02-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-02-28T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            source_session_id: Some("ses-123".to_owned()),
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
    let back: Fact =
        serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
    assert_eq!(
        fact.content, back.content,
        "fact content should survive serde roundtrip"
    );
    assert_eq!(
        fact.provenance.tier, back.provenance.tier,
        "fact tier should survive serde roundtrip"
    );
}

#[test]
fn entity_serde_roundtrip() {
    let entity = Entity {
        id: EntityId::new("e-1").expect("valid test id"),
        name: "Dr. Chen".to_owned(),
        entity_type: "person".to_owned(),
        aliases: vec!["acme_user".to_owned(), "test-user-01".to_owned()],
        created_at: test_timestamp("2026-01-28T00:00:00Z"),
        updated_at: test_timestamp("2026-02-28T00:00:00Z"),
    };
    let json = serde_json::to_string(&entity).expect("Entity serialization is infallible");
    let back: Entity =
        serde_json::from_str(&json).expect("Entity should deserialize from its own JSON");
    assert_eq!(
        entity.name, back.name,
        "entity name should survive serde roundtrip"
    );
    assert_eq!(
        entity.aliases, back.aliases,
        "entity aliases should survive serde roundtrip"
    );
}

#[test]
fn relationship_serde_roundtrip() {
    let rel = Relationship {
        src: EntityId::new("e-1").expect("valid test id"),
        dst: EntityId::new("e-2").expect("valid test id"),
        relation: "works_on".to_owned(),
        weight: 0.85,
        created_at: test_timestamp("2026-02-28T00:00:00Z"),
    };
    let json = serde_json::to_string(&rel).expect("Relationship serialization is infallible");
    let back: Relationship =
        serde_json::from_str(&json).expect("Relationship should deserialize from its own JSON");
    assert_eq!(
        rel.src, back.src,
        "relationship src should survive serde roundtrip"
    );
    assert_eq!(
        rel.dst, back.dst,
        "relationship dst should survive serde roundtrip"
    );
    assert_eq!(
        rel.relation, back.relation,
        "relationship relation should survive serde roundtrip"
    );
}

#[test]
fn embedded_chunk_serde_roundtrip() {
    let chunk = EmbeddedChunk {
        id: EmbeddingId::new("emb-1").expect("valid test id"),
        content: "some text".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "fact-1".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: vec![0.1, 0.2, 0.3],
        created_at: test_timestamp("2026-02-28T00:00:00Z"),
    };
    let json = serde_json::to_string(&chunk).expect("EmbeddedChunk serialization is infallible");
    let back: EmbeddedChunk =
        serde_json::from_str(&json).expect("EmbeddedChunk should deserialize from its own JSON");
    assert_eq!(
        chunk.content, back.content,
        "chunk content should survive serde roundtrip"
    );
    assert_eq!(
        chunk.embedding.len(),
        back.embedding.len(),
        "embedding length should survive serde roundtrip"
    );
}

#[test]
fn recall_result_serde_roundtrip() {
    let result = RecallResult {
        content: "The researcher published findings on memory consolidation".to_owned(),
        distance: 0.12,
        source_type: "fact".to_owned(),
        source_id: "fact-1".to_owned(),
    };
    let json = serde_json::to_string(&result).expect("RecallResult serialization is infallible");
    let back: RecallResult =
        serde_json::from_str(&json).expect("RecallResult should deserialize from its own JSON");
    assert_eq!(
        result.content, back.content,
        "recall result content should survive serde roundtrip"
    );
    assert!(
        (result.distance - back.distance).abs() < f64::EPSILON,
        "recall result distance should survive serde roundtrip"
    );
}

#[test]
fn fact_with_empty_content() {
    let fact = Fact {
        id: FactId::new("f-empty").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: String::new(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    let json =
        serde_json::to_string(&fact).expect("Fact with empty content serializes successfully");
    let back: Fact = serde_json::from_str(&json)
        .expect("Fact with empty content should deserialize from its own JSON");
    assert!(
        back.content.is_empty(),
        "empty fact content should survive serde roundtrip"
    );
}

#[test]
fn fact_with_unicode_content() {
    let fact = Fact {
        id: FactId::new("f-uni").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "The user writes \u{65E5}\u{672C}\u{8A9E} and emoji \u{1F980}".to_owned(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    let json =
        serde_json::to_string(&fact).expect("Fact with unicode content serializes successfully");
    let back: Fact = serde_json::from_str(&json)
        .expect("Fact with unicode content should deserialize from its own JSON");
    assert_eq!(
        fact.content, back.content,
        "unicode fact content should survive serde roundtrip"
    );
}

#[test]
fn entity_empty_aliases() {
    let entity = Entity {
        id: EntityId::new("e-2").expect("valid test id"),
        name: "Aletheia".to_owned(),
        entity_type: "project".to_owned(),
        aliases: vec![],
        created_at: test_timestamp("2026-01-01T00:00:00Z"),
        updated_at: test_timestamp("2026-01-01T00:00:00Z"),
    };
    let json =
        serde_json::to_string(&entity).expect("Entity with empty aliases serializes successfully");
    let back: Entity = serde_json::from_str(&json)
        .expect("Entity with empty aliases should deserialize from its own JSON");
    assert!(
        back.aliases.is_empty(),
        "empty aliases should survive serde roundtrip"
    );
}

#[test]
fn epistemic_tier_display() {
    assert_eq!(
        EpistemicTier::Verified.to_string(),
        "verified",
        "Verified should display as 'verified'"
    );
    assert_eq!(
        EpistemicTier::Inferred.to_string(),
        "inferred",
        "Inferred should display as 'inferred'"
    );
    assert_eq!(
        EpistemicTier::Assumed.to_string(),
        "assumed",
        "Assumed should display as 'assumed'"
    );
}

#[test]
fn default_stability_by_fact_type() {
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
        (default_stability_hours("event") - 720.0).abs() < f64::EPSILON,
        "event stability should be 720 hours"
    );
    assert!(
        (default_stability_hours("task") - 168.0).abs() < f64::EPSILON,
        "task stability should be 168 hours"
    );
    assert!(
        (default_stability_hours("observation") - 72.0).abs() < f64::EPSILON,
        "observation stability should be 72 hours"
    );
    assert!(
        (default_stability_hours("verification") - 168.0).abs() < f64::EPSILON,
        "verification stability should be 168 hours"
    );
    assert!(
        (default_stability_hours("inference") - 72.0).abs() < f64::EPSILON,
        "inference should fall back to 72 hours"
    );
    assert!(
        (default_stability_hours("unknown") - 72.0).abs() < f64::EPSILON,
        "unknown type should fall back to 72 hours"
    );
    assert!(
        (default_stability_hours("") - 72.0).abs() < f64::EPSILON,
        "empty type should fall back to 72 hours"
    );
}

#[test]
fn epistemic_tier_as_str_matches_serde() {
    for tier in [
        EpistemicTier::Verified,
        EpistemicTier::Inferred,
        EpistemicTier::Assumed,
    ] {
        let json = serde_json::to_string(&tier).expect("EpistemicTier serialization is infallible");
        let expected = format!("\"{}\"", tier.as_str());
        assert_eq!(
            json, expected,
            "EpistemicTier json should match as_str representation"
        );
    }
}

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
        EpistemicTier::Verified => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
    };
    let inferred_score = match EpistemicTier::Inferred {
        EpistemicTier::Verified => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
    };
    let assumed_score = match EpistemicTier::Assumed {
        EpistemicTier::Verified => 3,
        EpistemicTier::Inferred => 2,
        EpistemicTier::Assumed => 1,
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

// ---------------------------------------------------------------------------
// MemoryScope tests
// ---------------------------------------------------------------------------

#[test]
fn memory_scope_serde_roundtrip() {
    for scope in MemoryScope::ALL {
        let json = serde_json::to_string(&scope).expect("MemoryScope serialization is infallible");
        let back: MemoryScope =
            serde_json::from_str(&json).expect("MemoryScope should deserialize from its own JSON");
        assert_eq!(scope, back, "MemoryScope should survive serde roundtrip");
    }
}

#[test]
fn memory_scope_as_str_matches_serde() {
    for scope in MemoryScope::ALL {
        let json = serde_json::to_string(&scope).expect("MemoryScope serialization is infallible");
        let expected = format!("\"{}\"", scope.as_str());
        assert_eq!(
            json, expected,
            "MemoryScope json should match as_str representation"
        );
    }
}

#[test]
fn memory_scope_from_str_roundtrip() {
    for scope in MemoryScope::ALL {
        let parsed: MemoryScope = scope
            .as_str()
            .parse()
            .expect("MemoryScope as_str() should round-trip through FromStr");
        assert_eq!(
            scope, parsed,
            "MemoryScope should survive as_str/parse roundtrip"
        );
    }
}

#[test]
fn memory_scope_from_str_unknown() {
    assert!(
        "bogus".parse::<MemoryScope>().is_err(),
        "unrecognized string should fail to parse as MemoryScope"
    );
}

#[test]
fn memory_scope_display() {
    assert_eq!(
        MemoryScope::User.to_string(),
        "user",
        "User should display as 'user'"
    );
    assert_eq!(
        MemoryScope::Feedback.to_string(),
        "feedback",
        "Feedback should display as 'feedback'"
    );
    assert_eq!(
        MemoryScope::Project.to_string(),
        "project",
        "Project should display as 'project'"
    );
    assert_eq!(
        MemoryScope::Reference.to_string(),
        "reference",
        "Reference should display as 'reference'"
    );
}

#[test]
fn memory_scope_dir_names_match_as_str() {
    for scope in MemoryScope::ALL {
        assert_eq!(
            scope.as_dir_name(),
            scope.as_str(),
            "dir name should match as_str for {scope:?}"
        );
    }
}

#[test]
fn memory_scope_all_has_four_variants() {
    assert_eq!(
        MemoryScope::ALL.len(),
        4,
        "ALL should contain exactly 4 scope variants"
    );
}

#[test]
fn memory_scope_from_str_opt_returns_some_for_valid() {
    assert_eq!(
        MemoryScope::from_str_opt("user"),
        Some(MemoryScope::User),
        "from_str_opt should return Some(User) for 'user'"
    );
    assert_eq!(
        MemoryScope::from_str_opt("feedback"),
        Some(MemoryScope::Feedback),
        "from_str_opt should return Some(Feedback) for 'feedback'"
    );
}

#[test]
fn memory_scope_from_str_opt_returns_none_for_invalid() {
    assert_eq!(
        MemoryScope::from_str_opt("invalid"),
        None,
        "from_str_opt should return None for unrecognized scope"
    );
    assert_eq!(
        MemoryScope::from_str_opt(""),
        None,
        "from_str_opt should return None for empty string"
    );
}

// ---------------------------------------------------------------------------
// ScopeAccessPolicy tests
// ---------------------------------------------------------------------------

#[test]
fn user_scope_is_private() {
    let policy = MemoryScope::User.access_policy();
    assert!(
        !policy.permits_agent_read(),
        "agents must not read user scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write user scope"
    );
    assert!(
        policy.user_write_only,
        "user scope should be user-write-only"
    );
}

#[test]
fn feedback_scope_is_read_only_for_agents() {
    let policy = MemoryScope::Feedback.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read feedback scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write feedback scope"
    );
    assert!(
        policy.user_write_only,
        "feedback scope should be user-write-only"
    );
}

#[test]
fn project_scope_is_shared_read_write() {
    let policy = MemoryScope::Project.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read project scope"
    );
    assert!(
        policy.permits_agent_write(),
        "agents must write project scope"
    );
    assert!(
        !policy.user_write_only,
        "project scope should not be user-write-only"
    );
}

#[test]
fn reference_scope_is_hybrid() {
    let policy = MemoryScope::Reference.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read reference scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write reference scope"
    );
    assert!(
        policy.user_write_only,
        "reference scope should be user-write-only"
    );
}

// ---------------------------------------------------------------------------
// Fact with scope tests
// ---------------------------------------------------------------------------

#[test]
fn fact_scope_none_omitted_in_json() {
    let fact = Fact {
        id: FactId::new("f-no-scope").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "legacy fact without scope".to_owned(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            source_session_id: None,
            stability_hours: 72.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
    assert!(
        !json.contains("\"scope\""),
        "scope: None should be omitted from serialized JSON"
    );
    let back: Fact =
        serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
    assert_eq!(
        back.scope, None,
        "deserialized scope should be None when omitted"
    );
}

#[test]
fn fact_scope_some_included_in_json() {
    let fact = Fact {
        id: FactId::new("f-scoped").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "team project fact".to_owned(),
        fact_type: "project".to_owned(),
        scope: Some(MemoryScope::Project),
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-03-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
    assert!(
        json.contains("\"scope\":\"project\""),
        "scope: Some(Project) should appear in serialized JSON"
    );
    let back: Fact =
        serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
    assert_eq!(
        back.scope,
        Some(MemoryScope::Project),
        "deserialized scope should be Some(Project)"
    );
}

#[test]
fn fact_backward_compat_no_scope_field() {
    // WHY: Existing JSON without a `scope` field must deserialize to `scope: None`.
    let json = r#"{
        "id": "f-legacy",
        "nous_id": "syn",
        "fact_type": "observation",
        "content": "old fact from before team memory",
        "valid_from": "2026-01-01T00:00:00Z",
        "valid_to": "9999-01-01T00:00:00Z",
        "recorded_at": "2026-01-01T00:00:00Z",
        "confidence": 0.5,
        "tier": "assumed",
        "source_session_id": null,
        "stability_hours": 72.0,
        "superseded_by": null,
        "is_forgotten": false,
        "forgotten_at": null,
        "forget_reason": null,
        "access_count": 0,
        "last_accessed_at": null
    }"#;
    let fact: Fact =
        serde_json::from_str(json).expect("legacy JSON without scope field should deserialize");
    assert_eq!(
        fact.scope, None,
        "legacy fact without scope field should deserialize to scope: None"
    );
}

#[test]
fn fact_scope_all_variants_roundtrip() {
    for scope in MemoryScope::ALL {
        let fact = Fact {
            id: FactId::new("f-scope-test").expect("valid test id"),
            nous_id: "syn".to_owned(),
            content: format!("fact in {scope} scope"),
            fact_type: String::new(),
            scope: Some(scope),
            temporal: FactTemporal {
                valid_from: test_timestamp("2026-01-01"),
                valid_to: far_future(),
                recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
            },
            provenance: FactProvenance {
                confidence: 0.5,
                tier: EpistemicTier::Assumed,
                source_session_id: None,
                stability_hours: 72.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        };
        let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
        let back: Fact =
            serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
        assert_eq!(
            back.scope,
            Some(scope),
            "scope {scope:?} should survive serde roundtrip"
        );
    }
}

// ---------------------------------------------------------------------------
// PathValidationLayer tests
// ---------------------------------------------------------------------------

#[test]
fn path_validation_layer_serde_roundtrip() {
    for layer in PathValidationLayer::ALL {
        let json =
            serde_json::to_string(&layer).expect("PathValidationLayer serialization is infallible");
        let back: PathValidationLayer = serde_json::from_str(&json)
            .expect("PathValidationLayer should deserialize from its own JSON");
        assert_eq!(
            layer, back,
            "PathValidationLayer should survive serde roundtrip"
        );
    }
}

#[test]
fn path_validation_layer_as_str_matches_serde() {
    for layer in PathValidationLayer::ALL {
        let json =
            serde_json::to_string(&layer).expect("PathValidationLayer serialization is infallible");
        let expected = format!("\"{}\"", layer.as_str());
        assert_eq!(
            json, expected,
            "PathValidationLayer json should match as_str representation"
        );
    }
}

#[test]
fn path_validation_layer_from_str_roundtrip() {
    for layer in PathValidationLayer::ALL {
        let parsed: PathValidationLayer = layer
            .as_str()
            .parse()
            .expect("PathValidationLayer as_str() should round-trip through FromStr");
        assert_eq!(
            layer, parsed,
            "PathValidationLayer should survive as_str/parse roundtrip"
        );
    }
}

#[test]
fn path_validation_layer_from_str_unknown() {
    assert!(
        "bogus".parse::<PathValidationLayer>().is_err(),
        "unrecognized string should fail to parse as PathValidationLayer"
    );
}

#[test]
fn path_validation_layer_display() {
    assert_eq!(
        PathValidationLayer::NullByte.to_string(),
        "null_byte",
        "NullByte should display as 'null_byte'"
    );
    assert_eq!(
        PathValidationLayer::UnicodeNormalization.to_string(),
        "unicode_normalization",
        "UnicodeNormalization should display as 'unicode_normalization'"
    );
    assert_eq!(
        PathValidationLayer::ScopeContainment.to_string(),
        "scope_containment",
        "ScopeContainment should display as 'scope_containment'"
    );
}

#[test]
fn path_validation_layer_all_has_eight_entries() {
    assert_eq!(
        PathValidationLayer::ALL.len(),
        8,
        "ALL should contain exactly 8 validation layers"
    );
}

#[test]
fn path_validation_layer_io_classification() {
    // WHY: Only filesystem-interacting layers should require I/O.
    let io_layers = PathValidationLayer::ALL
        .iter()
        .filter(|l| l.requires_io())
        .count();
    assert_eq!(
        io_layers, 3,
        "exactly 3 layers (symlink resolution, dangling symlink, loop detection) require I/O"
    );
    assert!(
        PathValidationLayer::SymlinkResolution.requires_io(),
        "symlink resolution requires I/O"
    );
    assert!(
        PathValidationLayer::DanglingSymlink.requires_io(),
        "dangling symlink detection requires I/O"
    );
    assert!(
        PathValidationLayer::LoopDetection.requires_io(),
        "loop detection requires I/O"
    );
    assert!(
        !PathValidationLayer::NullByte.requires_io(),
        "null byte check does not require I/O"
    );
    assert!(
        !PathValidationLayer::Canonicalization.requires_io(),
        "canonicalization does not require I/O"
    );
    assert!(
        !PathValidationLayer::UrlEncodedTraversal.requires_io(),
        "URL-encoded traversal check does not require I/O"
    );
    assert!(
        !PathValidationLayer::UnicodeNormalization.requires_io(),
        "unicode normalization check does not require I/O"
    );
    assert!(
        !PathValidationLayer::ScopeContainment.requires_io(),
        "scope containment check does not require I/O"
    );
}

#[test]
fn path_validation_fs_layers_constant() {
    assert_eq!(
        PATH_VALIDATION_FS_LAYERS, 7,
        "PATH_VALIDATION_FS_LAYERS should be 7"
    );
}

#[test]
fn symlink_hop_limit_matches_linux_eloop() {
    assert_eq!(
        SYMLINK_HOP_LIMIT, 40,
        "SYMLINK_HOP_LIMIT should match Linux ELOOP limit of 40"
    );
}

// ---------------------------------------------------------------------------
// PathValidationError tests
// ---------------------------------------------------------------------------

#[test]
fn path_validation_error_layer_mapping() {
    // WHY: Every error variant must map back to the correct layer for logging.
    let cases: Vec<(PathValidationError, PathValidationLayer)> = vec![
        (
            PathValidationError::NullByte {
                path: String::new(),
            },
            PathValidationLayer::NullByte,
        ),
        (
            PathValidationError::Canonicalization {
                path: String::new(),
                component: String::new(),
            },
            PathValidationLayer::Canonicalization,
        ),
        (
            PathValidationError::SymlinkResolution {
                path: PathBuf::new(),
                root: PathBuf::new(),
            },
            PathValidationLayer::SymlinkResolution,
        ),
        (
            PathValidationError::DanglingSymlink {
                path: PathBuf::new(),
            },
            PathValidationLayer::DanglingSymlink,
        ),
        (
            PathValidationError::LoopDetection {
                path: PathBuf::new(),
                hops: 0,
            },
            PathValidationLayer::LoopDetection,
        ),
        (
            PathValidationError::UrlEncodedTraversal {
                path: String::new(),
                decoded_fragment: String::new(),
            },
            PathValidationLayer::UrlEncodedTraversal,
        ),
        (
            PathValidationError::UnicodeNormalization {
                path: String::new(),
                offending_char: '.',
            },
            PathValidationLayer::UnicodeNormalization,
        ),
        (
            PathValidationError::ScopeContainment {
                path: PathBuf::new(),
                scope: MemoryScope::User,
                expected_dir: PathBuf::new(),
            },
            PathValidationLayer::ScopeContainment,
        ),
    ];
    assert_eq!(
        cases.len(),
        PathValidationLayer::ALL.len(),
        "every PathValidationLayer must have a corresponding error variant"
    );
    for (error, expected_layer) in cases {
        assert_eq!(
            error.layer(),
            expected_layer,
            "error variant should map to {expected_layer}"
        );
    }
}

#[test]
fn path_validation_error_display() {
    let err = PathValidationError::NullByte {
        path: "bad\0path".to_owned(),
    };
    assert!(
        err.to_string().contains("null byte"),
        "NullByte display should mention null byte"
    );

    let err = PathValidationError::ScopeContainment {
        path: PathBuf::from("/escape"),
        scope: MemoryScope::Project,
        expected_dir: PathBuf::from("/root/project"),
    };
    let msg = err.to_string();
    assert!(msg.contains("project"), "display should mention the scope");
    assert!(msg.contains("escapes"), "display should mention escape");
}

#[test]
fn path_validation_error_is_std_error() {
    // WHY: PathValidationError must implement std::error::Error for
    // compatibility with snafu context propagation.
    let err = PathValidationError::NullByte {
        path: String::new(),
    };
    let _: &dyn std::error::Error = &err;
}

// ---------------------------------------------------------------------------
// ValidatedPath tests
// ---------------------------------------------------------------------------

#[test]
fn validated_path_accessors() {
    // WHY: ValidatedPath's public API must expose path and scope without
    // revealing the inner PathBuf directly.
    let vp = validate_memory_path(
        Path::new("notes.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    )
    .expect("valid path should pass validation");

    assert_eq!(
        vp.as_path(),
        Path::new("/test/memory/project/notes.md"),
        "as_path should return the normalized full path"
    );
    assert_eq!(
        vp.scope(),
        MemoryScope::Project,
        "scope should return the validated scope"
    );

    let as_ref: &Path = vp.as_ref();
    assert_eq!(
        as_ref,
        Path::new("/test/memory/project/notes.md"),
        "AsRef<Path> should match as_path"
    );
}

#[test]
fn validated_path_into_path_buf() {
    let vp = validate_memory_path(
        Path::new("file.md"),
        Path::new("/test/memory"),
        MemoryScope::User,
    )
    .expect("valid path should pass validation");

    let pb = vp.into_path_buf();
    assert_eq!(
        pb,
        PathBuf::from("/test/memory/user/file.md"),
        "into_path_buf should return the inner PathBuf"
    );
}

#[test]
fn validated_path_display() {
    let vp = validate_memory_path(
        Path::new("file.md"),
        Path::new("/test/memory"),
        MemoryScope::Feedback,
    )
    .expect("valid path should pass validation");

    assert_eq!(
        vp.to_string(),
        "/test/memory/feedback/file.md",
        "Display should render the path"
    );
}

// ---------------------------------------------------------------------------
// validate_memory_path() — positive tests per scope
// ---------------------------------------------------------------------------

#[test]
fn valid_user_scope_path() {
    let result = validate_memory_path(
        Path::new("preferences.md"),
        Path::new("/test/memory"),
        MemoryScope::User,
    );
    assert!(result.is_ok(), "simple file in user scope should validate");
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::User);
    assert_eq!(vp.as_path(), Path::new("/test/memory/user/preferences.md"));
}

#[test]
fn valid_feedback_scope_path() {
    let result = validate_memory_path(
        Path::new("corrections.md"),
        Path::new("/test/memory"),
        MemoryScope::Feedback,
    );
    assert!(
        result.is_ok(),
        "simple file in feedback scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Feedback);
}

#[test]
fn valid_project_scope_path() {
    let result = validate_memory_path(
        Path::new("roadmap.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "simple file in project scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Project);
}

#[test]
fn valid_reference_scope_path() {
    let result = validate_memory_path(
        Path::new("links.md"),
        Path::new("/test/memory"),
        MemoryScope::Reference,
    );
    assert!(
        result.is_ok(),
        "simple file in reference scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Reference);
}

#[test]
fn valid_nested_subdirectory_path() {
    let result = validate_memory_path(
        Path::new("sub/dir/deep.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "nested subdirectories within scope should validate"
    );
    assert_eq!(
        result.expect("checked above").as_path(),
        Path::new("/test/memory/project/sub/dir/deep.md")
    );
}

#[test]
fn valid_path_with_dots_in_filename() {
    let result = validate_memory_path(
        Path::new("file.backup.2026.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "dots in filename (not traversal) should validate"
    );
}

// ---------------------------------------------------------------------------
// validate_memory_path() — adversarial tests per layer
// ---------------------------------------------------------------------------

// Layer 1: NullByte

#[test]
fn rejects_null_byte_in_path() {
    let path = Path::new("file\0.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "null byte should be rejected");
    let err = result.unwrap_err();
    assert_eq!(
        err.layer(),
        PathValidationLayer::NullByte,
        "error should identify NullByte layer"
    );
}

#[test]
fn rejects_null_byte_at_end() {
    let path_str = "file.md\0";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "trailing null byte should be rejected");
    assert_eq!(result.unwrap_err().layer(), PathValidationLayer::NullByte);
}

// Layer 2: Canonicalization

#[test]
fn rejects_parent_directory_traversal() {
    let path = Path::new("../user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), ".. traversal should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn rejects_deep_traversal() {
    let path = Path::new("sub/../../user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "deep .. traversal should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn rejects_backslash_in_path() {
    let path = Path::new("sub\\..\\file.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "backslash should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

// Layer 3: URL-encoded traversal

#[test]
fn rejects_url_encoded_dot() {
    let path = Path::new("%2e%2e%2fuser/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded .. should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_url_encoded_slash() {
    let path = Path::new("sub%2fparent");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded / should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_url_encoded_backslash() {
    let path = Path::new("sub%5cfile");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded \\ should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_mixed_case_url_encoding() {
    let path = Path::new("sub%2Efile");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "mixed-case URL encoding should be rejected"
    );
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

// Layer 4: Unicode normalization

#[test]
fn rejects_fullwidth_period() {
    // U+FF0E (fullwidth period) normalizes to '.' under NFKC
    let path_str = "file\u{FF0E}md";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "fullwidth period should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.layer(), PathValidationLayer::UnicodeNormalization);
    if let PathValidationError::UnicodeNormalization { offending_char, .. } = err {
        assert_eq!(offending_char, '\u{FF0E}');
    }
}

#[test]
fn rejects_fullwidth_solidus() {
    // U+FF0F (fullwidth solidus) normalizes to '/' under NFKC
    let path_str = "sub\u{FF0F}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "fullwidth solidus should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization
    );
}

#[test]
fn rejects_fullwidth_backslash() {
    // U+FF3C (fullwidth reverse solidus) normalizes to '\' under NFKC
    let path_str = "sub\u{FF3C}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "fullwidth reverse solidus should be rejected"
    );
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization
    );
}

// Layer 5: Scope containment

#[test]
fn rejects_wrong_scope_directory() {
    // WHY: A project-scope path must be under project/, not user/.
    let path = Path::new("/test/memory/user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "wrong scope directory should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::ScopeContainment
    );
}

#[test]
fn rejects_path_escaping_root() {
    let path = Path::new("/etc/passwd");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "absolute path outside root should fail");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::ScopeContainment
    );
}

#[test]
fn rejects_root_directory_itself() {
    // WHY: The memory root itself is not a valid scope path.
    let path = Path::new("/test/memory");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "root directory itself should not validate as a scope path"
    );
}

// Layers 6–7: Symlink resolution, dangling symlink, loop detection (I/O)

#[cfg(unix)]
#[test]
fn rejects_symlink_escaping_root() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    // Create a target outside root
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("create outside dir");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write secret");

    // Create symlink inside scope pointing outside root
    let link = scope_dir.join("escape");
    std::os::unix::fs::symlink(&outside, &link).expect("create symlink");

    let result = validate_memory_path(Path::new("escape"), &root, MemoryScope::Project);
    assert!(result.is_err(), "symlink escaping root should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(
            err.layer(),
            PathValidationLayer::SymlinkResolution | PathValidationLayer::ScopeContainment
        ),
        "error should be SymlinkResolution or ScopeContainment, got {:?}",
        err.layer()
    );
}

#[cfg(unix)]
#[test]
fn rejects_dangling_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    // Create symlink to nonexistent target
    let link = scope_dir.join("dangling");
    std::os::unix::fs::symlink("/nonexistent/target", &link).expect("create symlink");

    let result = validate_memory_path(Path::new("dangling"), &root, MemoryScope::Project);
    assert!(result.is_err(), "dangling symlink should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::DanglingSymlink,
        "error should identify DanglingSymlink layer"
    );
}

#[cfg(unix)]
#[test]
fn accepts_valid_symlink_within_scope() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    let sub = scope_dir.join("sub");
    std::fs::create_dir_all(&sub).expect("create sub dir");

    // Create a real file and a symlink to it within the same scope
    let real_file = sub.join("real.md");
    std::fs::write(&real_file, b"content").expect("write real file");
    let link = scope_dir.join("alias.md");
    std::os::unix::fs::symlink(&real_file, &link).expect("create symlink");

    let result = validate_memory_path(Path::new("alias.md"), &root, MemoryScope::Project);
    assert!(
        result.is_ok(),
        "symlink within scope should be accepted: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// validate_memory_path() — cross-layer combos
// ---------------------------------------------------------------------------

#[test]
fn null_byte_caught_before_traversal() {
    // WHY: Layer ordering means null byte is checked before canonicalization.
    let path = Path::new("../\0secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    // NOTE: The first layer to fire wins. Both null byte and traversal are
    // present; null byte is checked first (Layer 1).
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::NullByte,
        "null byte should fire before canonicalization"
    );
}

#[test]
fn traversal_caught_before_url_encoding() {
    // WHY: Canonicalization (Layer 2) fires before URL decoding (Layer 3).
    let path = Path::new("../%2e%2e/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization,
        "canonicalization should fire before URL-encoded traversal"
    );
}

#[test]
fn url_encoding_caught_before_unicode() {
    // WHY: URL-encoded traversal (Layer 3) fires before Unicode (Layer 4).
    let path_str = "%2e\u{FF0E}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal,
        "URL encoding should fire before unicode normalization"
    );
}

#[test]
fn unicode_caught_before_scope_containment() {
    // WHY: Unicode normalization (Layer 4) fires before scope check (Layer 5).
    let path_str = "/wrong/scope/\u{FF0E}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization,
        "unicode normalization should fire before scope containment"
    );
}

#[test]
fn scope_containment_catches_cross_scope_access() {
    // WHY: Agent in project scope cannot access user scope memories.
    for wrong_scope in &[
        MemoryScope::User,
        MemoryScope::Feedback,
        MemoryScope::Reference,
    ] {
        let path_string = format!("/test/memory/{}/secret.md", wrong_scope.as_str());
        let path = Path::new(&path_string);
        let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
        assert!(
            result.is_err(),
            "accessing {wrong_scope} from project scope should fail"
        );
        assert_eq!(
            result.unwrap_err().layer(),
            PathValidationLayer::ScopeContainment
        );
    }
}

#[test]
fn double_url_encoded_traversal() {
    // WHY: Double-encoding like %252e could bypass single-pass decoding.
    // Our check catches %2e at the first pass.
    let path = Path::new("%252e%252e%252f");
    // NOTE: %25 is '%', so %252e decodes to %2e in a two-pass decode.
    // Our single-pass check doesn't catch this, but the scope containment
    // layer will catch the resulting path if it escapes.
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    // This path is literal "%252e%252e%252f" — no traversal at string level
    // and it stays within scope, so it passes. That's acceptable because
    // the filename is literally "%252e%252e%252f" after scope_dir join.
    assert!(
        result.is_ok(),
        "double-encoded path should pass (treated as literal filename)"
    );
}

#[test]
fn backslash_plus_url_encoding_combo() {
    let path = Path::new("sub\\%2e%2e");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    // Backslash is caught first (Layer 2, Canonicalization).
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn all_scopes_accept_valid_relative_path() {
    for scope in MemoryScope::ALL {
        let result =
            validate_memory_path(Path::new("valid-file.md"), Path::new("/test/memory"), scope);
        assert!(
            result.is_ok(),
            "valid relative path should pass for {} scope: {:?}",
            scope.as_str(),
            result.err()
        );
        assert_eq!(
            result.expect("checked above").scope(),
            scope,
            "validated scope should match input"
        );
    }
}

// ---------------------------------------------------------------------------
// Verification types
// ---------------------------------------------------------------------------

#[test]
fn verification_fact_type_roundtrip() {
    let ft = FactType::Verification;
    assert_eq!(ft.as_str(), "verification");
    assert_eq!(FactType::from_str_lossy("verification"), ft);
    assert_eq!(ft.to_string(), "verification");
}

#[test]
fn verification_fact_type_serde_roundtrip() {
    let ft = FactType::Verification;
    let json = serde_json::to_string(&ft).expect("FactType serialization is infallible");
    assert_eq!(
        json, r#""verification""#,
        "should serialize as snake_case string"
    );
    let back: FactType =
        serde_json::from_str(&json).expect("FactType should deserialize from its own JSON");
    assert_eq!(
        ft, back,
        "FactType::Verification should survive serde roundtrip"
    );
}

#[test]
fn verification_source_as_str_and_parse() {
    for (variant, expected) in [
        (VerificationSource::Command, "command"),
        (VerificationSource::Query, "query"),
        (VerificationSource::Arithmetic, "arithmetic"),
        (VerificationSource::Reference, "reference"),
    ] {
        assert_eq!(variant.as_str(), expected, "{variant:?} as_str mismatch");
        assert_eq!(
            VerificationSource::from_str_opt(expected),
            Some(variant),
            "from_str_opt({expected}) should return {variant:?}"
        );
        assert_eq!(
            variant.to_string(),
            expected,
            "{variant:?} Display mismatch"
        );
    }
    assert_eq!(
        VerificationSource::from_str_opt("bogus"),
        None,
        "unknown source should return None"
    );
}

#[test]
fn verification_source_serde_roundtrip() {
    for src in [
        VerificationSource::Command,
        VerificationSource::Query,
        VerificationSource::Arithmetic,
        VerificationSource::Reference,
    ] {
        let json =
            serde_json::to_string(&src).expect("VerificationSource serialization is infallible");
        let back: VerificationSource = serde_json::from_str(&json)
            .expect("VerificationSource should deserialize from its own JSON");
        assert_eq!(
            src, back,
            "VerificationSource should survive serde roundtrip"
        );
    }
}

#[test]
fn verification_status_as_str_and_parse() {
    for (variant, expected) in [
        (VerificationStatus::Pass, "pass"),
        (VerificationStatus::Fail, "fail"),
        (VerificationStatus::Stale, "stale"),
    ] {
        assert_eq!(variant.as_str(), expected, "{variant:?} as_str mismatch");
        assert_eq!(
            VerificationStatus::from_str_opt(expected),
            Some(variant),
            "from_str_opt({expected}) should return {variant:?}"
        );
        assert_eq!(
            variant.to_string(),
            expected,
            "{variant:?} Display mismatch"
        );
    }
    assert_eq!(
        VerificationStatus::from_str_opt("unknown"),
        None,
        "unknown status should return None"
    );
}

#[test]
fn verification_status_serde_roundtrip() {
    for status in [
        VerificationStatus::Pass,
        VerificationStatus::Fail,
        VerificationStatus::Stale,
    ] {
        let json =
            serde_json::to_string(&status).expect("VerificationStatus serialization is infallible");
        let back: VerificationStatus = serde_json::from_str(&json)
            .expect("VerificationStatus should deserialize from its own JSON");
        assert_eq!(
            status, back,
            "VerificationStatus should survive serde roundtrip"
        );
    }
}

#[test]
fn verification_record_serde_roundtrip() {
    let record = VerificationRecord {
        claim: "total line count is 383".to_owned(),
        source: VerificationSource::Command,
        expected: serde_json::json!(383),
        actual: serde_json::json!(383),
        tolerance: 0.0,
        status: VerificationStatus::Pass,
        verified_at: test_timestamp("2026-03-15T10:30:00Z"),
    };
    let json =
        serde_json::to_string(&record).expect("VerificationRecord serialization is infallible");
    let back: VerificationRecord = serde_json::from_str(&json)
        .expect("VerificationRecord should deserialize from its own JSON");
    assert_eq!(back.claim, record.claim, "claim should survive roundtrip");
    assert_eq!(
        back.source, record.source,
        "source should survive roundtrip"
    );
    assert_eq!(
        back.expected, record.expected,
        "expected should survive roundtrip"
    );
    assert_eq!(
        back.actual, record.actual,
        "actual should survive roundtrip"
    );
    assert!(
        (back.tolerance - record.tolerance).abs() < f64::EPSILON,
        "tolerance should survive roundtrip"
    );
    assert_eq!(
        back.status, record.status,
        "status should survive roundtrip"
    );
    assert_eq!(
        back.verified_at, record.verified_at,
        "verified_at should survive roundtrip"
    );
}

#[test]
fn verification_record_fail_with_tolerance() {
    let record = VerificationRecord {
        claim: "build time is 120s".to_owned(),
        source: VerificationSource::Arithmetic,
        expected: serde_json::json!(120),
        actual: serde_json::json!(135),
        tolerance: 0.1,
        status: VerificationStatus::Fail,
        verified_at: test_timestamp("2026-03-15T11:00:00Z"),
    };
    let json = serde_json::to_string(&record).expect("serialization should succeed");
    assert!(
        json.contains(r#""status":"fail""#),
        "JSON should contain fail status"
    );
    assert!(
        json.contains(r#""tolerance":0.1"#),
        "JSON should contain tolerance"
    );
}
