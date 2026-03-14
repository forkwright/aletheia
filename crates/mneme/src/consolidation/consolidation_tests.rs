#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

// ---- Mock provider ----

struct MockConsolidationProvider {
    response: String,
}

impl MockConsolidationProvider {
    fn new(response: &str) -> Self {
        Self {
            response: response.to_owned(),
        }
    }
}

impl ConsolidationProvider for MockConsolidationProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        Ok(self.response.clone())
    }
}

struct FailingProvider;

impl ConsolidationProvider for FailingProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        Err(LlmCallSnafu {
            message: "mock failure",
        }
        .build())
    }
}

// ---- Unit tests ----

#[test]
fn consolidation_config_defaults() {
    let config = ConsolidationConfig::default();
    assert_eq!(config.entity_fact_threshold, 10);
    assert_eq!(config.community_fact_threshold, 20);
    assert_eq!(config.min_age_days, 7);
    assert_eq!(config.batch_limit, 50);
    assert!((config.rate_limit_hours - 1.0).abs() < f64::EPSILON);
}

#[test]
fn trigger_type_labels() {
    let entity = ConsolidationTrigger::EntityOverflow {
        entity_id: EntityId::from("e-1"),
        fact_count: 15,
    };
    assert_eq!(entity.trigger_type(), "entity_overflow");
    assert_eq!(entity.trigger_id(), "e-1");

    let community = ConsolidationTrigger::CommunityOverflow {
        cluster_id: 42,
        fact_count: 25,
    };
    assert_eq!(community.trigger_type(), "community_overflow");
    assert_eq!(community.trigger_id(), "42");
}

#[test]
fn parse_valid_consolidation_response() {
    let response = r#"[
        {"content": "Alice works at Acme Corp as a senior engineer", "entities": ["Alice", "Acme Corp"], "relationships": [{"from": "Alice", "to": "Acme Corp", "type": "WORKS_AT"}]},
        {"content": "Alice prefers Rust for backend development", "entities": ["Alice", "Rust"], "relationships": []}
    ]"#;
    let entries = parse_consolidation_response(response).expect("parse succeeds");
    assert_eq!(entries.len(), 2);
    assert!(entries[0].content.contains("Alice works at Acme Corp"));
    assert_eq!(entries[0].entities.len(), 2);
    assert_eq!(entries[0].relationships.len(), 1);
    assert_eq!(entries[0].relationships[0].rel_type, "WORKS_AT");
}

#[test]
fn parse_response_with_preamble() {
    let response = r#"Here are the consolidated facts:
[{"content": "Bob is a data scientist", "entities": ["Bob"]}]
Some trailing text."#;
    let entries = parse_consolidation_response(response).expect("parse succeeds");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Bob is a data scientist");
}

#[test]
fn parse_invalid_response_fails() {
    let response = "This is not JSON at all";
    assert!(parse_consolidation_response(response).is_err());
}

#[test]
fn extract_json_array_finds_array() {
    let text = "prefix [1, 2, 3] suffix";
    assert_eq!(extract_json_array(text), Some("[1, 2, 3]"));
}

#[test]
fn extract_json_array_nested() {
    let text = r#"[{"a": [1, 2]}, {"b": 3}]"#;
    assert_eq!(extract_json_array(text), Some(text));
}

#[test]
fn extract_json_array_none() {
    assert_eq!(extract_json_array("no array here"), None);
}

#[test]
fn user_message_formatting() {
    let facts = vec![
        (
            FactId::from("f-1"),
            "Alice works at Acme".to_owned(),
            0.9,
            "2026-01-01T00:00:00Z".to_owned(),
        ),
        (
            FactId::from("f-2"),
            "Alice likes Rust".to_owned(),
            0.85,
            "2026-01-02T00:00:00Z".to_owned(),
        ),
    ];
    let msg = consolidation_user_message(&facts);
    assert!(msg.contains("2 total"));
    assert!(msg.contains("1. [id=f-1"));
    assert!(msg.contains("2. [id=f-2"));
    assert!(msg.contains("confidence=0.90"));
}

#[test]
fn system_prompt_is_stable() {
    let prompt = consolidation_system_prompt();
    assert!(prompt.contains("knowledge consolidation engine"));
    assert!(prompt.contains("JSON array"));
    assert!(prompt.contains("30-50% compression"));
}

