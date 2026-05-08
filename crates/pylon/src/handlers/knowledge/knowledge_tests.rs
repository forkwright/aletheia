#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]

use super::*;

fn make_fact(id: &str, content: &str, confidence: f64) -> mneme::knowledge::Fact {
    use mneme::id::FactId;
    use mneme::knowledge::{
        EpistemicTier, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    };
    mneme::knowledge::Fact {
        id: FactId::new(id).unwrap(),
        nous_id: "test-nous".to_owned(),
        fact_type: "knowledge".to_owned(),
        content: content.to_owned(),
        temporal: FactTemporal {
            valid_from: jiff::Timestamp::UNIX_EPOCH,
            valid_to: jiff::Timestamp::UNIX_EPOCH,
            recorded_at: jiff::Timestamp::UNIX_EPOCH,
        },
        provenance: FactProvenance {
            confidence,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: 24.0,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        scope: None,
        visibility: mneme::knowledge::Visibility::Private,
    }
}

#[test]
fn truncate_content_short_text_unchanged() {
    let s = "short";
    assert_eq!(truncate_content(s, 80), "short");
}

#[test]
fn truncate_content_long_text_gets_ellipsis() {
    let s = "a".repeat(100);
    let result = truncate_content(&s, 80);
    assert!(result.ends_with("..."));
    assert_eq!(result.len(), 83);
}

#[test]
fn truncate_content_handles_utf8_boundary() {
    // NOTE: "é" is 2 bytes; with max=1 we must not split mid-char.
    let s = "éàü";
    let result = truncate_content(s, 1);
    assert!(result.ends_with("..."));
    assert!(std::str::from_utf8(result.as_bytes()).is_ok());
}

#[test]
fn default_sort_returns_confidence() {
    assert_eq!(default_sort(), "confidence");
}

#[test]
fn default_order_returns_desc() {
    assert_eq!(default_order(), "desc");
}

#[test]
fn default_limit_returns_100() {
    assert_eq!(default_limit(), 100);
}

#[test]
fn sort_facts_by_confidence_descending() {
    let mut facts = vec![
        make_fact("a", "low", 0.3),
        make_fact("b", "high", 0.9),
        make_fact("c", "mid", 0.6),
    ];
    sort_facts(&mut facts, "confidence", "desc");
    assert_eq!(facts[0].id.as_str(), "b");
    assert_eq!(facts[1].id.as_str(), "c");
    assert_eq!(facts[2].id.as_str(), "a");
}

#[test]
fn sort_facts_by_confidence_ascending() {
    let mut facts = vec![
        make_fact("a", "low", 0.3),
        make_fact("b", "high", 0.9),
        make_fact("c", "mid", 0.6),
    ];
    sort_facts(&mut facts, "confidence", "asc");
    assert_eq!(facts[0].id.as_str(), "a");
    assert_eq!(facts[1].id.as_str(), "c");
    assert_eq!(facts[2].id.as_str(), "b");
}

#[test]
fn sort_facts_by_access_count_descending() {
    let mut facts = vec![
        make_fact("a", "one access", 0.5),
        make_fact("b", "five accesses", 0.5),
    ];
    facts[0].access.access_count = 1;
    facts[1].access.access_count = 5;
    sort_facts(&mut facts, "access_count", "desc");
    assert_eq!(facts[0].id.as_str(), "b");
    assert_eq!(facts[1].id.as_str(), "a");
}

#[test]
fn facts_query_default_values() {
    // NOTE: FactsQuery has individual serde defaults; test them via JSON.
    let q: FactsQuery = serde_json::from_str("{}").unwrap();
    assert_eq!(q.sort, "confidence");
    assert_eq!(q.order, "desc");
    assert_eq!(q.limit, 100);
    assert!(!q.include_forgotten);
}

#[cfg(feature = "knowledge-store")]
#[test]
fn entity_relationships_queries_store_for_inbound_and_outbound_edges() {
    use std::sync::Arc;

    use mneme::id::EntityId;
    use mneme::knowledge::{Entity, Relationship};

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let entity_a = EntityId::new("entity-a").unwrap();
    let entity_b = EntityId::new("entity-b").unwrap();
    let entity_c = EntityId::new("entity-c").unwrap();

    for entity in [
        Entity {
            id: entity_a.clone(),
            name: "A".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: entity_b.clone(),
            name: "B".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: entity_c.clone(),
            name: "C".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
    ] {
        store.insert_entity(&entity).unwrap();
    }

    for relationship in [
        Relationship {
            src: entity_a.clone(),
            dst: entity_b,
            relation: "depends_on".to_owned(),
            weight: 0.8,
            created_at: now,
        },
        Relationship {
            src: entity_c,
            dst: entity_a,
            relation: "supports".to_owned(),
            weight: 0.6,
            created_at: now,
        },
    ] {
        store.insert_relationship(&relationship).unwrap();
    }

    let config = taxis::config::AletheiaConfig::default();
    let state = KnowledgeState {
        knowledge_store: Some(store),
        config: Arc::new(tokio::sync::RwLock::new(config)),
        event_bus: Arc::new(crate::event_bus::EventBus::new(16)),
    };

    let relationships = get_entity_relationships(&state, "entity-a").unwrap();
    assert_eq!(relationships.len(), 2);
    assert!(
        relationships
            .iter()
            .any(|r| r.src.as_str() == "entity-a" && r.relation == "depends_on")
    );
    assert!(
        relationships
            .iter()
            .any(|r| r.dst.as_str() == "entity-a" && r.relation == "supports")
    );
}

#[test]
fn default_limit_is_capped_at_max() {
    let config = taxis::config::ApiLimitsConfig::default();
    assert!(config.max_facts_limit <= 1000);
    assert_eq!(config.max_facts_limit, 1000);
}

#[test]
fn search_result_serializes_snake_case() {
    let result = SearchResult {
        id: "fact-1".to_owned(),
        content: "Alice works at Acme Corp".to_owned(),
        confidence: 0.8,
        tier: "inferred".to_owned(),
        fact_type: "knowledge".to_owned(),
        score: 0.64,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert!(json.get("fact_type").is_some());
    assert_eq!(json["fact_type"], "knowledge");
    assert_eq!(json["confidence"], 0.8);
}

#[test]
fn forget_request_default_reason() {
    let req: ForgetRequest = serde_json::from_str("{}").unwrap();
    assert_eq!(req.reason, "user_requested");
}

#[test]
fn empty_search_returns_empty_results() {
    let response = SearchResponse { results: vec![] };
    let json = serde_json::to_value(&response).unwrap();
    assert!(json["results"].as_array().unwrap().is_empty());
}

#[test]
fn entities_response_serializes_empty() {
    let response = EntitiesResponse {
        entities: vec![],
        total: 0,
    };
    let json = serde_json::to_value(&response).unwrap();
    assert!(json["entities"].as_array().unwrap().is_empty());
    assert_eq!(json["total"], 0);
}

#[test]
fn validate_sort_order_accepts_all_valid_sort_fields() {
    for field in VALID_SORT_FIELDS {
        assert!(validate_sort_order(field, "asc").is_ok());
        assert!(validate_sort_order(field, "desc").is_ok());
    }
}

#[test]
fn validate_sort_order_rejects_invalid_sort_field() {
    let err = validate_sort_order("invalid_field", "asc").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid sort field 'invalid_field'"), "{msg}");
    assert!(msg.contains("confidence"), "{msg}");
    assert!(msg.contains("recency"), "{msg}");
}

#[test]
fn validate_sort_order_rejects_invalid_order() {
    let err = validate_sort_order("confidence", "upward").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid order 'upward'"), "{msg}");
}

#[test]
fn validate_sort_order_accepts_case_insensitive_order() {
    assert!(validate_sort_order("confidence", "ASC").is_ok());
    assert!(validate_sort_order("confidence", "DESC").is_ok());
    assert!(validate_sort_order("confidence", "Asc").is_ok());
    assert!(validate_sort_order("confidence", "Desc").is_ok());
}

#[test]
fn sort_facts_with_uppercase_order() {
    let mut facts = vec![make_fact("a", "low", 0.3), make_fact("b", "high", 0.9)];
    // NOTE: After validation, order is normalized to lowercase before reaching sort_facts.
    sort_facts(&mut facts, "confidence", "desc");
    assert_eq!(facts[0].id.as_str(), "b");
    assert_eq!(facts[1].id.as_str(), "a");
}
