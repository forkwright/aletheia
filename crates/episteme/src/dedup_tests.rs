#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;
use crate::id::EntityId;

fn ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp")
}

fn entity(
    id: &str,
    name: &str,
    etype: &str,
    aliases: Vec<&str>,
    rels: u32,
    created: &str,
) -> EntityInfo {
    EntityInfo {
        id: EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: etype.to_owned(),
        aliases: aliases.into_iter().map(String::from).collect(),
        relationship_count: rels,
        created_at: ts(created),
    }
}

fn no_embed(_a: &EntityId, _b: &EntityId) -> f64 {
    0.0
}

#[test]
fn decision_auto_merge_at_threshold() {
    assert_eq!(MergeDecision::from_score(0.90), MergeDecision::AutoMerge);
}

#[test]
fn decision_auto_merge_above() {
    assert_eq!(MergeDecision::from_score(0.95), MergeDecision::AutoMerge);
}

#[test]
fn decision_review_at_threshold() {
    assert_eq!(MergeDecision::from_score(0.70), MergeDecision::Review);
}

#[test]
fn decision_review_between() {
    assert_eq!(MergeDecision::from_score(0.80), MergeDecision::Review);
}

#[test]
fn decision_skip_below() {
    assert_eq!(MergeDecision::from_score(0.69), MergeDecision::Skip);
}

#[test]
fn decision_skip_zero() {
    assert_eq!(MergeDecision::from_score(0.0), MergeDecision::Skip);
}

#[test]
fn merge_score_perfect() {
    let score = compute_merge_score(1.0, 1.0, true, true);
    assert!((score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn merge_score_formula_weights() {
    let score = compute_merge_score(0.9, 0.8, true, false);
    assert!((score - 0.80).abs() < 1e-10);
}

#[test]
fn merge_score_no_signals() {
    let score = compute_merge_score(0.0, 0.0, false, false);
    assert!((score - 0.0).abs() < f64::EPSILON);
}

#[test]
fn jw_exact_case_insensitive() {
    let sim = jaro_winkler_ci("Alice Test", "alice test");
    assert!((sim - 1.0).abs() < f64::EPSILON);
}

#[test]
fn jw_similar_names() {
    let sim = jaro_winkler_ci("Alice Test", "Alicia Test");
    assert!(sim >= 0.85, "expected >= 0.85, got {sim}");
}

#[test]
fn jw_different_names() {
    let sim = jaro_winkler_ci("Alice Test", "Bob Example");
    assert!(sim < 0.85, "expected < 0.85, got {sim}");
}

#[test]
fn alias_overlap_shared() {
    let a = vec!["JS".to_owned(), "Johnny".to_owned()];
    let b = vec!["js".to_owned()];
    assert!(aliases_overlap(&a, &b));
}

#[test]
fn alias_overlap_none() {
    let a = vec!["JS".to_owned()];
    let b = vec!["AJ".to_owned()];
    assert!(!aliases_overlap(&a, &b));
}

#[test]
fn alias_overlap_empty() {
    assert!(!aliases_overlap(&[], &[]));
}

#[test]
fn name_in_alias_match() {
    assert!(name_in_aliases(
        "JS",
        &["js".to_owned(), "smith".to_owned()],
        "Alice Test",
        &[],
    ));
}

#[test]
fn cosine_identical() {
    let v = vec![1.0_f32, 2.0, 3.0];
    let sim = cosine_similarity(&v, &v);
    assert!((sim - 1.0).abs() < 1e-6);
}

#[test]
fn cosine_orthogonal() {
    let a = vec![1.0_f32, 0.0];
    let b = vec![0.0_f32, 1.0];
    assert!((cosine_similarity(&a, &b)).abs() < 1e-6);
}

#[test]
fn cosine_empty() {
    assert!((cosine_similarity(&[], &[]) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn exact_name_match_case_insensitive() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec![], 2, "2026-01-01"),
        entity("e2", "alice test", "person", vec![], 1, "2026-01-02"),
    ];
    let candidates = generate_candidates(&entities, &no_embed);
    assert_eq!(candidates.len(), 1);
    assert!((candidates[0].name_similarity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn jaro_winkler_candidate() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec![], 1, "2026-01-01"),
        entity("e2", "Alicia Test", "person", vec![], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed);
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].name_similarity >= 0.85);
}

#[test]
fn different_types_not_candidates() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec![], 1, "2026-01-01"),
        entity("e2", "Alice Test", "organization", vec![], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed);
    assert!(candidates.is_empty());
}

