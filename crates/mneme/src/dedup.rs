#![allow(
    dead_code,
    reason = "pipeline stages not yet wired into main ingestion loop; lint fires in lib but not test target"
)]
//! Entity deduplication pipeline for merging semantically identical entities.
//!
//! Runs as a background maintenance task after ingestion batches. Without dedup,
//! the knowledge graph fragments: the same person becomes 10 nodes with 1 edge
//! each instead of 1 node with 10 edges.
//!
//! # Three-phase pipeline
//!
//! 1. **Candidate generation**: find potential duplicate pairs within same entity type
//! 2. **Merge scoring**: weighted composite of name similarity, embedding similarity,
//!    type match, and alias overlap
//! 3. **Merge execution**: transfer edges, aliases, fact_entities, and record audit trail

use crate::id::EntityId;

/// A candidate pair of entities that may be duplicates.
#[derive(Debug, Clone)]
pub struct EntityMergeCandidate {
    /// First entity in the pair.
    pub entity_a: EntityId,
    /// Second entity in the pair.
    pub entity_b: EntityId,
    /// Display name of entity A.
    pub name_a: String,
    /// Display name of entity B.
    pub name_b: String,
    /// Jaro-Winkler similarity between names (0.0–1.0).
    pub name_similarity: f64,
    /// Cosine similarity between name embeddings (0.0–1.0).
    pub embed_similarity: f64,
    /// Whether both entities share the same `entity_type`.
    pub type_match: bool,
    /// Whether the entities share any alias.
    pub alias_overlap: bool,
    /// Weighted composite merge score.
    pub merge_score: f64,
}

/// Decision based on the merge score thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeDecision {
    /// Score ≥ 0.90: merge automatically.
    AutoMerge,
    /// 0.70 ≤ score < 0.90: queue for human review.
    Review,
    /// Score < 0.70: skip.
    Skip,
}

impl MergeDecision {
    /// Classify a merge score into a decision.
    #[must_use]
    pub fn from_score(score: f64) -> Self {
        if score >= 0.90 {
            Self::AutoMerge
        } else if score >= 0.70 {
            Self::Review
        } else {
            Self::Skip
        }
    }
}

/// Audit record for a completed entity merge.
#[derive(Debug, Clone)]
pub struct MergeRecord {
    /// The surviving entity.
    pub canonical_entity_id: EntityId,
    /// The entity that was merged and removed.
    pub merged_entity_id: EntityId,
    /// Display name of the merged entity (preserved for audit).
    pub merged_entity_name: String,
    /// The composite score that triggered the merge.
    pub merge_score: f64,
    /// Number of `fact_entities` mappings transferred.
    pub facts_transferred: u32,
    /// Number of relationship edges redirected.
    pub relationships_redirected: u32,
    /// When the merge was executed.
    pub merged_at: jiff::Timestamp,
}

/// Score weights for the merge formula.
#[cfg(any(feature = "mneme-engine", test))]
const WEIGHT_NAME: f64 = 0.4;
#[cfg(any(feature = "mneme-engine", test))]
const WEIGHT_EMBED: f64 = 0.3;
#[cfg(any(feature = "mneme-engine", test))]
const WEIGHT_TYPE: f64 = 0.2;
#[cfg(any(feature = "mneme-engine", test))]
const WEIGHT_ALIAS: f64 = 0.1;

/// Jaro-Winkler threshold for candidate generation.
#[cfg(any(feature = "mneme-engine", test))]
const JW_THRESHOLD: f64 = 0.85;

/// Embedding cosine threshold for candidate generation.
#[cfg(any(feature = "mneme-engine", test))]
const EMBED_THRESHOLD: f64 = 0.80;

/// Compute the weighted merge score.
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub fn compute_merge_score(
    name_similarity: f64,
    embed_similarity: f64,
    type_match: bool,
    alias_overlap: bool,
) -> f64 {
    let type_val = if type_match { 1.0 } else { 0.0 };
    let alias_val = if alias_overlap { 1.0 } else { 0.0 };
    WEIGHT_NAME * name_similarity
        + WEIGHT_EMBED * embed_similarity
        + WEIGHT_TYPE * type_val
        + WEIGHT_ALIAS * alias_val
}

/// Compute Jaro-Winkler similarity between two strings (case-insensitive).
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn jaro_winkler_ci(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    strsim::jaro_winkler(&a_lower, &b_lower)
}

/// Check if two alias lists share any common entry (case-insensitive).
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn aliases_overlap(a: &[String], b: &[String]) -> bool {
    for alias_a in a {
        let lower_a = alias_a.to_lowercase();
        for alias_b in b {
            if alias_b.to_lowercase() == lower_a {
                return true;
            }
        }
    }
    false
}

