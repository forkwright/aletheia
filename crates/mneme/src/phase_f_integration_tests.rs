//! Cross-feature integration tests for Phase F features.
//!
//! Verifies that instinct system output integrates correctly with
//! conflict detection (F.1), dedup (F.2), recall scoring (F.3),
//! graph intelligence (F.4), and succession chains.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::float_cmp,
    reason = "test assertions compare exact float constants"
)]
#![expect(
    clippy::items_after_statements,
    reason = "scoped use imports in test functions"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use crate::knowledge::{EpistemicTier, FactType};

fn ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp")
}

#[test]
fn recall_scoring_applies_fsrs_decay_to_instinct_facts() {
    use crate::recall::RecallEngine;

    let engine = RecallEngine::new();

    let fact_type = FactType::Preference;
    let tier = EpistemicTier::Inferred;

    let fresh = engine.score_decay(0.0, fact_type, tier, 0);
    assert_eq!(fresh, 1.0, "freshly created instinct fact has full recall");

    let after_one_day = engine.score_decay(24.0, fact_type, tier, 0);
    assert!(
        after_one_day < 1.0,
        "instinct fact decays below 1.0 after 24 hours"
    );
    assert!(
        after_one_day > 0.0,
        "decay stays positive within initial stability window"
    );

    let after_month = engine.score_decay(720.0, fact_type, tier, 0);
    assert!(
        after_month < after_one_day,
        "older instinct fact has lower recall score"
    );

    assert!(
        engine.score_decay(48.0, fact_type, tier, 0) < after_one_day,
        "decay is monotonically non-increasing with age"
    );
}

#[test]
fn preference_fact_type_has_8760_base_stability_hours() {
    use crate::instinct::INSTINCT_STABILITY_HOURS;
    use crate::recall::compute_effective_stability;

    assert_eq!(
        FactType::Preference.base_stability_hours(),
        8_760.0,
        "Preference facts have one-year base stability"
    );

    // WHY: INSTINCT_STABILITY_HOURS is the initial stored stability for newly created instinct
    // facts: set intentionally low (7 days) so facts must be confirmed before persisting.
    assert_eq!(
        INSTINCT_STABILITY_HOURS, 168.0,
        "instinct facts start with 168-hour (7-day) initial stability"
    );

    let effective = compute_effective_stability(FactType::Preference, EpistemicTier::Inferred, 0);
    assert!(
        effective > INSTINCT_STABILITY_HOURS,
        "FSRS effective stability ({effective}) exceeds initial instinct stability"
    );
}

#[test]
fn dedup_candidate_generation_finds_duplicate_instinct_patterns() {
    use crate::dedup::{EntityInfo, MergeDecision, compute_merge_score, generate_candidates};
    use crate::id::EntityId;

    let ts_a = ts("2026-03-01T00:00:00Z");
    let ts_b = ts("2026-03-05T00:00:00Z");

    let entities = vec![
        EntityInfo {
            id: EntityId::from("instinct-grep-1"),
            name: "grep code preference".to_owned(),
            entity_type: "tool_pattern".to_owned(),
            aliases: vec![],
            relationship_count: 5,
            created_at: ts_a,
        },
        EntityInfo {
            id: EntityId::from("instinct-grep-2"),
            name: "grep code preference".to_owned(),
            entity_type: "tool_pattern".to_owned(),
            aliases: vec![],
            relationship_count: 3,
            created_at: ts_b,
        },
    ];

    let candidates = generate_candidates(&entities, &|_, _| 0.0);
    assert!(
        !candidates.is_empty(),
        "exact name match for same-type instinct entities should produce a candidate"
    );
    assert_eq!(candidates[0].name_a, "grep code preference");
    assert_eq!(candidates[0].name_b, "grep code preference");
    assert!(
        candidates[0].type_match,
        "same entity_type means type_match=true"
    );

    let score = compute_merge_score(candidates[0].name_similarity, 0.0, true, false);
    assert!(
        score >= 0.0,
        "merge score should be non-negative for exact name match"
    );
    assert!(score <= 1.0, "merge score should be at most 1.0");

    let all_signals_score = compute_merge_score(1.0, 1.0, true, true);
    assert_eq!(
        MergeDecision::from_score(all_signals_score),
        MergeDecision::AutoMerge,
        "perfect name + embed + type + alias → auto-merge"
    );
}