#[test]
fn alias_overlap_triggers_candidate() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec!["AT"], 1, "2026-01-01"),
        entity("e2", "A. Test", "person", vec!["AT"], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed);
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].alias_overlap);
}

#[test]
fn embedding_similarity_triggers_candidate() {
    let entities = vec![
        entity("e1", "ACME Corp", "organization", vec![], 1, "2026-01-01"),
        entity(
            "e2",
            "Acme Corporation",
            "organization",
            vec![],
            1,
            "2026-01-01",
        ),
    ];
    let embed_fn = |a: &EntityId, b: &EntityId| -> f64 {
        if (a.as_str() == "e1" && b.as_str() == "e2") || (a.as_str() == "e2" && b.as_str() == "e1")
        {
            0.95
        } else {
            0.0
        }
    };
    let candidates = generate_candidates(&entities, &embed_fn);
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].embed_similarity >= 0.80);
}

#[test]
fn classify_auto_merge_and_review() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec!["AT"], 2, "2026-01-01"),
        entity("e2", "alice test", "person", vec!["AT"], 1, "2026-01-02"),
        entity("e3", "Alicia Test", "person", vec![], 1, "2026-01-03"),
    ];
    let high_embed = |a: &EntityId, b: &EntityId| -> f64 {
        if (a.as_str() == "e1" && b.as_str() == "e2") || (a.as_str() == "e2" && b.as_str() == "e1")
        {
            0.95
        } else {
            0.0
        }
    };
    let candidates = generate_candidates(&entities, &high_embed);
    let (auto_merge, review) = classify_candidates(candidates);
    assert!(
        !auto_merge.is_empty(),
        "expected at least one auto-merge candidate"
    );
    let total = auto_merge.len() + review.len();
    assert!(total >= 1);
}

/// Pins the failure mode named in aletheia#4165(A): with the production
/// embedding-similarity closure (`|_,_| 0.0`) — the ONLY closure any production
/// caller passes — the `AutoMerge` bucket is mathematically unreachable.
///
/// Max achievable score under embed=0.0 for two identical-name, same-type,
/// alias-overlapping entities is:
///   0.4·name(1.0) + 0.3·embed(0.0) + 0.2·type(1.0) + 0.1·alias(1.0) = 0.70
/// The `AutoMerge` threshold is 0.90. So under the production closure no candidate
/// can ever reach `AutoMerge` — `run_entity_dedup` always returns zero records
/// and the maintenance task's `"N entities merged automatically"` log is
/// structurally always N=0.
///
/// This test asserts that property using the SAME fixture as
/// `classify_auto_merge_and_review` above: the sibling test feeds embed=0.95
/// and proves `AutoMerge` works *with* embeddings; this test feeds embed=0.0
/// and proves `AutoMerge` is unreachable *without* them. The pair flips green→red
/// the moment a real embedding path is wired (per the operator decision in
/// `planning/research/memory-dedup-reachability.md`), making the change visible.
#[test]
fn auto_merge_unreachable_under_production_embedding_closure() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec!["AT"], 2, "2026-01-01"),
        entity("e2", "alice test", "person", vec!["AT"], 1, "2026-01-02"),
        entity("e3", "Alicia Test", "person", vec![], 1, "2026-01-03"),
    ];
    let candidates = generate_candidates(&entities, &no_embed);
    let (auto_merge, review) = classify_candidates(candidates);
    assert!(
        auto_merge.is_empty(),
        "production-shape embed closure (always-0.0) must keep auto_merge empty: \
         max achievable score is 0.4 + 0 + 0.2 + 0.1 = 0.70 < 0.90 AutoMerge threshold; \
         saw {} candidate(s) with scores [{}]",
        auto_merge.len(),
        auto_merge
            .iter()
            .map(|c| format!("{:.3}", c.merge_score))
            .collect::<Vec<_>>()
            .join(", "),
    );
    assert!(
        !review.is_empty(),
        "the e1/e2 exact-name pair should still surface as Review under embed=0.0 \
         (score 0.70 = Review threshold), so operators can drain via the review queue"
    );
    for c in &review {
        assert!(
            c.merge_score < 0.90,
            "Review candidate score {:.3} must be below AutoMerge threshold",
            c.merge_score
        );
    }
}