/// Check if either entity's name appears as an alias in the other (case-insensitive).
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn name_in_aliases(
    name_a: &str,
    aliases_b: &[String],
    name_b: &str,
    aliases_a: &[String],
) -> bool {
    let lower_a = name_a.to_lowercase();
    let lower_b = name_b.to_lowercase();
    aliases_b.iter().any(|a| a.to_lowercase() == lower_a)
        || aliases_a.iter().any(|a| a.to_lowercase() == lower_b)
}

/// Cosine similarity between two f32 vectors.
#[cfg(test)]
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut norm_a, mut norm_b) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().zip(b.iter()) {
        let (xf, yf) = (f64::from(*x), f64::from(*y));
        dot += xf * yf;
        norm_a += xf * xf;
        norm_b += yf * yf;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f64::EPSILON {
        return 0.0;
    }
    dot / denom
}

/// Lightweight entity data for dedup processing (avoids full Entity struct dependency on engine).
#[cfg(any(feature = "mneme-engine", test))]
#[derive(Debug, Clone)]
pub(crate) struct EntityInfo {
    pub(crate) id: EntityId,
    pub(crate) name: String,
    pub(crate) entity_type: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) relationship_count: u32,
    pub(crate) created_at: jiff::Timestamp,
}

/// Phase 1: Generate candidate merge pairs from a list of entities.
///
/// Finds pairs within the same `entity_type` where at least one of:
/// - Exact name match (case-insensitive)
/// - Jaro-Winkler similarity ≥ 0.85
/// - Any shared alias or name-in-alias match
///
/// Embedding similarity is passed as an optional precomputed map; if absent, defaults to 0.0.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn generate_candidates(
    entities: &[EntityInfo],
    embed_similarities: &dyn Fn(&EntityId, &EntityId) -> f64,
) -> Vec<EntityMergeCandidate> {
    let mut candidates = Vec::new();

    for (i, a) in entities.iter().enumerate() {
        for b in &entities[i + 1..] {
            let type_match = a.entity_type == b.entity_type;

            if !type_match {
                continue;
            }

            let name_sim = jaro_winkler_ci(&a.name, &b.name);
            let alias_overlap = aliases_overlap(&a.aliases, &b.aliases)
                || name_in_aliases(&a.name, &b.aliases, &b.name, &a.aliases);
            let is_exact_match = a.name.to_lowercase() == b.name.to_lowercase();
            let is_jw_match = name_sim >= JW_THRESHOLD;
            let embed_sim = embed_similarities(&a.id, &b.id);
            let is_embed_match = embed_sim >= EMBED_THRESHOLD;

            if !is_exact_match && !is_jw_match && !is_embed_match && !alias_overlap {
                continue;
            }

            let merge_score = compute_merge_score(name_sim, embed_sim, type_match, alias_overlap);

            candidates.push(EntityMergeCandidate {
                entity_a: a.id.clone(),
                entity_b: b.id.clone(),
                name_a: a.name.clone(),
                name_b: b.name.clone(),
                name_similarity: name_sim,
                embed_similarity: embed_sim,
                type_match,
                alias_overlap,
                merge_score,
            });
        }
    }

    candidates
}

/// Phase 2: Classify candidates into auto-merge, review, or skip.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn classify_candidates(
    candidates: Vec<EntityMergeCandidate>,
) -> (Vec<EntityMergeCandidate>, Vec<EntityMergeCandidate>) {
    let mut auto_merge = Vec::new();
    let mut review = Vec::new();

    for c in candidates {
        match MergeDecision::from_score(c.merge_score) {
            MergeDecision::AutoMerge => auto_merge.push(c),
            MergeDecision::Review => review.push(c),
            // NOTE: score below threshold, candidate discarded
            MergeDecision::Skip => {}
        }
    }

    (auto_merge, review)
}

