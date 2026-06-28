#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]

use super::*;

#[cfg(feature = "knowledge-store")]
use axum::http::StatusCode;
use symbolon::types::Role;

use crate::event_bus::EventBus;
use crate::extract::Claims;
use crate::state::KnowledgeState;

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
        project_id: None,
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

#[test]
fn entities_query_default_values() {
    let q: EntitiesQuery = serde_json::from_str("{}").unwrap();
    assert_eq!(q.sort, "page_rank");
    assert_eq!(q.order, "desc");
    assert!(q.entity_type.is_empty());
    assert!(q.agent.is_empty());
    assert!(q.min_confidence.is_none());
}

#[cfg(feature = "knowledge-store")]
#[test]
fn entity_relationships_projects_view_fields() {
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

    let claims = operator_claims();
    let policy = KnowledgeReadPolicy::from_single_nous(&claims, None).unwrap();
    let relationships = get_entity_relationships(&state, &policy, "entity-a").unwrap();
    assert_eq!(relationships.len(), 2);
    assert!(relationships.iter().any(|r| {
        r.entity_id == "entity-b"
            && r.entity_name == "B"
            && r.relationship_type == "depends_on"
            && r.direction == RelationshipDirection::Outgoing
            && (r.confidence - 0.8).abs() < f64::EPSILON
    }));
    assert!(relationships.iter().any(|r| {
        r.entity_id == "entity-c"
            && r.entity_name == "C"
            && r.relationship_type == "supports"
            && r.direction == RelationshipDirection::Incoming
            && (r.confidence - 0.6).abs() < f64::EPSILON
    }));
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn list_entities_honors_filters_and_relationship_counts() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use axum::extract::{Query, State};
    use mneme::engine::DataValue;
    use mneme::id::EntityId;
    use mneme::knowledge::{Entity, Relationship};

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let entity_a = EntityId::new("entity-a").unwrap();
    let entity_b = EntityId::new("entity-b").unwrap();

    for entity in [
        Entity {
            id: entity_a.clone(),
            name: "Alice".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec!["A".to_owned()],
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: entity_b.clone(),
            name: "Widget".to_owned(),
            entity_type: "tool".to_owned(),
            aliases: vec!["Gizmo".to_owned()],
            created_at: now,
            updated_at: now,
        },
    ] {
        store.insert_entity(&entity).unwrap();
    }

    store
        .insert_relationship(&Relationship {
            src: entity_a.clone(),
            dst: entity_b,
            relation: "uses".to_owned(),
            weight: 0.9,
            created_at: now,
        })
        .unwrap();

    let fact = make_fact("fact-alice", "Alice manages the workspace", 0.95);
    store.insert_fact(&fact).unwrap();
    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        DataValue::Str("fact-alice".to_owned().into()),
    );
    params.insert(
        "entity_id".to_owned(),
        DataValue::Str("entity-a".to_owned().into()),
    );
    params.insert(
        "created_at".to_owned(),
        DataValue::Str(now.to_string().into()),
    );
    store
        .run_mut_query(
            r"
                ?[fact_id, entity_id, created_at] <- [[ $fact_id, $entity_id, $created_at ]]
                :put fact_entities { fact_id, entity_id => created_at }
            ",
            params,
        )
        .unwrap();

    let config = taxis::config::AletheiaConfig::default();
    let state = KnowledgeState {
        knowledge_store: Some(store),
        config: Arc::new(tokio::sync::RwLock::new(config)),
        event_bus: Arc::new(crate::event_bus::EventBus::new(16)),
    };

    let query = EntitiesQuery {
        limit: 50,
        offset: 0,
        q: Some("ali".to_owned()),
        sort: "name".to_owned(),
        order: "asc".to_owned(),
        entity_type: vec!["person".to_owned()],
        min_confidence: Some(0.8),
        agent: vec!["test-nous".to_owned()],
    };

    let response = match list_entities(State(state), operator_claims(), Query(query)).await {
        Ok(response) => response,
        Err(err) => panic!("list entities: {err:?}"),
    };
    assert_eq!(response.0.total, 1);
    assert_eq!(response.0.entities.len(), 1);
    let entity = &response.0.entities[0];
    assert_eq!(entity.id, "entity-a");
    assert_eq!(entity.name, "Alice");
    assert_eq!(entity.relationship_count, 1);
    assert!(entity.confidence >= 0.8);
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn get_entity_missing_returns_404() {
    use std::sync::Arc;

    use axum::extract::{Path, State};

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let config = taxis::config::AletheiaConfig::default();
    let state = KnowledgeState {
        knowledge_store: Some(store),
        config: Arc::new(tokio::sync::RwLock::new(config)),
        event_bus: Arc::new(crate::event_bus::EventBus::new(16)),
    };

    let Err(err) = get_entity(
        State(state),
        operator_claims(),
        Path("missing-entity".to_owned()),
    )
    .await
    else {
        panic!("missing entity should return an error");
    };
    match err {
        ApiError::NotFound { path, .. } => {
            assert_eq!(path, "entity/missing-entity");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn update_sensitivity_handler_persists_to_fact_list_path() {
    use std::sync::Arc;

    use axum::Json;
    use axum::extract::{Path, State};
    use symbolon::types::Role;

    use crate::extract::Claims;
    use crate::state::KnowledgeState;

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let fact = make_fact("fact-sensitive", "Alice handles payroll", 0.95);
    store.insert_fact(&fact).unwrap();

    let config = taxis::config::AletheiaConfig::default();
    let state = KnowledgeState {
        knowledge_store: Some(Arc::clone(&store)),
        config: Arc::new(tokio::sync::RwLock::new(config)),
        event_bus: Arc::new(crate::event_bus::EventBus::new(16)),
    };
    let claims = Claims {
        sub: "alice".to_owned(),
        role: Role::Operator,
        nous_id: None,
    };

    let response = match update_sensitivity(
        State(state.clone()),
        claims,
        Path("fact-sensitive".to_owned()),
        Json(UpdateSensitivityRequest {
            sensitivity: "confidential".to_owned(),
        }),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => panic!("update sensitivity: {error:?}"),
    };

    assert_eq!(response.0["status"], "updated");
    assert_eq!(response.0["sensitivity"], "confidential");

    let listed = match list_facts(
        State(state),
        operator_claims(),
        Query(FactsQuery {
            nous_id: Some("test-nous".to_owned()),
            sort: "confidence".to_owned(),
            order: "desc".to_owned(),
            filter: None,
            fact_type: None,
            tier: None,
            limit: 10,
            offset: 0,
            include_forgotten: false,
        }),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => panic!("list facts: {error:?}"),
    };

    assert_eq!(listed.0.facts.len(), 1);
    assert_eq!(
        listed.0.facts[0].sensitivity,
        mneme::knowledge::FactSensitivity::Confidential
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

fn operator_claims() -> Claims {
    Claims {
        sub: "alice".to_owned(),
        role: Role::Operator,
        nous_id: None,
    }
}

fn readonly_claims() -> Claims {
    Claims {
        sub: "bob".to_owned(),
        role: Role::Readonly,
        nous_id: None,
    }
}

#[cfg(feature = "knowledge-store")]
fn scoped_agent_claims(nous_id: &str) -> Claims {
    Claims {
        sub: format!("{nous_id}-user"),
        role: Role::Agent,
        nous_id: Some(nous_id.to_owned()),
    }
}

#[cfg(feature = "knowledge-store")]
fn knowledge_state_with_store(
    store: std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> KnowledgeState {
    KnowledgeState {
        #[cfg(feature = "knowledge-store")]
        knowledge_store: Some(store),
        config: std::sync::Arc::new(tokio::sync::RwLock::new(
            taxis::config::AletheiaConfig::default(),
        )),
        event_bus: std::sync::Arc::new(EventBus::new(16)),
    }
}

#[cfg(feature = "knowledge-store")]
fn make_fact_for(
    id: &str,
    nous_id: &str,
    content: &str,
    visibility: mneme::knowledge::Visibility,
) -> mneme::knowledge::Fact {
    let mut fact = make_fact(id, content, 0.9);
    fact.nous_id = nous_id.to_owned();
    fact.visibility = visibility;
    fact
}

#[cfg(feature = "knowledge-store")]
fn default_facts_query(nous_id: Option<&str>) -> FactsQuery {
    FactsQuery {
        nous_id: nous_id.map(ToOwned::to_owned),
        sort: "confidence".to_owned(),
        order: "desc".to_owned(),
        filter: None,
        fact_type: None,
        tier: None,
        limit: 50,
        offset: 0,
        include_forgotten: false,
    }
}

#[cfg(feature = "knowledge-store")]
fn seed_policy_store() -> (
    std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    KnowledgeState,
) {
    use mneme::id::{EntityId, FactId};
    use mneme::knowledge::{Entity, Visibility};

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    for entity in [
        Entity {
            id: EntityId::new("entity-alice").unwrap(),
            name: "Alice Memory".to_owned(),
            entity_type: "person".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: EntityId::new("entity-bob").unwrap(),
            name: "Bob Memory".to_owned(),
            entity_type: "person".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: EntityId::new("entity-shared").unwrap(),
            name: "Shared Memory".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
    ] {
        store.insert_entity(&entity).unwrap();
    }

    for (fact, entity_id) in [
        (
            make_fact_for(
                "fact-alice-private",
                "alice-nous",
                "zebra alice private memory",
                Visibility::Private,
            ),
            "entity-alice",
        ),
        (
            make_fact_for(
                "fact-bob-private",
                "bob-nous",
                "zebra bob private memory",
                Visibility::Private,
            ),
            "entity-bob",
        ),
        (
            make_fact_for(
                "fact-bob-shared",
                "bob-nous",
                "zebra bob shared memory",
                Visibility::Shared,
            ),
            "entity-shared",
        ),
    ] {
        store.insert_fact(&fact).unwrap();
        store
            .insert_fact_entity(
                &FactId::new(fact.id.as_str()).unwrap(),
                &EntityId::new(entity_id).unwrap(),
            )
            .unwrap();
    }

    let state = knowledge_state_with_store(std::sync::Arc::clone(&store));
    (store, state)
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn knowledge_read_policy_applies_across_fact_search_timeline_and_entity_reads() {
    use axum::extract::{Path, Query, State};
    use mneme::knowledge::Visibility;

    let (_store, state) = seed_policy_store();

    let operator_list = list_facts(
        State(state.clone()),
        operator_claims(),
        Query(default_facts_query(None)),
    )
    .await
    .unwrap();
    assert_eq!(operator_list.0.facts.len(), 3);

    let readonly_list = list_facts(
        State(state.clone()),
        readonly_claims(),
        Query(default_facts_query(None)),
    )
    .await
    .unwrap();
    assert_eq!(readonly_list.0.facts.len(), 1);
    assert_eq!(readonly_list.0.facts[0].id.as_str(), "fact-bob-shared");

    let alice_claims = scoped_agent_claims("alice-nous");
    let alice_list = list_facts(
        State(state.clone()),
        alice_claims.clone(),
        Query(default_facts_query(None)),
    )
    .await
    .unwrap();
    assert_eq!(alice_list.0.facts.len(), 1);
    assert_eq!(alice_list.0.facts[0].id.as_str(), "fact-alice-private");

    let override_err = list_facts(
        State(state.clone()),
        alice_claims.clone(),
        Query(default_facts_query(Some("bob-nous"))),
    )
    .await
    .unwrap_err();
    assert!(matches!(override_err, ApiError::Forbidden { .. }));

    let private_fact_err = get_fact(
        State(state.clone()),
        alice_claims.clone(),
        Path("fact-bob-private".to_owned()),
    )
    .await
    .unwrap_err();
    assert!(matches!(private_fact_err, ApiError::Forbidden { .. }));

    let shared_fact = get_fact(
        State(state.clone()),
        alice_claims.clone(),
        Path("fact-bob-shared".to_owned()),
    )
    .await
    .unwrap();
    assert_eq!(shared_fact.0.fact.visibility, Visibility::Shared);

    let search_err = search(
        State(state.clone()),
        alice_claims.clone(),
        Query(SearchQuery {
            q: "zebra".to_owned(),
            nous_id: Some("bob-nous".to_owned()),
            limit: 10,
        }),
    )
    .await
    .unwrap_err();
    assert!(matches!(search_err, ApiError::Forbidden { .. }));

    let timeline_response = timeline(
        State(state.clone()),
        alice_claims.clone(),
        Query(TimelineQuery {
            nous_id: None,
            limit: 20,
            offset: 0,
        }),
    )
    .await
    .unwrap();
    assert_eq!(timeline_response.0.events.len(), 1);
    assert_eq!(timeline_response.0.events[0].fact_id, "fact-alice-private");

    let entity_err = list_entities(
        State(state),
        alice_claims,
        Query(EntitiesQuery {
            limit: 20,
            offset: 0,
            q: None,
            sort: "name".to_owned(),
            order: "asc".to_owned(),
            entity_type: Vec::new(),
            min_confidence: None,
            agent: vec!["bob-nous".to_owned()],
        }),
    )
    .await
    .unwrap_err();
    assert!(matches!(entity_err, ApiError::Forbidden { .. }));
}

fn knowledge_state_without_store() -> KnowledgeState {
    KnowledgeState {
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
        config: std::sync::Arc::new(tokio::sync::RwLock::new(
            taxis::config::AletheiaConfig::default(),
        )),
        event_bus: std::sync::Arc::new(EventBus::new(16)),
    }
}

#[tokio::test]
async fn flag_entity_insufficient_role_returns_403() {
    let state = knowledge_state_without_store();
    let err = flag_entity(
        State(state),
        readonly_claims(),
        Path("entity-a".to_owned()),
        Json(FlagRequest {
            reason: "test".to_owned(),
            severity: FlagSeverity::Low,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::Forbidden { .. }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn flag_entity_empty_reason_returns_400() {
    let state = knowledge_state_without_store();
    let err = flag_entity(
        State(state),
        operator_claims(),
        Path("entity-a".to_owned()),
        Json(FlagRequest {
            reason: "   ".to_owned(),
            severity: FlagSeverity::Low,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::BadRequest { .. }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn flag_entity_no_store_returns_503() {
    let state = knowledge_state_without_store();
    let err = flag_entity(
        State(state),
        operator_claims(),
        Path("entity-a".to_owned()),
        Json(FlagRequest {
            reason: "missing store".to_owned(),
            severity: FlagSeverity::Low,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::ServiceUnavailable { .. }),
        "unexpected error: {err:?}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn flag_entity_missing_entity_returns_404() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let state = knowledge_state_with_store(store);
    let err = flag_entity(
        State(state),
        operator_claims(),
        Path("missing-entity".to_owned()),
        Json(FlagRequest {
            reason: "not found".to_owned(),
            severity: FlagSeverity::Medium,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::NotFound { .. }),
        "unexpected error: {err:?}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn flag_entity_persists_review_data() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mneme::engine::DataValue;
    use mneme::id::EntityId;
    use mneme::knowledge::Entity;

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let entity = Entity {
        id: EntityId::new("entity-a").unwrap(),
        name: "A".to_owned(),
        entity_type: "concept".to_owned(),
        aliases: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store.insert_entity(&entity).unwrap();

    let state = knowledge_state_with_store(Arc::clone(&store));
    let response = flag_entity(
        State(state),
        operator_claims(),
        Path("entity-a".to_owned()),
        Json(FlagRequest {
            reason: "duplicate name".to_owned(),
            severity: FlagSeverity::High,
        }),
    )
    .await
    .unwrap();
    assert_eq!(response, StatusCode::NO_CONTENT);

    let mut params = BTreeMap::new();
    params.insert(
        "entity_id".to_owned(),
        DataValue::Str("entity-a".to_owned().into()),
    );
    let rows = store
        .run_query(
            r"?[reason, severity, flagged_by] :=
                *entity_flags{entity_id, reason, severity, flagged_by, flagged_at},
                entity_id = $entity_id",
            params,
        )
        .unwrap();
    assert_eq!(rows.row_count(), 1);
    assert_eq!(rows.get_string(0, "reason").unwrap(), "duplicate name");
    assert_eq!(rows.get_string(0, "severity").unwrap(), "high");
    assert_eq!(rows.get_string(0, "flagged_by").unwrap(), "alice");
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn merge_entities_transfers_facts_and_removes_merged() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mneme::engine::DataValue;
    use mneme::id::{EntityId, FactId};
    use mneme::knowledge::Entity;

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let canonical_id = EntityId::new("entity-a").unwrap();
    let merged_id = EntityId::new("entity-b").unwrap();
    for entity in [
        Entity {
            id: canonical_id.clone(),
            name: "A".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: merged_id.clone(),
            name: "B".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
    ] {
        store.insert_entity(&entity).unwrap();
    }

    let fact = make_fact("fact-b", "fact owned by b", 0.9);
    store.insert_fact(&fact).unwrap();
    store
        .insert_fact_entity(&FactId::new("fact-b").unwrap(), &merged_id)
        .unwrap();

    let state = knowledge_state_with_store(Arc::clone(&store));
    let response = merge_entities(
        State(state),
        operator_claims(),
        Json(MergeRequest {
            canonical_id: "entity-a".to_owned(),
            merged_id: "entity-b".to_owned(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(response, StatusCode::NO_CONTENT);

    let entities = store.list_entities().unwrap();
    assert!(entities.iter().any(|e| e.id.as_str() == "entity-a"));
    assert!(!entities.iter().any(|e| e.id.as_str() == "entity-b"));

    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        DataValue::Str("fact-b".to_owned().into()),
    );
    let rows = store
        .run_query(
            r"?[entity_id] := *fact_entities{fact_id, entity_id}, fact_id = $fact_id",
            params,
        )
        .unwrap();
    assert_eq!(rows.row_count(), 1);
    assert_eq!(rows.get_string(0, "entity_id").unwrap(), "entity-a");
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn merge_entities_missing_canonical_returns_404() {
    use mneme::id::EntityId;
    use mneme::knowledge::Entity;

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    store
        .insert_entity(&Entity {
            id: EntityId::new("entity-b").unwrap(),
            name: "B".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        })
        .unwrap();

    let state = knowledge_state_with_store(store);
    let err = merge_entities(
        State(state),
        operator_claims(),
        Json(MergeRequest {
            canonical_id: "entity-a".to_owned(),
            merged_id: "entity-b".to_owned(),
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::NotFound { .. }),
        "unexpected error: {err:?}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn merge_entities_same_id_returns_400() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let state = knowledge_state_with_store(store);
    let err = merge_entities(
        State(state),
        operator_claims(),
        Json(MergeRequest {
            canonical_id: "entity-a".to_owned(),
            merged_id: "entity-a".to_owned(),
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::BadRequest { .. }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn merge_entities_no_store_returns_503() {
    let state = knowledge_state_without_store();
    let err = merge_entities(
        State(state),
        operator_claims(),
        Json(MergeRequest {
            canonical_id: "entity-a".to_owned(),
            merged_id: "entity-b".to_owned(),
        }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::ServiceUnavailable { .. }),
        "unexpected error: {err:?}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn delete_entity_removes_relationships_and_flags() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mneme::engine::DataValue;
    use mneme::id::EntityId;
    use mneme::knowledge::{Entity, Relationship};

    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let entity_a = EntityId::new("entity-a").unwrap();
    let entity_b = EntityId::new("entity-b").unwrap();
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
    ] {
        store.insert_entity(&entity).unwrap();
    }
    store
        .insert_relationship(&Relationship {
            src: entity_a.clone(),
            dst: entity_b.clone(),
            relation: "depends_on".to_owned(),
            weight: 0.8,
            created_at: now,
        })
        .unwrap();
    store
        .flag_entity(&entity_b, "review", "low", "alice")
        .unwrap();

    let state = knowledge_state_with_store(Arc::clone(&store));
    let response = delete_entity(State(state), operator_claims(), Path("entity-b".to_owned()))
        .await
        .unwrap();
    assert_eq!(response, StatusCode::NO_CONTENT);

    let entities = store.list_entities().unwrap();
    assert!(!entities.iter().any(|e| e.id.as_str() == "entity-b"));

    let mut params = BTreeMap::new();
    params.insert(
        "entity_id".to_owned(),
        DataValue::Str("entity-b".to_owned().into()),
    );
    let rows = store
        .run_query(
            r"?[entity_id] := *entity_flags{entity_id, reason, severity, flagged_by, flagged_at}, entity_id = $entity_id",
            params,
        )
        .unwrap();
    assert_eq!(rows.row_count(), 0);

    let rows = store
        .run_query(
            r"?[src, dst] := *relationships{src, dst}, src = 'entity-a', dst = 'entity-b'",
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(rows.row_count(), 0);
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn delete_entity_missing_returns_404() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().unwrap();
    let state = knowledge_state_with_store(store);
    let err = delete_entity(State(state), operator_claims(), Path("missing".to_owned()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::NotFound { .. }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn delete_entity_no_store_returns_503() {
    let state = knowledge_state_without_store();
    let err = delete_entity(State(state), operator_claims(), Path("entity-a".to_owned()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::ServiceUnavailable { .. }),
        "unexpected error: {err:?}"
    );
}
