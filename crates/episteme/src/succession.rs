//! Ecological succession: tracking how knowledge evolves over time.
//!
//! Detects supersession patterns, identifies volatile vs stable domains,
//! and adapts FSRS decay rates based on domain volatility. Facts in frequently-changing
//! domains decay faster; facts in stable domains persist longer.
//!
//! ## Volatility scoring
//!
//! Per-entity volatility = `(superseded / total) × (1.0 + 0.1 × avg_chain)`
//! clamped to [0.0, 1.0].
//!
//! ## Adaptive stability
//!
//! The FSRS stability multiplier is scaled by `1.5 - volatility`:
//! - Stable domain (volatility ≈ 0) → 1.5× base stability
//! - Neutral (volatility = 0.5) → 1.0× (unchanged)
//! - Volatile domain (volatility ≈ 1) → 0.5× base stability
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "module internals; only exercised by crate-level tests"
    )
)]

use serde::{Deserialize, Serialize};

use crate::id::EntityId;
use crate::knowledge::{EpistemicTier, FactType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DomainVolatility {
    /// The entity whose domain is measured.
    pub entity_id: EntityId,
    /// Total facts associated with this entity.
    pub total_facts: u32,
    /// Facts that have been superseded by newer facts.
    pub superseded_facts: u32,
    /// Average supersession chain length across this entity's facts.
    pub avg_chain_length: f64,
    /// Computed volatility score in [0.0, 1.0].
    /// 0.0 = perfectly stable, 1.0 = maximally volatile.
    pub volatility_score: f64,
    /// When this score was computed.
    pub computed_at: jiff::Timestamp,
}

/// Per-nous knowledge profile: diagnostic view of what a nous "knows about."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct KnowledgeProfile {
    /// The nous whose profile this is.
    pub nous_id: String,
    /// Top entities by fact count, with their volatility scores.
    pub top_entities: Vec<EntityProfile>,
    /// Average stability across all active facts (hours).
    pub avg_stability_hours: f64,
    /// Total active (non-superseded, non-forgotten) facts.
    pub total_active_facts: u32,
}

/// A single entity entry within a [`KnowledgeProfile`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EntityProfile {
    /// Entity identifier.
    pub entity_id: EntityId,
    /// Entity display name.
    pub entity_name: String,
    /// Number of active facts associated with this entity for this nous.
    pub fact_count: u32,
    /// Average stability of those facts (hours).
    pub avg_stability_hours: f64,
    /// Domain volatility score (0.0–1.0), if available.
    pub volatility_score: Option<f64>,
}

/// Compute the volatility score from supersession metrics.
///
/// Formula: `(superseded / total) × (1.0 + 0.1 × avg_chain_length)`
/// clamped to [0.0, 1.0].
///
/// Returns 0.0 for entities with zero total facts.
#[must_use]
pub(crate) fn compute_volatility(
    total_facts: u32,
    superseded_facts: u32,
    avg_chain_length: f64,
) -> f64 {
    if total_facts == 0 {
        return 0.0;
    }
    let ratio = f64::from(superseded_facts) / f64::from(total_facts);
    let chain_factor = 1.0 + 0.1 * avg_chain_length;
    (ratio * chain_factor).clamp(0.0, 1.0)
}

/// Compute the volatility-adjusted stability multiplier.
///
/// Maps volatility ∈ [0.0, 1.0] to a multiplier ∈ [0.5, 1.5]:
/// - `volatility = 0.0` → 1.5× (stable domains persist longer)
/// - `volatility = 0.5` → 1.0× (neutral, no change)
/// - `volatility = 1.0` → 0.5× (volatile domains decay faster)
#[must_use]
pub(crate) fn volatility_multiplier(volatility: f64) -> f64 {
    1.5 - volatility.clamp(0.0, 1.0)
}

/// Compute adaptive FSRS stability incorporating domain volatility.
///
/// Extends [`crate::recall::compute_effective_stability`] with a volatility factor.
/// The base stability is scaled by `volatility_multiplier(volatility)`.
#[must_use]
pub(crate) fn adaptive_stability(
    fact_type: FactType,
    tier: EpistemicTier,
    access_count: u32,
    volatility: f64,
) -> f64 {
    let base = crate::recall::compute_effective_stability(fact_type, tier, access_count);
    base * volatility_multiplier(volatility)
}

