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
    entity_with_facts(id, name, etype, aliases, rels, 0, created)
}

fn entity_with_facts(
    id: &str,
    name: &str,
    etype: &str,
    aliases: Vec<&str>,
    rels: u32,
    facts: u32,
    created: &str,
) -> EntityInfo {
    EntityInfo {
        id: EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: etype.to_owned(),
        aliases: aliases.into_iter().map(String::from).collect(),
        relationship_count: rels,
        fact_count: facts,
        created_at: ts(created),
        name_embedding: None,
    }
}

/// Variant of [`entity`] that also attaches a name embedding.
///
/// Mirrors the production v13 storage shape: an entity row with a
/// non-NULL `name_embedding` column. Used by reachability tests to
/// assert that `MergeDecision::AutoMerge` is actually reached when
/// real cosine similarity is in scope.
#[allow(dead_code)]
fn entity_with_embedding(
    id: &str,
    name: &str,
    etype: &str,
    aliases: Vec<&str>,
    rels: u32,
    created: &str,
    embedding: Vec<f32>,
) -> EntityInfo {
    let mut e = entity(id, name, etype, aliases, rels, created);
    e.name_embedding = Some(embedding);
    e
}

fn no_embed(_a: &EntityId, _b: &EntityId) -> f64 {
    0.0
}

#[test]
fn decision_auto_merge_at_threshold() {
    assert_eq!(
        MergeDecision::from_score(0.90, &DedupTuning::DEFAULT),
        MergeDecision::AutoMerge
    );
}

#[test]
fn decision_auto_merge_above() {
    assert_eq!(
        MergeDecision::from_score(0.95, &DedupTuning::DEFAULT),
        MergeDecision::AutoMerge
    );
}

#[test]
fn decision_review_at_threshold() {
    assert_eq!(
        MergeDecision::from_score(0.70, &DedupTuning::DEFAULT),
        MergeDecision::Review
    );
}

#[test]
fn decision_review_between() {
    assert_eq!(
        MergeDecision::from_score(0.80, &DedupTuning::DEFAULT),
        MergeDecision::Review
    );
}

#[test]
fn decision_skip_below() {
    assert_eq!(
        MergeDecision::from_score(0.69, &DedupTuning::DEFAULT),
        MergeDecision::Skip
    );
}

#[test]
fn decision_skip_zero() {
    assert_eq!(
        MergeDecision::from_score(0.0, &DedupTuning::DEFAULT),
        MergeDecision::Skip
    );
}

#[test]
fn merge_score_perfect() {
    let score = compute_merge_score(1.0, 1.0, true, true, &DedupTuning::DEFAULT);
    assert!((score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn merge_score_formula_weights() {
    let score = compute_merge_score(0.9, 0.8, true, false, &DedupTuning::DEFAULT);
    assert!((score - 0.80).abs() < 1e-10);
}

#[test]
fn merge_score_no_signals() {
    let score = compute_merge_score(0.0, 0.0, false, false, &DedupTuning::DEFAULT);
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
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert_eq!(candidates.len(), 1);
    assert!((candidates[0].name_similarity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn jaro_winkler_candidate() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec![], 1, "2026-01-01"),
        entity("e2", "Alicia Test", "person", vec![], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].name_similarity >= 0.85);
}

#[test]
fn different_types_not_candidates() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec![], 1, "2026-01-01"),
        entity("e2", "Alice Test", "organization", vec![], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert!(candidates.is_empty());
}

#[test]
fn alias_overlap_triggers_candidate() {
    let entities = vec![
        entity("e1", "Alice Test", "person", vec!["AT"], 1, "2026-01-01"),
        entity("e2", "A. Test", "person", vec!["AT"], 1, "2026-01-01"),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
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
    let candidates = generate_candidates(&entities, &embed_fn, &DedupTuning::DEFAULT);
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
    let candidates = generate_candidates(&entities, &high_embed, &DedupTuning::DEFAULT);
    let (auto_merge, review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert!(
        !auto_merge.is_empty(),
        "expected at least one auto-merge candidate"
    );
    let total = auto_merge.len() + review.len();
    assert!(total >= 1);
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
fn canonical_fact_count_tiebreak() {
    // WHY (#5855): when relationship counts tie, the fact-richer entity must
    // survive as canonical. Both entities have equal relationship counts (1),
    // so the fact-count tiebreaker applies: `b` (50 facts) beats `a` (1 fact).
    let a = entity_with_facts("e1", "Alice Test", "person", vec![], 1, 1, "2026-01-02");
    let b = entity_with_facts("e2", "alice test", "person", vec![], 1, 50, "2026-01-01");
    let (canonical, merged) = pick_canonical(&a, &b);
    assert_eq!(
        canonical.id,
        EntityId::new("e2").expect("valid test id"),
        "fact-richer entity should be canonical when relationship counts tie"
    );
    assert_eq!(merged.id, EntityId::new("e1").expect("valid test id"));
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
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert!(
        candidates.is_empty(),
        "single entity should produce no candidates"
    );
}

#[test]
fn empty_graph_no_candidates() {
    let candidates = generate_candidates(&[], &no_embed, &DedupTuning::DEFAULT);
    assert!(candidates.is_empty());
}

/// WHY(#5670): the pre-fix `generate_candidates` compared every same-type pair,
/// producing O(N²) work. The blocking implementation must keep candidate count
/// sub-quadratic for unrelated entities, otherwise month-scale nous growth
/// degrades the 6-hour maintenance pass.
#[test]
fn candidate_generation_grows_sub_quadratically() {
    // Deterministic pseudo-random spread: adjacent sorted names must not share
    // prefixes or trigrams, so no name-based blocking emits pairs. No aliases
    // means the token index also emits nothing.
    let spread = |i: u64| {
        let x = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        format!("Ent{x:016x}")
    };

    let small: Vec<EntityInfo> = (0..100)
        .map(|i| {
            entity(
                &format!("s{i}"),
                &spread(i),
                "concept",
                vec![],
                1,
                "2026-01-01",
            )
        })
        .collect();
    let large: Vec<EntityInfo> = (0..200)
        .map(|i| {
            entity(
                &format!("l{i}"),
                &spread(i + 10_000),
                "concept",
                vec![],
                1,
                "2026-01-02",
            )
        })
        .collect();

    let small_candidates = generate_candidates(&small, &no_embed, &DedupTuning::DEFAULT);
    let large_candidates = generate_candidates(&large, &no_embed, &DedupTuning::DEFAULT);

    // Unrelated random names should produce very few false-positive candidates.
    // The shared "Ent" prefix gives all names a 3-char Winkler boost; a small
    // number of pairs (~7 for N=100) cross the JW threshold. The bound is far
    // below the O(N²) baseline of ~5000 comparisons for N=100.
    assert!(
        small_candidates.len() <= 30,
        "small unrelated cohort should produce <= 30 candidates, got {}",
        small_candidates.len()
    );
    assert!(
        large_candidates.len() <= 30,
        "large unrelated cohort should produce <= 30 candidates, got {}",
        large_candidates.len()
    );

    // Sub-quadratic guard: doubling N must less than quadruple candidates.
    if !small_candidates.is_empty() {
        assert!(
            large_candidates.len() < small_candidates.len() * 4,
            "candidate count grew super-linearly: large={}, small={}",
            large_candidates.len(),
            small_candidates.len()
        );
    }
}

#[path = "dedup_proptests.rs"]
mod proptests;
#[path = "dedup_tuning_tests.rs"]
mod tuning_tests;
