use super::super::{
    DedupTuning, MergeDecision, classify_candidates, compute_merge_score, generate_candidates,
};
use super::{entity, no_embed};

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
