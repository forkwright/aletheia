//! (See parent module for full rationale.)

use super::super::*;
use super::test_timestamp;
use crate::id::{EmbeddingId, EntityId, FactId};

// Split: EntityId / EmbeddingId / FactId / scalar enum types + forget reason serde.

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
        project_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
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
fn fact_sensitivity_serde_roundtrip() {
    // WHY (#3404, #3413): every variant must survive serde roundtrip, and
    // legacy Fact JSON without the field must default to `Public`.
    for s in [
        FactSensitivity::Public,
        FactSensitivity::Internal,
        FactSensitivity::Confidential,
    ] {
        let json = serde_json::to_string(&s).expect("FactSensitivity serializes");
        let back: FactSensitivity =
            serde_json::from_str(&json).expect("FactSensitivity deserializes");
        assert_eq!(s, back, "{s:?} must survive serde roundtrip");
    }

    let legacy_json = r#"{
        "id": "fact-legacy",
        "nous_id": "syn",
        "fact_type": "",
        "content": "legacy fact",
        "valid_from": "2026-02-01T00:00:00Z",
        "valid_to": "9999-01-01T00:00:00Z",
        "recorded_at": "2026-02-01T00:00:00Z",
        "confidence": 0.5,
        "tier": "inferred",
        "stability_hours": 720.0,
        "is_forgotten": false,
        "access_count": 0
    }"#;
    let fact: Fact = serde_json::from_str(legacy_json)
        .expect("legacy Fact JSON without sensitivity must deserialize via serde default");
    assert_eq!(
        fact.sensitivity,
        FactSensitivity::Public,
        "missing sensitivity must default to Public for backward compat"
    );
}

#[test]
fn fact_sensitivity_ordering() {
    // WHY: the sovereignty filter reduces to `sensitivity <= max_allowed`,
    // so `Public < Internal < Confidential` is load-bearing.
    assert!(FactSensitivity::Public < FactSensitivity::Internal);
    assert!(FactSensitivity::Internal < FactSensitivity::Confidential);
    assert!(FactSensitivity::Public < FactSensitivity::Confidential);
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
        sensitivity: FactSensitivity::Public,
        graph_importance: 0.0,
        scope: None,
        project_id: None,
        visibility: Visibility::Private,
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
        project_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
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
        project_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
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
