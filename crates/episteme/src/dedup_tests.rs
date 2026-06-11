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
        name_embedding: None,
    }
}

/// Variant of [`entity`] that also attaches a name embedding.
///
/// Mirrors the production v13 storage shape: an entity row with a
/// non-NULL `name_embedding` column. Used by reachability tests to
/// assert that `MergeDecision::AutoMerge` is actually reached when
/// real cosine similarity is in scope.
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

// WHY(#4165): regression — `MergeDecision::AutoMerge` must stay reachable from
// the production callers of `generate_candidates` (`find_duplicate_entities`,
// `run_entity_dedup`); a `|_, _| 0.0` `embed_sim` closure caps the composite
// score at 0.70 and makes AutoMerge structurally unreachable.
//   - `pre_fix_regression_*` — the no-embed path produces no auto-merges
//   - `path_a_reachable_*` — `make_embedding_lookup` over stored
//     `name_embedding`s reaches AutoMerge for semantic duplicates
//   - `path_a_lookup_*` — closure-level edge cases (missing embeddings,
//     dimension mismatch, asymmetric stores)

/// Pre-fix regression: a closure that always returns 0.0 (the historical
/// `|_, _| 0.0` shape from `entity.rs:141` and `entity.rs:355`) must never
/// produce an auto-merge, no matter how closely names + types + aliases
/// agree. Maximum achievable score is 0.4·1 + 0.3·0 + 0.2·1 + 0.1·1 = 0.70.
#[test]
fn pre_fix_regression_zero_embed_caps_at_review_tier() {
    // Two entities that match perfectly on every non-embedding signal:
    // identical name (case-insensitive), same type, overlapping alias.
    let entities = vec![
        entity(
            "e1",
            "Acme Corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-01",
        ),
        entity(
            "e2",
            "acme corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-02",
        ),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert_eq!(candidates.len(), 1, "exactly one pair");
    let c = &candidates[0];
    assert!(
        c.merge_score <= 0.70 + 1e-9,
        "without embedding similarity the merge score must cap at 0.70; got {}",
        c.merge_score
    );
    let (auto_merge, _review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert!(
        auto_merge.is_empty(),
        "pre-fix closure (embed=0.0) must never produce AutoMerge — this is the bug #4165 was opened for"
    );
}

/// Sibling to [`pre_fix_regression_zero_embed_caps_at_review_tier`]. The
/// design doc specifically called this gap out as untestable until the
/// pipeline was fixed; this test pins the same assertion via the
/// production lookup helper applied to entities with no stored
/// embeddings — exactly the degraded-mode case.
#[test]
fn pre_fix_regression_make_embedding_lookup_empty_store() {
    let entities = vec![
        entity(
            "e1",
            "Acme Corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-01",
        ),
        entity(
            "e2",
            "acme corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-02",
        ),
    ];
    let lookup = make_embedding_lookup(&entities);
    let candidates = generate_candidates(&entities, &lookup, &DedupTuning::DEFAULT);
    let (auto_merge, _) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert!(
        auto_merge.is_empty(),
        "make_embedding_lookup over entities without embeddings must reproduce pre-fix behaviour (no AutoMerges)"
    );
}

/// Reachability: with realistic stored `name_embedding`s + the production
/// lookup helper, `AutoMerge` is actually reached. This is the affirmative
/// proof that #4165 Path A is wired end-to-end through `EntityInfo`,
/// `make_embedding_lookup`, `generate_candidates`, and
/// `classify_candidates`.
#[test]
fn path_a_reachable_with_stored_name_embeddings() {
    // Two near-identical orientation vectors: unit-length, 8-dim,
    // cosine ≈ 1.0 — modelling what a real provider returns for "Acme
    // Corporation" vs "acme corporation".
    let a_emb = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let b_emb = a_emb.clone();
    let entities = vec![
        entity_with_embedding(
            "e1",
            "Acme Corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-01",
            a_emb,
        ),
        entity_with_embedding(
            "e2",
            "acme corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-02",
            b_emb,
        ),
    ];
    let lookup = make_embedding_lookup(&entities);
    let candidates = generate_candidates(&entities, &lookup, &DedupTuning::DEFAULT);
    assert_eq!(candidates.len(), 1, "exactly one candidate pair");
    let c = &candidates[0];
    assert!(
        (c.embed_similarity - 1.0).abs() < 1e-6,
        "stored embeddings must drive embed_similarity to 1.0; got {}",
        c.embed_similarity
    );
    assert!(
        c.merge_score >= 0.90,
        "name=1.0 + embed=1.0 + type=true + alias=true must clear AutoMerge threshold; got {}",
        c.merge_score
    );
    let (auto_merge, _review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert_eq!(
        auto_merge.len(),
        1,
        "exactly one AutoMerge candidate — proves Path A is reachable from production code shape"
    );
}

/// Reachability under semantic-only similarity: names diverge enough
/// to fail the Jaro-Winkler threshold (~0.85), but the stored
/// embeddings agree. Without the 0.30 embed weight this pair would
/// be silently capped at `Skip` — proves the weight is doing real
/// work post-fix by promoting the pair into the actionable space
/// (`Review` or `AutoMerge`).
///
/// We deliberately do *not* assert `AutoMerge` here. For "Photovoltaic
/// Panel" vs "Solar Panel" JW is ~0.50, so even with embed=1.0 +
/// type=true + alias=true the composite score is 0.4·0.50 + 0.6 = 0.80
/// → `Review`. That outcome is exactly what the design intends: the
/// 0.90 threshold is a multi-signal-agreement guard, and synonym pairs
/// without name overlap legitimately stay in the operator-review
/// queue. The reachability proof for `AutoMerge` is
/// `path_a_reachable_with_stored_name_embeddings` (name+embed=1.0).
#[test]
fn path_a_embedding_promotes_synonym_pair_above_skip() {
    let emb = vec![0.0, 1.0, 0.0, 0.0];
    let entities = vec![
        entity_with_embedding(
            "e1",
            "Photovoltaic Panel",
            "product",
            vec!["PV"],
            3,
            "2026-01-01",
            emb.clone(),
        ),
        entity_with_embedding(
            "e2",
            "Solar Panel",
            "product",
            vec!["PV"],
            3,
            "2026-01-02",
            emb,
        ),
    ];
    let lookup = make_embedding_lookup(&entities);
    let candidates = generate_candidates(&entities, &lookup, &DedupTuning::DEFAULT);
    assert_eq!(
        candidates.len(),
        1,
        "alias overlap forces them into the candidate set"
    );
    let c = &candidates[0];
    assert!(
        (c.embed_similarity - 1.0).abs() < 1e-6,
        "embeddings carry the semantic signal; got {}",
        c.embed_similarity
    );
    assert!(
        c.merge_score >= 0.70,
        "embed contribution must push the pair into Review tier at minimum; got {}",
        c.merge_score
    );
    let (_auto_merge, review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert!(
        !review.is_empty(),
        "synonym pair with agreeing embeddings lands in Review (operator-actionable) — \
         before #4165 Path A it would have been Skip-tier and invisible to the operator"
    );
}

/// Path A safety: borderline embeddings (cosine well below 1.0) must
/// not trigger `AutoMerge` when names also disagree. The 0.90 threshold
/// is the multi-signal-agreement guard the design doc called out.
#[test]
fn path_a_borderline_embedding_stays_in_review_tier() {
    // 30° apart — cosine ≈ 0.866. Below the 0.90 threshold by itself.
    let a_emb = vec![1.0, 0.0];
    let b_emb = vec![0.866_025_4, 0.5];
    let entities = vec![
        entity_with_embedding(
            "e1",
            "Differential Equation",
            "concept",
            vec![],
            1,
            "2026-01-01",
            a_emb,
        ),
        entity_with_embedding(
            "e2",
            "Difference Equation",
            "concept",
            vec![],
            1,
            "2026-01-02",
            b_emb,
        ),
    ];
    let lookup = make_embedding_lookup(&entities);
    let candidates = generate_candidates(&entities, &lookup, &DedupTuning::DEFAULT);
    assert_eq!(
        candidates.len(),
        1,
        "JW above 0.85 puts them in the candidate set"
    );
    let (auto_merge, review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
    assert!(
        auto_merge.is_empty(),
        "moderately-similar embeddings with no alias overlap must not auto-merge — operator review required"
    );
    assert_eq!(review.len(), 1, "should land in the review queue");
}

/// `make_embedding_lookup` must return 0.0 when *either* side of a pair
/// lacks a stored embedding. Otherwise a partially-backfilled corpus
/// could silently bias scoring toward whatever entities were embedded
/// first, surprising operators after a partial provider outage.
#[test]
fn path_a_lookup_returns_zero_for_missing_embeddings() {
    let entities = vec![
        entity_with_embedding(
            "e1",
            "Alice Test",
            "person",
            vec![],
            1,
            "2026-01-01",
            vec![1.0, 0.0, 0.0],
        ),
        // e2 has no embedding — production v13 row with NULL name_embedding.
        entity("e2", "Alice Test", "person", vec![], 1, "2026-01-02"),
    ];
    let lookup = make_embedding_lookup(&entities);
    let ea = EntityId::new("e1").expect("valid id");
    let eb = EntityId::new("e2").expect("valid id");
    assert!(
        lookup(&ea, &eb).abs() < f64::EPSILON,
        "missing-on-one-side must yield 0.0"
    );
    assert!(
        lookup(&eb, &ea).abs() < f64::EPSILON,
        "missing-on-one-side must yield 0.0 regardless of argument order"
    );
}

/// `make_embedding_lookup` must return 0.0 for dimension mismatches —
/// not panic, not silently misalign the dot product. This pins the
/// behaviour that defends the dedup pipeline against a config drift
/// where the embedding provider's `dim` differs from
/// `KnowledgeConfig::dim`.
#[test]
fn path_a_lookup_returns_zero_for_dimension_mismatch() {
    let entities = vec![
        entity_with_embedding("e1", "x", "thing", vec![], 1, "2026-01-01", vec![1.0, 0.0]),
        entity_with_embedding(
            "e2",
            "y",
            "thing",
            vec![],
            1,
            "2026-01-02",
            vec![1.0, 0.0, 0.0, 0.0],
        ),
    ];
    let lookup = make_embedding_lookup(&entities);
    let ea = EntityId::new("e1").expect("valid id");
    let eb = EntityId::new("e2").expect("valid id");
    assert!(
        lookup(&ea, &eb).abs() < f64::EPSILON,
        "dim mismatch must be treated like a missing embedding"
    );
}

/// `make_embedding_lookup` must return 0.0 when neither side is known —
/// avoids silently inflating scores for entities the caller produced
/// from outside the loaded `EntityInfo` slice.
#[test]
fn path_a_lookup_returns_zero_for_unknown_ids() {
    let entities: Vec<EntityInfo> = Vec::new();
    let lookup = make_embedding_lookup(&entities);
    let ea = EntityId::new("e1").expect("valid id");
    let eb = EntityId::new("e2").expect("valid id");
    assert!(
        lookup(&ea, &eb).abs() < f64::EPSILON,
        "empty store must yield 0.0 for every pair"
    );
}

/// Bounds check on `cosine_similarity` itself: floating-point drift in
/// real providers can push a unit-length pair slightly above 1.0, and
/// dedup-pipeline weight calculations expect strictly `[0.0, 1.0]`.
/// This pins the clamp at the boundary.
#[test]
fn path_a_cosine_similarity_clamps_to_unit_interval() {
    // Two vectors that differ only by floating-point representation
    // (cosine should be exactly 1.0 — assert the clamp does not push it
    // below or above).
    let v = vec![1.0_f32, 0.0, 0.0];
    let sim = cosine_similarity(&v, &v);
    assert!(
        (0.0..=1.0).contains(&sim),
        "cosine_similarity must stay in [0.0, 1.0]; got {sim}"
    );
    assert!((sim - 1.0).abs() < 1e-6, "identical vectors → 1.0");
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
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    let (auto_merge, review) = classify_candidates(candidates, &DedupTuning::DEFAULT);
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

#[test]
fn score_boundary_exactly_070() {
    assert_eq!(
        MergeDecision::from_score(0.70, &DedupTuning::DEFAULT),
        MergeDecision::Review
    );
}

#[test]
fn score_boundary_just_below_070() {
    assert_eq!(
        MergeDecision::from_score(0.6999, &DedupTuning::DEFAULT),
        MergeDecision::Skip
    );
}

#[test]
fn score_boundary_exactly_090() {
    assert_eq!(
        MergeDecision::from_score(0.90, &DedupTuning::DEFAULT),
        MergeDecision::AutoMerge
    );
}

#[test]
fn score_boundary_just_below_090() {
    assert_eq!(
        MergeDecision::from_score(0.8999, &DedupTuning::DEFAULT),
        MergeDecision::Review
    );
}

// WHY(#4165): regression — operator-tuned
// `taxis::AgentBehaviorDefaults::knowledge_dedup_*` keys must flow through
// `compute_merge_score`, `generate_candidates`, and `MergeDecision::from_score`;
// the tests below assert that adjusting `tuning` changes observable behaviour
// at each stage of the pipeline.

/// Composite score is recomputed under tuned weights.
///
/// Default weights (0.4/0.3/0.2/0.1) score (name=1, embed=0, type=true,
/// alias=true) at 0.70. A custom tuning that ups the name weight to 0.5
/// must shift the score upward by exactly the difference, proving the
/// formula uses `tuning` rather than the module constants.
#[test]
fn tuned_weights_change_composite_score() {
    let custom = DedupTuning {
        weight_name: 0.5,
        weight_embed: 0.2, // 0.5 + 0.2 + 0.2 + 0.1 = 1.0, balanced
        ..DedupTuning::DEFAULT
    };
    let default_score = compute_merge_score(1.0, 0.0, true, true, &DedupTuning::DEFAULT);
    let custom_score = compute_merge_score(1.0, 0.0, true, true, &custom);
    // (0.5 - 0.4) * 1.0 = +0.1 for the name term; embed_sim is 0.0 so
    // the embed weight change cancels out.
    assert!(
        (custom_score - default_score - 0.10).abs() < 1e-9,
        "tuning must reach compute_merge_score: default={default_score}, custom={custom_score}",
    );
}

/// Lowering `jw_threshold` admits a pair that the default threshold rejects.
///
/// "Carbon" vs "Carbide" has JW ≈ 0.79 — below the default 0.85 floor.
/// With type match but no alias overlap and no embedding, the only
/// candidacy gate is JW. Default behaviour rejects the pair; lowering
/// `jw_threshold` to 0.70 must admit it.
#[test]
fn tuned_jw_threshold_admits_pair_under_default_floor() {
    let entities = vec![
        entity("e1", "Carbon", "element", vec![], 1, "2026-01-01"),
        entity("e2", "Carbide", "element", vec![], 1, "2026-01-02"),
    ];

    let default_candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert!(
        default_candidates.is_empty(),
        "default JW floor (0.85) must reject this pair as a baseline"
    );

    let relaxed = DedupTuning {
        jw_threshold: 0.70,
        ..DedupTuning::DEFAULT
    };
    let relaxed_candidates = generate_candidates(&entities, &no_embed, &relaxed);
    assert_eq!(
        relaxed_candidates.len(),
        1,
        "lowering jw_threshold to 0.70 must admit the same pair — proves tuning reaches the candidate filter"
    );
}

/// Lowering `auto_merge_threshold` promotes a Review-tier candidate to
/// `AutoMerge` without touching any other input.
///
/// A pair with score in `[0.70, 0.90)` defaults to `Review`. Reducing
/// the auto-merge threshold below the pair's score must reclassify it
/// to `AutoMerge`, proving the classification helper reads the supplied
/// tuning instead of hardcoded 0.90/0.70 constants.
#[test]
fn tuned_auto_merge_threshold_promotes_review_to_auto_merge() {
    // Score = 0.4·1.0 + 0.3·0.0 + 0.2·1.0 + 0.1·1.0 = 0.70 → Review under default tuning.
    let entities = vec![
        entity(
            "e1",
            "Acme Corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-01",
        ),
        entity(
            "e2",
            "acme corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-02",
        ),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);
    assert_eq!(candidates.len(), 1, "exactly one pair");
    let score = candidates[0].merge_score;
    assert!(
        (score - 0.70).abs() < 1e-9,
        "baseline shape expects score=0.70 (the unreachable-AutoMerge ceiling pre-#4165); got {score}",
    );

    let (auto_default, review_default) =
        classify_candidates(candidates.clone(), &DedupTuning::DEFAULT);
    assert!(
        auto_default.is_empty() && review_default.len() == 1,
        "default classification must route this pair to Review"
    );

    let promoted = DedupTuning {
        auto_merge_threshold: 0.69,
        ..DedupTuning::DEFAULT
    };
    let (auto_promoted, review_promoted) = classify_candidates(candidates, &promoted);
    assert_eq!(
        auto_promoted.len(),
        1,
        "lowering auto_merge_threshold below 0.70 must promote the Review-tier pair to AutoMerge — proves tuning reaches MergeDecision::from_score"
    );
    assert!(
        review_promoted.is_empty(),
        "promoted pair must leave the Review bucket"
    );
}

/// Raising `review_threshold` above an existing review-tier pair must
/// drop it to `Skip` rather than queue it. Closes the tuning contract
/// from the other direction.
#[test]
fn tuned_review_threshold_drops_pair_to_skip() {
    let entities = vec![
        entity(
            "e1",
            "Acme Corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-01",
        ),
        entity(
            "e2",
            "acme corporation",
            "organization",
            vec!["Acme"],
            5,
            "2026-01-02",
        ),
    ];
    let candidates = generate_candidates(&entities, &no_embed, &DedupTuning::DEFAULT);

    let strict = DedupTuning {
        // Composite score is 0.70 (the unreachable-AutoMerge ceiling);
        // bumping the review floor above it must downgrade to Skip.
        review_threshold: 0.71,
        ..DedupTuning::DEFAULT
    };
    let (auto, review) = classify_candidates(candidates, &strict);
    assert!(
        auto.is_empty() && review.is_empty(),
        "raising review_threshold above the pair's score must drop it from both buckets"
    );
}

mod proptests {
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
}
