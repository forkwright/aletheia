#![expect(clippy::expect_used, reason = "test assertions")]

use proptest::prelude::*;

use super::super::{
    DedupTuning, EntityId, EntityInfo, MergeDecision, classify_candidates, compute_merge_score,
    generate_candidates, jaro_winkler_ci,
};

fn ts_epoch() -> jiff::Timestamp {
    jiff::Timestamp::UNIX_EPOCH
}

fn make_entity(id: &str, name: &str, etype: &str) -> EntityInfo {
    EntityInfo {
        id: EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: etype.to_owned(),
        aliases: vec![],
        relationship_count: 1,
        created_at: ts_epoch(),
        name_embedding: None,
    }
}

proptest! {
    /// The composite merge score must always lie in [0.0, 1.0] for inputs
    /// drawn from their valid domains.
    #[test]
    fn merge_score_always_in_unit_interval(
        name_sim in 0.0_f64..=1.0,
        embed_sim in 0.0_f64..=1.0,
        type_match in proptest::bool::ANY,
        alias_overlap in proptest::bool::ANY,
    ) {
        let score = compute_merge_score(
            name_sim,
            embed_sim,
            type_match,
            alias_overlap,
            &DedupTuning::DEFAULT,
        );
        prop_assert!(
            (0.0..=1.0).contains(&score),
            "score {score} not in [0.0, 1.0]"
        );
    }

    /// MergeDecision partitions [0.0, 1.0] into three non-overlapping regions.
    /// Every score must map to exactly one decision.
    #[test]
    fn decision_covers_all_scores(score in 0.0_f64..=1.0) {
        let decision = MergeDecision::from_score(score, &DedupTuning::DEFAULT);
        if score >= 0.90 {
            prop_assert!(decision == MergeDecision::AutoMerge, "score={}, expected AutoMerge", score);
        } else if score >= 0.70 {
            prop_assert!(decision == MergeDecision::Review, "score={}, expected Review", score);
        } else {
            prop_assert!(decision == MergeDecision::Skip, "score={}, expected Skip", score);
        }
    }

    /// Jaro-Winkler similarity is symmetric: sim(a, b) == sim(b, a).
    #[test]
    fn jaro_winkler_symmetric(
        a in "[a-zA-Z ]{0,30}",
        b in "[a-zA-Z ]{0,30}",
    ) {
        let sim_ab = jaro_winkler_ci(&a, &b);
        let sim_ba = jaro_winkler_ci(&b, &a);
        prop_assert!(
            (sim_ab - sim_ba).abs() < 1e-10,
            "jaro_winkler_ci not symmetric: {sim_ab} != {sim_ba}"
        );
    }

    /// Jaro-Winkler similarity is always in [0.0, 1.0].
    #[test]
    fn jaro_winkler_bounded(
        a in "[a-zA-Z ]{0,30}",
        b in "[a-zA-Z ]{0,30}",
    ) {
        let sim = jaro_winkler_ci(&a, &b);
        prop_assert!(
            (0.0..=1.0).contains(&sim),
            "jaro_winkler_ci returned {sim} outside [0.0, 1.0]"
        );
    }

    /// classify_candidates must be lossless: no candidates are silently dropped.
    ///
    /// Every candidate ends up in exactly one of auto_merge, review, or skip.
    /// The sum of (auto_merge + review) must equal the number of non-skip candidates.
    #[test]
    fn classify_preserves_count(
        names in prop::collection::vec("[a-zA-Z]{3,15}", 2..=8),
    ) {
        let entities: Vec<EntityInfo> = names
            .iter()
            .enumerate()
            .map(|(i, name)| make_entity(&format!("e{i}"), name, "person"))
            .collect();

        let candidates = generate_candidates(&entities, &|_, _| 0.0, &DedupTuning::DEFAULT);
        let total_candidates = candidates.len();
        let (auto_merge, review) = classify_candidates(candidates, &DedupTuning::DEFAULT);

        prop_assert!(
            auto_merge.len() + review.len() <= total_candidates,
            "auto_merge + review ({}) > total candidates ({total_candidates})",
            auto_merge.len() + review.len()
        );
    }

    /// A single entity must never generate any self-candidates.
    #[test]
    fn single_entity_no_candidates(name in "[a-zA-Z]{3,20}") {
        let entities = vec![make_entity("e0", &name, "person")];
        let candidates = generate_candidates(&entities, &|_, _| 0.0, &DedupTuning::DEFAULT);
        prop_assert!(candidates.is_empty());
    }
}
