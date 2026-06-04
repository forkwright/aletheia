//! End-to-end recall tests for the speculative surprise + evidence factors (Q1).
//!
//! Verifies that, once their knowledge-config weights are enabled, the surprise
//! and evidence-coverage factors genuinely affect ranking — and stay inert by
//! default.

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use super::super::*;
use super::*;

/// With a positive surprise weight and a session prior primed on one topic, a
/// topically-divergent candidate outranks a same-topic one at equal vector
/// distance.
#[test]
fn surprise_weight_boosts_divergent_candidate() {
    let mut calc = mneme::surprise::SurpriseCalculator::new();
    for _ in 0..6 {
        calc.compute_surprise(
            "chop the onions and garlic then saute them in olive oil until golden",
        );
    }

    let config = RecallConfig {
        surprise_weight: 12.0,
        surprise_threshold: 0.0,
        min_score: 0.0,
        max_results: 5,
        ..Default::default()
    };
    let stage = RecallStage::new(config).with_surprise_calculator(Some(calc));

    // Identical vector distance — only topic-shift surprise differs.
    let similar =
        make_knowledge_result_with_id("saute the garlic and onions in olive oil", 0.5, "fact-cook");
    let divergent = make_knowledge_result_with_id(
        "the schwarzschild radius of a black hole scales linearly with its mass",
        0.5,
        "fact-physics",
    );

    let result = stage
        .run(
            "what should i cook for dinner",
            "syn",
            &mock_embed(),
            &MockVectorSearch::new(vec![similar, divergent]),
            100_000,
        )
        .expect("recall runs");

    let section = result.recall_section.expect("recall section present");
    let physics_idx = section
        .find("schwarzschild")
        .expect("physics candidate present");
    let cooking_idx = section.find("saute").expect("cooking candidate present");
    assert!(
        physics_idx < cooking_idx,
        "the surprising (divergent) candidate should rank first:\n{section}"
    );
}

/// With surprise inert (default weight 0, no calculator), ranking follows vector
/// distance only — the closer candidate wins regardless of topic.
#[test]
fn surprise_inert_by_default() {
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 5,
        ..Default::default()
    };
    let stage = RecallStage::new(config);

    let near = make_knowledge_result_with_id("alpha gardening tomatoes outdoors", 0.1, "fact-near");
    let far = make_knowledge_result_with_id(
        "the schwarzschild radius scales with black hole mass",
        0.9,
        "fact-far",
    );

    let result = stage
        .run(
            "anything",
            "syn",
            &mock_embed(),
            &MockVectorSearch::new(vec![near, far]),
            100_000,
        )
        .expect("recall runs");

    let section = result.recall_section.expect("recall section present");
    let near_idx = section.find("alpha gardening").expect("near present");
    let far_idx = section.find("schwarzschild").expect("far present");
    assert!(
        near_idx < far_idx,
        "closer candidate should rank first when surprise is inert:\n{section}"
    );
}

/// With a positive evidence-coverage weight in the iterative path, facts that
/// lexically answer a decomposed query gap outrank an off-topic filler.
#[test]
fn evidence_weight_boosts_gap_answering_candidates() {
    let config = RecallConfig {
        evidence_coverage_weight: 15.0,
        iterative: true,
        max_cycles: 2,
        min_score: 0.0,
        max_results: 5,
        ..Default::default()
    };
    let stage = RecallStage::new(config);

    let query = "what is the capital of France and the population of France";

    // Cycle 1: a gap-answering fact + off-topic filler (novel terms trigger cycle 2).
    let cycle1 = vec![
        make_knowledge_result_with_id(
            "The capital of France is Paris in western Europe",
            0.5,
            "fact-capital",
        ),
        make_knowledge_result_with_id(
            "Unrelated note about gardening tomatoes and zucchini outdoors",
            0.5,
            "fact-filler",
        ),
    ];
    // Cycle 2: the second gap-answering fact.
    let cycle2 = vec![make_knowledge_result_with_id(
        "The population of France numbers about 68 million residents",
        0.5,
        "fact-pop",
    )];
    let vs = CycledMockSearch::new(vec![cycle1, cycle2]);

    let result = stage
        .run(query, "syn", &mock_embed(), &vs, 100_000)
        .expect("recall runs");

    assert!(vs.call_count() >= 2, "cycle 2 should have run");

    let section = result.recall_section.expect("recall section present");
    let filler_idx = section
        .find("gardening tomatoes")
        .expect("filler candidate present");
    let capital_idx = section
        .find("capital of France")
        .expect("capital candidate present");
    assert!(
        capital_idx < filler_idx,
        "gap-answering candidate should outrank the off-topic filler:\n{section}"
    );
}
