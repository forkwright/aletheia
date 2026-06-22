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
        fact_count: 0,
        created_at: ts(created),
        name_embedding: None,
    }
}

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
/// `kanon/projects/aletheia/planning/research/memory-dedup-reachability.md`),
/// making the change visible.
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