#[test]
fn conflict_detection_identifies_contradicting_behavioral_preferences() {
    use crate::conflict::{
        ConflictAction, ConflictCandidate, ConflictClassification, FactForConflictCheck,
        resolve_action,
    };
    use crate::id::FactId;

    let classification = ConflictClassification::parse("CONTRADICTS")
        .expect("CONTRADICTS should parse to Some(Contradicts)");
    assert_eq!(classification, ConflictClassification::Contradicts);

    let refines = ConflictClassification::parse("REFINES - more specific version")
        .expect("REFINES should parse");
    assert_eq!(refines, ConflictClassification::Refines);

    let existing = ConflictCandidate {
        existing_fact_id: FactId::from("fact-old-preference"),
        existing_content:
            "When working on code tasks, tool 'grep' is preferred (success rate: 82%, n=11)"
                .to_owned(),
        existing_confidence: 0.7,
        existing_tier: EpistemicTier::Inferred,
        cosine_similarity: 0.85,
    };

    let new_fact = FactForConflictCheck {
        content:
            "When working on code tasks, tool 'ripgrep' is preferred (success rate: 91%, n=22)"
                .to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        subject: "code".to_owned(),
        is_correction: false,
        embedding: vec![],
    };

    let action = resolve_action(&ConflictClassification::Contradicts, &existing, &new_fact);
    assert!(
        matches!(action, ConflictAction::Supersede { .. }),
        "higher-confidence behavioral preference should supersede lower-confidence one"
    );

    let weaker_fact = FactForConflictCheck {
        content: "When working on code tasks, tool 'awk' is preferred (success rate: 71%, n=7)"
            .to_owned(),
        confidence: 0.5,
        tier: EpistemicTier::Inferred,
        subject: "code".to_owned(),
        is_correction: false,
        embedding: vec![],
    };
    let drop_action = resolve_action(
        &ConflictClassification::Contradicts,
        &existing,
        &weaker_fact,
    );
    assert_eq!(
        drop_action,
        ConflictAction::Drop,
        "lower-confidence new behavioral preference should be dropped"
    );
}

#[test]
fn graph_context_includes_instinct_generated_entities_in_pagerank() {
    use crate::graph_intelligence::GraphContext;

    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("tool:grep".to_owned(), 0.75);
    ctx.pageranks.insert("tool:web_search".to_owned(), 0.42);
    ctx.pageranks.insert("tool:read_file".to_owned(), 0.10);

    assert_eq!(ctx.importance("tool:grep"), 0.75);
    assert_eq!(ctx.importance("tool:web_search"), 0.42);
    assert_eq!(ctx.importance("tool:read_file"), 0.10);
    assert_eq!(
        ctx.importance("tool:nonexistent"),
        0.0,
        "missing entity returns 0.0"
    );

    assert!(
        !ctx.is_empty(),
        "context with pagerank entries is not empty"
    );

    use crate::graph_intelligence::score_epistemic_tier_with_importance;
    let base = 0.6; // Inferred tier base score
    let boosted_high = score_epistemic_tier_with_importance(base, 0.75);
    let boosted_low = score_epistemic_tier_with_importance(base, 0.10);
    assert!(
        boosted_high > boosted_low,
        "higher PageRank importance produces higher tier score"
    );
    assert!(boosted_high <= 1.0, "boosted score cannot exceed 1.0");
}

#[test]
fn succession_chain_evolution_bonus_applied_to_instinct_facts() {
    use crate::graph_intelligence::score_access_with_evolution;
    use crate::recall::RecallEngine;

    let engine = RecallEngine::new();
    let access_count = 3_u64;
    let base = engine.score_access_frequency(access_count);

    let no_chain = score_access_with_evolution(base, 0);
    assert!(
        (no_chain - base).abs() < f64::EPSILON,
        "chain_length=0 adds no evolution bonus"
    );

    let with_chain_2 = score_access_with_evolution(base, 2);
    assert!(
        with_chain_2 > base,
        "chain_length=2 should increase access score by evolution bonus"
    );
    assert!(
        (with_chain_2 - base - 0.10).abs() < 1e-10,
        "chain_length=2 bonus is exactly 0.10"
    );

    let capped = score_access_with_evolution(0.0, 4);
    assert!(
        (capped - 0.20).abs() < 1e-10,
        "evolution bonus caps at 0.20 at chain_length=4"
    );

    let long_chain = score_access_with_evolution(0.0, 10);
    assert!(
        (long_chain - 0.20).abs() < 1e-10,
        "evolution bonus stays at 0.20 beyond chain_length=4"
    );

    let saturated = score_access_with_evolution(0.95, 10);
    assert!(
        saturated <= 1.0,
        "evolution bonus is capped to keep score ≤ 1.0"
    );
}
