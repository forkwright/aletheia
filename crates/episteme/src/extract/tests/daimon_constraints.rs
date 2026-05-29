//! Regression tests for daimon agent constraint 2: anti-self-description (#4109).
//!
//! Identity-type facts must be skipped at the persist boundary so that
//! self-description statements ("I am X", "My name is Y") extracted from
//! agent output are never persisted as episodic memory facts.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

#[cfg(feature = "mneme-engine")]
#[test]
fn identity_typed_fact_skipped_at_persist() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![],
        relationships: vec![],
        // Explicitly marked as identity — simulates what the LLM might extract
        // when the agent describes itself in third person ("The agent is named Claude").
        facts: vec![ExtractedFact {
            subject: "The agent".to_owned(),
            predicate: "is named".to_owned(),
            object: "Claude".to_owned(),
            confidence: 0.95,
            fact_type: Some("identity".to_owned()),
            is_correction: false,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed");

    assert_eq!(
        result.facts_inserted, 0,
        "identity-typed fact must not be persisted (anti-self-description constraint)"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn non_identity_fact_is_persisted_normally() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![],
        relationships: vec![],
        facts: vec![ExtractedFact {
            subject: "Aletheia".to_owned(),
            predicate: "uses".to_owned(),
            object: "Rust".to_owned(),
            confidence: 0.9,
            fact_type: Some("observation".to_owned()),
            is_correction: false,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed");

    assert_eq!(
        result.facts_inserted, 1,
        "non-identity observation fact must be persisted normally"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn identity_classified_at_runtime_also_skipped() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    // No explicit fact_type — the engine classifies via FactType::classify().
    // "I am an AI assistant" → subject="I" contains identity pattern → Identity type.
    let extraction = Extraction {
        entities: vec![],
        relationships: vec![],
        facts: vec![ExtractedFact {
            subject: "I".to_owned(),
            predicate: "am".to_owned(),
            object: "an AI assistant".to_owned(),
            confidence: 0.95,
            fact_type: None,
            is_correction: false,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed");

    assert_eq!(
        result.facts_inserted, 0,
        "runtime-classified identity fact must not be persisted"
    );
}