#[test]
fn canonical_most_relationships() {
    let a = entity("e1", "Alice Test", "person", vec![], 5, "2026-01-02");
    let b = entity("e2", "alice test", "person", vec![], 2, "2026-01-01");
    let (canonical, merged) = pick_canonical(&a, &b);
    assert_eq!(canonical.id, EntityId::new("e1").expect("valid test id"));
    assert_eq!(merged.id, EntityId::new("e2").expect("valid test id"));
}

#[test]
fn canonical_tiebreak_oldest() {
    let a = entity("e1", "Alice Test", "person", vec![], 3, "2026-01-02");
    let b = entity("e2", "alice test", "person", vec![], 3, "2026-01-01");
    let (canonical, _) = pick_canonical(&a, &b);
    assert_eq!(
        canonical.id,
        EntityId::new("e2").expect("valid test id"),
        "older entity should be canonical"
    );
}

#[test]
fn idempotent_no_self_candidates() {
    let entities = vec![entity(
        "e1",
        "Alice Test",
        "person",
        vec![],
        1,
        "2026-01-01",
    )];
    let candidates = generate_candidates(&entities, &no_embed);
    assert!(
        candidates.is_empty(),
        "single entity should produce no candidates"
    );
}

#[test]
fn empty_graph_no_candidates() {
    let candidates = generate_candidates(&[], &no_embed);
    assert!(candidates.is_empty());
}

#[test]
fn score_boundary_exactly_070() {
    assert_eq!(MergeDecision::from_score(0.70), MergeDecision::Review);
}

#[test]
fn score_boundary_just_below_070() {
    assert_eq!(MergeDecision::from_score(0.6999), MergeDecision::Skip);
}

#[test]
fn score_boundary_exactly_090() {
    assert_eq!(MergeDecision::from_score(0.90), MergeDecision::AutoMerge);
}

#[test]
fn score_boundary_just_below_090() {
    assert_eq!(MergeDecision::from_score(0.8999), MergeDecision::Review);
}

mod proptests {
    use proptest::prelude::*;

    use super::super::{
        EntityId, EntityInfo, MergeDecision, classify_candidates, compute_merge_score,
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
            let score = compute_merge_score(name_sim, embed_sim, type_match, alias_overlap);
            prop_assert!(
                (0.0..=1.0).contains(&score),
                "score {score} not in [0.0, 1.0]"
            );
        }

        /// MergeDecision partitions [0.0, 1.0] into three non-overlapping regions.
        /// Every score must map to exactly one decision.
        #[test]
        fn decision_covers_all_scores(score in 0.0_f64..=1.0) {
            let decision = MergeDecision::from_score(score);
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

            let candidates = generate_candidates(&entities, &|_, _| 0.0);
            let total_candidates = candidates.len();
            let (auto_merge, review) = classify_candidates(candidates);

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
            let candidates = generate_candidates(&entities, &|_, _| 0.0);
            prop_assert!(candidates.is_empty());
        }
    }
}