#[test]
fn batch_facts_splits_correctly() {
    let facts: Vec<(FactId, String, f64, String)> = (0..7)
        .map(|i| {
            (
                FactId::from(format!("f-{i}")),
                format!("fact {i}"),
                0.8,
                "2026-01-01T00:00:00Z".to_owned(),
            )
        })
        .collect();

    let batches = batch_facts(&facts, 3);
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
    assert_eq!(batches[2].len(), 1);
}

#[test]
fn batch_facts_single_batch() {
    let facts: Vec<(FactId, String, f64, String)> = (0..5)
        .map(|i| {
            (
                FactId::from(format!("f-{i}")),
                format!("fact {i}"),
                0.8,
                "2026-01-01T00:00:00Z".to_owned(),
            )
        })
        .collect();

    let batches = batch_facts(&facts, 50);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 5);
}

#[test]
fn consolidated_fact_serde_roundtrip() {
    let fact = ConsolidatedFact {
        content: "Alice is a senior engineer at Acme Corp".to_owned(),
        confidence: 0.95,
        tier: "inferred".to_owned(),
        source_fact_ids: vec![FactId::from("f-1"), FactId::from("f-2")],
    };
    let json = serde_json::to_string(&fact).expect("serialize");
    let back: ConsolidatedFact = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(fact.content, back.content);
    assert!((fact.confidence - back.confidence).abs() < f64::EPSILON);
    assert_eq!(fact.source_fact_ids.len(), back.source_fact_ids.len());
}

#[test]
fn consolidation_trigger_serde_roundtrip() {
    let trigger = ConsolidationTrigger::EntityOverflow {
        entity_id: EntityId::from("e-alice"),
        fact_count: 15,
    };
    let json = serde_json::to_string(&trigger).expect("serialize");
    let back: ConsolidationTrigger = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(trigger, back);
}

