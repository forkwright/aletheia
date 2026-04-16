//! Shared test data builders for knowledge domain types.
//!
//! Provides builder functions for `Fact`, `Entity`, and `Relationship` that
//! eliminate the 30+ field manual constructions scattered across the workspace.
//! Import via `eidos::test_fixtures` from downstream crates.
//!
//! Gated behind the `test-support` feature so production builds never compile
//! this module.

use crate::id::{EntityId, FactId};
use crate::knowledge::{
    Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
    FactTemporal, Relationship, far_future, parse_timestamp,
};

/// Parse an ISO 8601 timestamp for test data. Panics on invalid input.
pub fn test_ts(s: &str) -> jiff::Timestamp {
    parse_timestamp(s).expect("valid test timestamp in test helper")
}

/// Build a minimal `Fact` with sensible test defaults.
///
/// Fields can be mutated after construction for test-specific overrides:
/// ```ignore
/// let mut f = make_fact("f1", "syn", "Rust is fast");
/// f.provenance.confidence = 0.5;
/// ```
pub fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        fact_type: String::new(),
        temporal: FactTemporal {
            valid_from: test_ts("2026-01-01"),
            valid_to: far_future(),
            recorded_at: test_ts("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
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
        scope: None,
    }
}

/// Build a minimal `Entity` with sensible test defaults.
pub fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
    Entity {
        id: EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: vec![],
        created_at: test_ts("2026-03-01T00:00:00Z"),
        updated_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

/// Build a minimal `Relationship` with sensible test defaults.
pub fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: EntityId::new(src).expect("valid test id"),
        dst: EntityId::new(dst).expect("valid test id"),
        relation: relation.to_owned(),
        weight,
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}