/// Datalog script for computing per-entity volatility metrics.
///
/// Joins `facts` with `fact_entities` and supersession chain data to produce:
/// `[entity_id, total_facts, superseded_facts, avg_chain_length]`
///
/// Run after `SUPERSESSION_CHAIN_LENGTHS`: uses the same `chain[]` recursion inline.
pub(crate) const ENTITY_VOLATILITY_METRICS: &str = r"
chain[id, d] := *facts{id, superseded_by}, is_null(superseded_by), d = 0
chain[id, n] := *facts{id, superseded_by}, superseded_by = next_id, not is_null(next_id),
    chain[next_id, prev_n], n = prev_n + 1

chain_len[id, max(depth)] := chain[id, depth]

entity_facts[eid, fid, cl] :=
    *fact_entities{fact_id: fid, entity_id: eid},
    *facts{id: fid, is_forgotten},
    is_forgotten == false,
    chain_len[fid, cl]

entity_facts[eid, fid, cl] :=
    *fact_entities{fact_id: fid, entity_id: eid},
    *facts{id: fid, is_forgotten},
    is_forgotten == false,
    not chain_len[fid, _],
    cl = 0

superseded[eid, fid] :=
    *fact_entities{fact_id: fid, entity_id: eid},
    *facts{id: fid, superseded_by, is_forgotten},
    is_forgotten == false,
    not is_null(superseded_by)

total[eid, count(fid)] := entity_facts[eid, fid, _]

sup_count[eid, count(fid)] := superseded[eid, fid]

avg_cl[eid, mean(cl)] := entity_facts[eid, _, cl]

?[entity_id, total_facts, superseded_facts, avg_chain_length] :=
    total[entity_id, total_facts],
    sup_count[entity_id, superseded_facts],
    avg_cl[entity_id, avg_chain_length]

?[entity_id, total_facts, superseded_facts, avg_chain_length] :=
    total[entity_id, total_facts],
    not sup_count[entity_id, _],
    superseded_facts = 0,
    avg_cl[entity_id, avg_chain_length]
";

/// Datalog script to store volatility scores in `graph_scores`.
///
/// Parameters: `$entity_id`, `$volatility`, `$now` (ISO 8601 string).
pub(crate) const STORE_VOLATILITY_SCORE: &str = r"
?[entity_id, score_type, score, cluster_id, updated_at] :=
    entity_id = $entity_id,
    score_type = 'volatility',
    score = $volatility,
    cluster_id = -1,
    updated_at = $now

:put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
";

/// Datalog script for per-nous knowledge profile.
///
/// Returns `[entity_id, entity_name, fact_count, avg_stability]` for the top
/// entities associated with a given nous.
///
/// Parameters: `$nous_id`.
pub(crate) const NOUS_KNOWLEDGE_PROFILE: &str = r"
active_facts[fid, eid] :=
    *fact_entities{fact_id: fid, entity_id: eid},
    *facts{id: fid, nous_id, is_forgotten},
    nous_id == $nous_id,
    is_forgotten == false,
    *facts{id: fid, superseded_by},
    is_null(superseded_by)

entity_stats[eid, count(fid), mean(stab)] :=
    active_facts[fid, eid],
    *facts{id: fid, stability_hours: stab}

?[entity_id, entity_name, fact_count, avg_stability] :=
    entity_stats[entity_id, fact_count, avg_stability],
    *entities{id: entity_id, name: entity_name}

:order -fact_count
:limit 20
";

/// Datalog script for counting total active facts per nous.
///
/// Parameters: `$nous_id`.
pub(crate) const NOUS_ACTIVE_FACT_STATS: &str = r"
active[fid, stab] :=
    *facts{id: fid, nous_id, is_forgotten, superseded_by, stability_hours: stab},
    nous_id == $nous_id,
    is_forgotten == false,
    is_null(superseded_by)