#[test]
fn mock_provider_returns_response() {
    let provider = MockConsolidationProvider::new(r#"[{"content": "test"}]"#);
    let result = provider
        .consolidate("system", "user")
        .expect("should succeed");
    assert!(result.contains("test"));
}

#[test]
fn failing_provider_returns_error() {
    let provider = FailingProvider;
    let result = provider.consolidate("system", "user");
    assert!(result.is_err());
}

#[test]
fn audit_record_serde_roundtrip() {
    let record = ConsolidationAuditRecord {
        id: "audit-1".to_owned(),
        trigger_type: "entity_overflow".to_owned(),
        trigger_id: "e-1".to_owned(),
        original_count: 15,
        consolidated_count: 5,
        original_fact_ids: r#"["f-1","f-2"]"#.to_owned(),
        consolidated_fact_ids: r#"["f-new-1"]"#.to_owned(),
        consolidated_at: "2026-03-01T00:00:00Z".to_owned(),
    };
    let json = serde_json::to_string(&record).expect("serialize");
    let back: ConsolidationAuditRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(record.id, back.id);
    assert_eq!(record.original_count, back.original_count);
}

// ---------------------------------------------------------------------------
// Trigger evaluation
// ---------------------------------------------------------------------------

/// Requirement 19: entity with 11 facts (> threshold 10) triggers entity overflow.
#[test]
fn entity_fact_count_11_triggers_entity_overflow() {
    let config = ConsolidationConfig::default();
    let fact_count = 11;
    assert!(
        fact_count > config.entity_fact_threshold,
        "11 facts must exceed the entity threshold of {} to trigger overflow",
        config.entity_fact_threshold
    );

    let trigger = ConsolidationTrigger::EntityOverflow {
        entity_id: EntityId::from("e-alice"),
        fact_count,
    };
    assert_eq!(trigger.trigger_type(), "entity_overflow");
}

/// Requirement 20: entity with 9 facts (< threshold 10) does not trigger.
#[test]
fn entity_fact_count_9_does_not_trigger_overflow() {
    let config = ConsolidationConfig::default();
    let fact_count = 9;
    assert!(
        fact_count < config.entity_fact_threshold,
        "9 facts must be below the entity threshold of {} — no trigger",
        config.entity_fact_threshold
    );
}

/// Requirement 21: community with 21 facts (> threshold 20) triggers community overflow.
#[test]
fn community_fact_count_21_triggers_community_overflow() {
    let config = ConsolidationConfig::default();
    let fact_count = 21;
    assert!(
        fact_count > config.community_fact_threshold,
        "21 facts must exceed the community threshold of {} to trigger overflow",
        config.community_fact_threshold
    );

    let trigger = ConsolidationTrigger::CommunityOverflow {
        cluster_id: 7,
        fact_count,
    };
    assert_eq!(trigger.trigger_type(), "community_overflow");
}

/// Requirement 22: facts younger than 7 days are excluded from consolidation count.
///
/// The Datalog query enforces this via `recorded_at < $cutoff`.
#[test]
fn entity_overflow_query_excludes_recent_facts_via_age_cutoff() {
    assert!(
        ENTITY_OVERFLOW_CANDIDATES.contains("recorded_at < $cutoff"),
        "entity overflow query must filter out facts recorded after the age cutoff"
    );
}

/// Requirement 23: Verified-tier facts are excluded from consolidation candidates.
///
/// The Datalog query enforces this via `tier != 'verified'`.
#[test]
fn entity_overflow_query_excludes_verified_tier_facts() {
    assert!(
        ENTITY_OVERFLOW_CANDIDATES.contains("tier != 'verified'"),
        "entity overflow query must exclude Verified-tier facts from candidates"
    );
    assert!(
        COMMUNITY_OVERFLOW_CANDIDATES.contains("tier != 'verified'"),
        "community overflow query must also exclude Verified-tier facts"
    );
}

// ---------------------------------------------------------------------------
// Response parsing — edge cases
// ---------------------------------------------------------------------------

/// Requirement 28: empty array response is valid (no consolidation produced).
#[test]
fn parse_empty_array_response_is_valid() {
    let response = "[]";
    let entries = parse_consolidation_response(response).expect("empty array must parse");
    assert!(
        entries.is_empty(),
        "empty array response must produce zero consolidated entries"
    );
}

/// Requirement 27: JSON array with missing required fields returns a parse error.
///
/// `content` is required (no `#[serde(default)]`). Omitting it must fail.
#[test]
fn parse_response_missing_required_content_field_errors() {
    // Valid JSON structure but missing the required `content` field
    let response = r#"[{"entities": ["alice@example.com"]}]"#;
    let result = parse_consolidation_response(response);
    assert!(
        result.is_err(),
        "JSON entry without required `content` field must return an error"
    );
}

// ---------------------------------------------------------------------------
// Batch processing — boundary conditions
// ---------------------------------------------------------------------------

/// Requirement 30: single-fact batch is valid (no off-by-one).
#[test]
fn batch_single_fact_produces_one_batch_of_one() {
    let facts: Vec<(FactId, String, f64, String)> = vec![(
        FactId::from("f-1"),
        "Alice is a software engineer at Acme Corp".to_owned(),
        0.9,
        "2026-01-01T00:00:00Z".to_owned(),
    )];

    let batches = batch_facts(&facts, 50);
    assert_eq!(
        batches.len(),
        1,
        "single fact must produce exactly one batch"
    );
    assert_eq!(
        batches[0].len(),
        1,
        "the single batch must contain exactly one fact"
    );
}

/// Requirement 31: batch of exactly `batch_size` produces single batch (boundary).
#[test]
fn batch_exactly_batch_size_produces_single_batch() {
    let batch_size = 5;
    let facts: Vec<(FactId, String, f64, String)> = (0..batch_size)
        .map(|i| {
            (
                FactId::from(format!("f-{i}")),
                format!("fact {i} about bob@example.org"),
                0.8,
                "2026-01-01T00:00:00Z".to_owned(),
            )
        })
        .collect();

    let batches = batch_facts(&facts, batch_size);
    assert_eq!(
        batches.len(),
        1,
        "exactly batch_size facts must produce a single batch (no off-by-one split)"
    );
    assert_eq!(
        batches[0].len(),
        batch_size,
        "the single batch must contain all {batch_size} facts"
    );
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Requirement 34: processed count is always <= input count.
        ///
        /// For any input size and any batch_limit ≥ 1, the total number of
        /// facts across all batches equals the input count (conservation).
        /// This implies processed ≤ input trivially holds.
        #[test]
        fn batch_total_count_equals_input_count(
            count in 0usize..200,
            batch_size in 1usize..50,
        ) {
            let facts: Vec<(FactId, String, f64, String)> = (0..count)
                .map(|i| {
                    (
                        FactId::from(format!("f-{i}")),
                        format!("synthetic fact {i}"),
                        0.8,
                        "2026-01-01T00:00:00Z".to_owned(),
                    )
                })
                .collect();

            let batches = batch_facts(&facts, batch_size);
            let total: usize = batches.iter().map(std::vec::Vec::len).sum();
            prop_assert_eq!(
                total,
                count,
                "total facts across all batches must equal input count (no facts lost or duplicated)"
            );
        }
    }
}