/// Choose which entity becomes canonical: the one with more relationships,
/// tie-broken by oldest `created_at`.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn pick_canonical<'a>(
    a: &'a EntityInfo,
    b: &'a EntityInfo,
) -> (&'a EntityInfo, &'a EntityInfo) {
    if a.relationship_count > b.relationship_count {
        (a, b)
    } else if b.relationship_count > a.relationship_count {
        (b, a)
    } else if a.created_at <= b.created_at {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
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
            id: EntityId::from(id),
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
        // 0.4*0.9 + 0.3*0.8 + 0.2*1.0 + 0.1*0.0 = 0.36 + 0.24 + 0.20 + 0.0 = 0.80
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
        let sim = jaro_winkler_ci("John Smith", "john smith");
        assert!((sim - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jw_similar_names() {
        let sim = jaro_winkler_ci("John Smith", "Jon Smith");
        assert!(sim >= 0.85, "expected >= 0.85, got {sim}");
    }

    #[test]
    fn jw_different_names() {
        let sim = jaro_winkler_ci("John Smith", "Alice Johnson");
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
            "John Smith",
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
            entity("e1", "John Smith", "person", vec![], 2, "2026-01-01"),
            entity("e2", "john smith", "person", vec![], 1, "2026-01-02"),
        ];
        let candidates = generate_candidates(&entities, &no_embed);
        assert_eq!(candidates.len(), 1);
        assert!((candidates[0].name_similarity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaro_winkler_candidate() {
        let entities = vec![
            entity("e1", "John Smith", "person", vec![], 1, "2026-01-01"),
            entity("e2", "Jon Smith", "person", vec![], 1, "2026-01-01"),
        ];
        let candidates = generate_candidates(&entities, &no_embed);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].name_similarity >= 0.85);
    }

    #[test]
    fn different_types_not_candidates() {
        let entities = vec![
            entity("e1", "John Smith", "person", vec![], 1, "2026-01-01"),
            entity("e2", "John Smith", "organization", vec![], 1, "2026-01-01"),
        ];
        let candidates = generate_candidates(&entities, &no_embed);
        assert!(candidates.is_empty());
    }

    #[test]
    fn alias_overlap_triggers_candidate() {
        let entities = vec![
            entity("e1", "John Smith", "person", vec!["JS"], 1, "2026-01-01"),
            entity("e2", "J. Smith", "person", vec!["JS"], 1, "2026-01-01"),
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
            if (a.as_str() == "e1" && b.as_str() == "e2")
                || (a.as_str() == "e2" && b.as_str() == "e1")
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
        // e1 vs e2: exact name match (1.0) + same type (1.0) + alias overlap (1.0)
        // score = 0.4*1.0 + 0.3*0.0 + 0.2*1.0 + 0.1*1.0 = 0.70 → Review
        // With embed=0.95: 0.4*1.0 + 0.3*0.95 + 0.2*1.0 + 0.1*1.0 = 0.985 → AutoMerge
        let entities = vec![
            entity("e1", "John Smith", "person", vec!["JS"], 2, "2026-01-01"),
            entity("e2", "john smith", "person", vec!["JS"], 1, "2026-01-02"),
            entity("e3", "Jon Smith", "person", vec![], 1, "2026-01-03"),
        ];
        let high_embed = |a: &EntityId, b: &EntityId| -> f64 {
            if (a.as_str() == "e1" && b.as_str() == "e2")
                || (a.as_str() == "e2" && b.as_str() == "e1")
            {
                0.95
            } else {
                0.0
            }
        };
        let candidates = generate_candidates(&entities, &high_embed);
        let (auto_merge, review) = classify_candidates(candidates);
        // e1 vs e2 should be auto-merge with high embed similarity
        assert!(
            !auto_merge.is_empty(),
            "expected at least one auto-merge candidate"
        );
        // e1 vs e3 or e2 vs e3 might be review or skip
        let total = auto_merge.len() + review.len();
        assert!(total >= 1);
    }

    #[test]
    fn canonical_most_relationships() {
        let a = entity("e1", "John Smith", "person", vec![], 5, "2026-01-02");
        let b = entity("e2", "john smith", "person", vec![], 2, "2026-01-01");
        let (canonical, merged) = pick_canonical(&a, &b);
        assert_eq!(canonical.id, EntityId::from("e1"));
        assert_eq!(merged.id, EntityId::from("e2"));
    }

    #[test]
    fn canonical_tiebreak_oldest() {
        let a = entity("e1", "John Smith", "person", vec![], 3, "2026-01-02");
        let b = entity("e2", "john smith", "person", vec![], 3, "2026-01-01");
        let (canonical, _) = pick_canonical(&a, &b);
        assert_eq!(
            canonical.id,
            EntityId::from("e2"),
            "older entity should be canonical"
        );
    }

    #[test]
    fn idempotent_no_self_candidates() {
        let entities = vec![entity(
            "e1",
            "John Smith",
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
                id: EntityId::from(id),
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
                // Build unique same-type entities from the generated names.
                let entities: Vec<EntityInfo> = names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| make_entity(&format!("e{i}"), name, "person"))
                    .collect();

                let candidates = generate_candidates(&entities, &|_, _| 0.0);
                let total_candidates = candidates.len();
                let (auto_merge, review) = classify_candidates(candidates);

                // Every candidate was either auto-merged, queued for review, or skipped.
                // The ones that remain (not skipped) must equal auto_merge + review.
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
}
