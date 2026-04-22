//! Tests for `impl Stamped for ScoredResult`.
use eidos::meta::Stamped as _;

use super::super::{FactorScores, ScoredResult};
use crate::knowledge::FactSensitivity;

fn sample_scored_result() -> ScoredResult {
    ScoredResult {
        content: "test memory content".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "fact-001".to_owned(),
        nous_id: "syn".to_owned(),
        factors: FactorScores {
            vector_similarity: 0.9,
            decay: 0.8,
            relevance: 0.7,
            epistemic_tier: 1.0,
            relationship_proximity: 0.5,
            access_frequency: 0.6,
            graph_importance: 0.4,
        },
        score: 0.75,
        sensitivity: FactSensitivity::Public,
    }
}

#[test]
fn scored_result_stamp_producer_prefix() {
    let result = sample_scored_result();
    let meta = result.stamp();
    assert!(
        meta.producer.starts_with("episteme@"),
        "producer must start with 'episteme@', got: {}",
        meta.producer
    );
}

#[test]
fn scored_result_stamp_schema_version() {
    let result = sample_scored_result();
    let meta = result.stamp();
    assert_eq!(meta.schema_version, 1, "schema_version must be 1");
}

#[test]
fn scored_result_stamp_result_count() {
    let result = sample_scored_result();
    let meta = result.stamp();
    assert_eq!(
        meta.row_counts.get("results").copied(),
        Some(1),
        "row_counts['results'] must be 1 for a single result"
    );
}

#[test]
fn scored_result_stamp_generated_at_nonempty() {
    let result = sample_scored_result();
    let meta = result.stamp();
    assert!(
        !meta.generated_at.is_empty(),
        "generated_at must not be empty"
    );
}