?[count(fid), mean(stab)] := active[fid, stab]
";

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn volatility_zero_facts_returns_zero() {
        assert!((compute_volatility(0, 0, 0.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn volatility_no_supersessions() {
        let v = compute_volatility(10, 0, 0.0);
        assert!(v.abs() < f64::EPSILON, "expected 0.0, got {v}");
    }

    #[test]
    fn volatility_single_fact_no_supersession() {
        let v = compute_volatility(1, 0, 0.0);
        assert!(v.abs() < f64::EPSILON, "expected 0.0, got {v}");
    }

    #[test]
    fn volatility_high_supersession_rate() {
        let v = compute_volatility(10, 8, 2.0);
        assert!((v - 0.96).abs() < f64::EPSILON, "expected 0.96, got {v}");
    }

    #[test]
    fn volatility_low_supersession_rate() {
        let v = compute_volatility(10, 1, 0.5);
        assert!((v - 0.105).abs() < 1e-10, "expected 0.105, got {v}");
    }

    #[test]
    fn volatility_clamped_to_one() {
        let v = compute_volatility(10, 10, 20.0);
        assert!((v - 1.0).abs() < f64::EPSILON, "expected 1.0, got {v}");
    }

    #[test]
    fn volatility_multiplier_stable() {
        let m = volatility_multiplier(0.0);
        assert!((m - 1.5).abs() < f64::EPSILON, "stable → 1.5, got {m}");
    }

    #[test]
    fn volatility_multiplier_neutral() {
        let m = volatility_multiplier(0.5);
        assert!((m - 1.0).abs() < f64::EPSILON, "neutral → 1.0, got {m}");
    }

    #[test]
    fn volatility_multiplier_volatile() {
        let m = volatility_multiplier(1.0);
        assert!((m - 0.5).abs() < f64::EPSILON, "volatile → 0.5, got {m}");
    }

    #[test]
    fn adaptive_stability_volatile_lower() {
        let ft = FactType::Event;
        let tier = EpistemicTier::Inferred;
        let volatile = adaptive_stability(ft, tier, 0, 0.9);
        let stable = adaptive_stability(ft, tier, 0, 0.1);
        assert!(
            stable > volatile,
            "stable domain ({stable}) should have higher stability than volatile ({volatile})"
        );
    }

    #[test]
    fn adaptive_stability_neutral_equals_base() {
        let ft = FactType::Event;
        let tier = EpistemicTier::Inferred;
        let neutral = adaptive_stability(ft, tier, 0, 0.5);
        let base = crate::recall::compute_effective_stability(ft, tier, 0);
        assert!(
            (neutral - base).abs() < f64::EPSILON,
            "neutral volatility should equal base: {neutral} vs {base}"
        );
    }

    #[test]
    fn adaptive_stability_stable_domain_multiplier() {
        let ft = FactType::Observation;
        let tier = EpistemicTier::Verified;
        let stable = adaptive_stability(ft, tier, 5, 0.0);
        let base = crate::recall::compute_effective_stability(ft, tier, 5);
        let expected = base * 1.5;
        assert!(
            (stable - expected).abs() < 1e-6,
            "stable domain: expected {expected}, got {stable}"
        );
    }

    #[test]
    fn adaptive_stability_volatile_domain_multiplier() {
        let ft = FactType::Observation;
        let tier = EpistemicTier::Verified;
        let volatile = adaptive_stability(ft, tier, 5, 1.0);
        let base = crate::recall::compute_effective_stability(ft, tier, 5);
        let expected = base * 0.5;
        assert!(
            (volatile - expected).abs() < 1e-6,
            "volatile domain: expected {expected}, got {volatile}"
        );
    }

    #[test]
    fn domain_volatility_serde_roundtrip() {
        let dv = DomainVolatility {
            entity_id: EntityId::from("ent-1"),
            total_facts: 10,
            superseded_facts: 3,
            avg_chain_length: 1.5,
            volatility_score: 0.345,
            computed_at: jiff::Timestamp::now(),
        };
        let json = serde_json::to_string(&dv).expect("serialize");
        let back: DomainVolatility = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(dv.entity_id, back.entity_id);
        assert_eq!(dv.total_facts, back.total_facts);
        assert!((dv.volatility_score - back.volatility_score).abs() < f64::EPSILON);
    }

    #[test]
    fn knowledge_profile_serde_roundtrip() {
        let profile = KnowledgeProfile {
            nous_id: "syn".to_owned(),
            top_entities: vec![EntityProfile {
                entity_id: EntityId::from("ent-1"),
                entity_name: "Alice".to_owned(),
                fact_count: 5,
                avg_stability_hours: 720.0,
                volatility_score: Some(0.2),
            }],
            avg_stability_hours: 1000.0,
            total_active_facts: 25,
        };
        let json = serde_json::to_string(&profile).expect("serialize");
        let back: KnowledgeProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile.nous_id, back.nous_id);
        assert_eq!(profile.top_entities.len(), back.top_entities.len());
    }

    #[cfg(feature = "mneme-engine")]
    mod engine_tests {
        use super::*;
        use crate::knowledge::{
            Entity, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal, far_future,
        };
        use crate::knowledge_store::KnowledgeStore;

        fn mem_store() -> std::sync::Arc<KnowledgeStore> {
            KnowledgeStore::open_mem().expect("open_mem")
        }

        fn make_entity(id: &str, name: &str) -> Entity {
            Entity {
                id: EntityId::new_unchecked(id),
                name: name.to_owned(),
                entity_type: "concept".to_owned(),
                aliases: vec![],
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            }
        }

        fn make_fact(id: &str, nous_id: &str) -> Fact {
            Fact {
                id: crate::id::FactId::from(id),
                nous_id: nous_id.to_owned(),
                content: format!("fact content for {id}"),
                fact_type: "observation".to_owned(),
                temporal: FactTemporal {
                    valid_from: jiff::Timestamp::now(),
                    valid_to: far_future(),
                    recorded_at: jiff::Timestamp::now(),
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
            }
        }

        fn link_fact_entity(store: &KnowledgeStore, fact_id: &str, entity_id: &str) {
            store
                .insert_fact_entity(
                    &crate::id::FactId::from(fact_id),
                    &EntityId::new_unchecked(entity_id),
                )
                .expect("insert_fact_entity");
        }

        #[test]
        fn chain_length_a_b_c() {
            let store = mem_store();

            let mut fact_a = make_fact("fact-a", "syn");
            fact_a.lifecycle.superseded_by = Some(crate::id::FactId::from("fact-b"));
            fact_a.temporal.valid_to = jiff::Timestamp::now();

            let mut fact_b = make_fact("fact-b", "syn");
            fact_b.lifecycle.superseded_by = Some(crate::id::FactId::from("fact-c"));
            fact_b.temporal.valid_to = jiff::Timestamp::now();

            let fact_c = make_fact("fact-c", "syn");

            store.insert_fact(&fact_a).expect("insert fact_a");
            store.insert_fact(&fact_b).expect("insert fact_b");
            store.insert_fact(&fact_c).expect("insert fact_c");

            let chains = store.compute_chain_lengths().expect("chain_lengths");
            assert_eq!(
                chains.get("fact-a").copied(),
                Some(2),
                "A→B→C: A should have chain_length 2"
            );
            assert_eq!(
                chains.get("fact-b").copied(),
                Some(1),
                "B→C: B should have chain_length 1"
            );
            assert_eq!(
                chains.get("fact-c").copied(),
                Some(0),
                "C is leaf: chain_length 0"
            );
        }

        #[test]
        fn volatility_high_entity() {
            let store = mem_store();

            let entity = make_entity("ent-volatile", "Volatile Topic");
            store.insert_entity(&entity).expect("insert entity");

            for i in 0..10 {
                let id = format!("f-v-{i}");
                let mut fact = make_fact(&id, "syn");
                if i < 8 {
                    let replacement_id = format!("f-v-rep-{i}");
                    fact.lifecycle.superseded_by =
                        Some(crate::id::FactId::from(replacement_id.as_str()));
                    fact.temporal.valid_to = jiff::Timestamp::now();

                    let rep = make_fact(&replacement_id, "syn");
                    store.insert_fact(&rep).expect("insert replacement");
                    link_fact_entity(&store, &replacement_id, "ent-volatile");
                }
                store.insert_fact(&fact).expect("insert fact");
                link_fact_entity(&store, &id, "ent-volatile");
            }

            let volatilities = store
                .compute_domain_volatility()
                .expect("compute_domain_volatility");

            let vol = volatilities
                .iter()
                .find(|v| v.entity_id.as_str() == "ent-volatile")
                .expect("should find volatile entity");

            assert!(
                vol.volatility_score > 0.3,
                "highly superseded entity should have high volatility, got {}",
                vol.volatility_score
            );
        }

        #[test]
        fn volatility_low_entity() {
            let store = mem_store();

            let entity = make_entity("ent-stable", "Stable Topic");
            store.insert_entity(&entity).expect("insert entity");

            for i in 0..10 {
                let id = format!("f-s-{i}");
                let mut fact = make_fact(&id, "syn");
                if i == 0 {
                    let rep_id = "f-s-rep-0";
                    fact.lifecycle.superseded_by = Some(crate::id::FactId::from(rep_id));
                    fact.temporal.valid_to = jiff::Timestamp::now();
                    let rep = make_fact(rep_id, "syn");
                    store.insert_fact(&rep).expect("insert rep");
                    link_fact_entity(&store, rep_id, "ent-stable");
                }
                store.insert_fact(&fact).expect("insert fact");
                link_fact_entity(&store, &id, "ent-stable");
            }

            let volatilities = store
                .compute_domain_volatility()
                .expect("compute_domain_volatility");

            let vol = volatilities
                .iter()
                .find(|v| v.entity_id.as_str() == "ent-stable")
                .expect("should find stable entity");

            assert!(
                vol.volatility_score < 0.2,
                "mostly-stable entity should have low volatility, got {}",
                vol.volatility_score
            );
        }

        #[test]
        fn store_and_load_volatility_scores() {
            let store = mem_store();

            let entity = make_entity("ent-1", "Test Entity");
            store.insert_entity(&entity).expect("insert entity");

            let fact = make_fact("f-1", "syn");
            store.insert_fact(&fact).expect("insert fact");
            link_fact_entity(&store, "f-1", "ent-1");

            store
                .compute_and_store_volatility()
                .expect("compute_and_store_volatility");

            let ctx = store.load_graph_context().expect("load_graph_context");

            let volatilities = store
                .load_volatility_scores()
                .expect("load_volatility_scores");

            assert!(
                volatilities.contains_key("ent-1"),
                "volatility score should be stored for ent-1"
            );
            let v = volatilities["ent-1"];
            assert!(
                v.abs() < f64::EPSILON,
                "single non-superseded fact should have 0.0 volatility, got {v}"
            );

            drop(ctx);
        }

        #[test]
        fn nous_knowledge_profile_query() {
            let store = mem_store();

            store
                .insert_entity(&make_entity("ent-rust", "Rust"))
                .expect("insert");
            store
                .insert_entity(&make_entity("ent-py", "Python"))
                .expect("insert");

            for i in 0..5 {
                let id = format!("f-rust-{i}");
                let fact = make_fact(&id, "syn");
                store.insert_fact(&fact).expect("insert");
                link_fact_entity(&store, &id, "ent-rust");
            }
            for i in 0..2 {
                let id = format!("f-py-{i}");
                let fact = make_fact(&id, "syn");
                store.insert_fact(&fact).expect("insert");
                link_fact_entity(&store, &id, "ent-py");
            }

            store
                .compute_and_store_volatility()
                .expect("compute_and_store_volatility");

            let profile = store
                .nous_knowledge_profile("syn")
                .expect("nous_knowledge_profile");

            assert_eq!(profile.nous_id, "syn");
            assert_eq!(profile.total_active_facts, 7);
            assert!(!profile.top_entities.is_empty());

            let rust_entry = &profile.top_entities[0];
            assert_eq!(rust_entry.entity_id.as_str(), "ent-rust");
            assert_eq!(rust_entry.fact_count, 5);
        }

        #[test]
        fn entity_with_zero_facts_no_volatility() {
            let store = mem_store();

            let entity = make_entity("ent-empty", "Empty Entity");
            store.insert_entity(&entity).expect("insert entity");

            let volatilities = store
                .compute_domain_volatility()
                .expect("compute_domain_volatility");

            let found = volatilities
                .iter()
                .any(|v| v.entity_id.as_str() == "ent-empty");
            assert!(
                !found,
                "entity with zero facts should not appear in volatility results"
            );
        }
    }
}
