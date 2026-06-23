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
#![cfg_attr(
    any(feature = "mneme-engine", test),
    expect(
        clippy::as_conversions,
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

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
    /// Jaro-Winkler similarity between names (0.0--1.0).
    pub name_similarity: f64,
    /// Cosine similarity between name embeddings (0.0--1.0).
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
    /// Score ≥ `tuning.auto_merge_threshold` (default 0.90): merge automatically.
    AutoMerge,
    /// `tuning.review_threshold` ≤ score < `tuning.auto_merge_threshold`
    /// (default 0.70..0.90): queue for human review.
    Review,
    /// Score < `tuning.review_threshold` (default 0.70): skip.
    Skip,
}

impl MergeDecision {
    /// Classify a merge score into a decision against the supplied tuning.
    #[cfg(any(feature = "mneme-engine", test))]
    #[must_use]
    pub(crate) fn from_score(score: f64, tuning: &DedupTuning) -> Self {
        if score >= tuning.auto_merge_threshold {
            Self::AutoMerge
        } else if score >= tuning.review_threshold {
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

/// Default name-similarity weight in the composite merge score.
///
/// Operators tuning this should set `knowledge_dedup_weight_name` in
/// `taxis::config::AgentBehaviorDefaults`; runtime callers should build a
/// [`DedupTuning`] from that config and pass it into [`generate_candidates`].
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_WEIGHT_NAME: f64 = 0.4;
/// Default embedding-similarity weight in the composite merge score.
///
/// See [`DEFAULT_WEIGHT_NAME`] for the config plumbing pattern.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_WEIGHT_EMBED: f64 = 0.3;
/// Default `entity_type`-match weight in the composite merge score.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_WEIGHT_TYPE: f64 = 0.2;
/// Default alias-overlap weight in the composite merge score.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_WEIGHT_ALIAS: f64 = 0.1;

/// Default Jaro-Winkler threshold for candidate generation.
///
/// See [`DedupTuning`] for the config-driven path operators should prefer.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_JW_THRESHOLD: f64 = 0.85;

/// Default embedding cosine threshold for candidate generation.
///
/// See [`DedupTuning`] for the config-driven path operators should prefer.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_EMBED_THRESHOLD: f64 = 0.80;

/// Default composite score at which `MergeDecision::AutoMerge` fires.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_AUTO_MERGE_THRESHOLD: f64 = 0.90;

/// Default composite score above which candidates queue for `MergeDecision::Review`.
#[cfg(any(feature = "mneme-engine", test))]
pub const DEFAULT_REVIEW_THRESHOLD: f64 = 0.70;

/// Runtime-tunable parameters for the dedup pipeline.
///
/// Mirrors the `taxis::config::AgentBehaviorDefaults::knowledge_dedup_*`
/// configuration keys; CLI and maintenance callers build one from the
/// resolved agent config and pass it into the dedup entry points so
/// operator changes actually take effect (#4165 D). Tests and crate-internal
/// callers can use [`DedupTuning::DEFAULT`] for the pre-config behaviour.
///
/// **Invariant**: the score weights are scaled by similarity and
/// match-flag values that range in `[0.0, 1.0]`, so the maximum reachable
/// composite score is `weight_name + weight_embed + weight_type +
/// weight_alias`. Operators tuning the weights must keep the auto-merge
/// threshold reachable (otherwise the pipeline is back in the
/// "unreachable `AutoMerge`" failure mode that #4165 fixed).
#[cfg(any(feature = "mneme-engine", test))]
#[derive(Debug, Clone, Copy)]
pub struct DedupTuning {
    /// Weight applied to the name similarity term. Default
    /// [`DEFAULT_WEIGHT_NAME`].
    pub weight_name: f64,
    /// Weight applied to the embedding similarity term. Default
    /// [`DEFAULT_WEIGHT_EMBED`].
    pub weight_embed: f64,
    /// Weight applied to the `entity_type`-match term. Default
    /// [`DEFAULT_WEIGHT_TYPE`].
    pub weight_type: f64,
    /// Weight applied to the alias-overlap term. Default
    /// [`DEFAULT_WEIGHT_ALIAS`].
    pub weight_alias: f64,
    /// Minimum Jaro-Winkler score that admits a pair as a candidate.
    /// Default [`DEFAULT_JW_THRESHOLD`].
    pub jw_threshold: f64,
    /// Minimum cosine embedding similarity that admits a pair as a
    /// candidate. Default [`DEFAULT_EMBED_THRESHOLD`].
    pub embed_threshold: f64,
    /// Composite score above which the pipeline auto-merges without
    /// operator review. Default [`DEFAULT_AUTO_MERGE_THRESHOLD`].
    pub auto_merge_threshold: f64,
    /// Composite score above which a candidate queues for operator
    /// review. Default [`DEFAULT_REVIEW_THRESHOLD`].
    pub review_threshold: f64,
}

#[cfg(any(feature = "mneme-engine", test))]
impl DedupTuning {
    /// Default tuning matching the pre-config behaviour: classic
    /// 0.4/0.3/0.2/0.1 weights, 0.85 JW floor, 0.80 embed floor,
    /// 0.90 auto-merge, 0.70 review.
    pub const DEFAULT: Self = Self {
        weight_name: DEFAULT_WEIGHT_NAME,
        weight_embed: DEFAULT_WEIGHT_EMBED,
        weight_type: DEFAULT_WEIGHT_TYPE,
        weight_alias: DEFAULT_WEIGHT_ALIAS,
        jw_threshold: DEFAULT_JW_THRESHOLD,
        embed_threshold: DEFAULT_EMBED_THRESHOLD,
        auto_merge_threshold: DEFAULT_AUTO_MERGE_THRESHOLD,
        review_threshold: DEFAULT_REVIEW_THRESHOLD,
    };
}

#[cfg(any(feature = "mneme-engine", test))]
impl Default for DedupTuning {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Compute the weighted merge score under `tuning`.
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn compute_merge_score(
    name_similarity: f64,
    embed_similarity: f64,
    type_match: bool,
    alias_overlap: bool,
    tuning: &DedupTuning,
) -> f64 {
    let type_val = if type_match { 1.0 } else { 0.0 };
    let alias_val = if alias_overlap { 1.0 } else { 0.0 };
    tuning.weight_name * name_similarity
        + tuning.weight_embed * embed_similarity
        + tuning.weight_type * type_val
        + tuning.weight_alias * alias_val
}

/// Compute Jaro-Winkler similarity between two strings (case-insensitive).
///
/// Inlined implementation (replaces strsim dependency). The algorithm:
/// 1. Jaro similarity: matching characters within a window + transpositions
/// 2. Winkler boost: bonus for shared prefix (up to 4 chars, weight 0.1)
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn jaro_winkler_ci(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    jaro_winkler(&a_lower, &b_lower)
}

/// Jaro-Winkler similarity. Returns 0.0 (no match) to 1.0 (identical).
#[cfg(any(feature = "mneme-engine", test))]
fn jaro_winkler(s1: &str, s2: &str) -> f64 {
    let jaro = jaro(s1, s2);
    if jaro == 0.0 {
        return 0.0;
    }
    let prefix_len = s1
        .chars()
        .zip(s2.chars())
        .take(4)
        .take_while(|(a, b)| a == b)
        .count();
    #[expect(
        clippy::cast_precision_loss,
        reason = "prefix_len is at most 4, well within f64 mantissa"
    )]
    let prefix_f = prefix_len as f64;
    jaro + (prefix_f * 0.1 * (1.0 - jaro))
}

/// Jaro similarity between two strings.
#[cfg(any(feature = "mneme-engine", test))]
fn jaro(s1: &str, s2: &str) -> f64 {
    if s1.is_empty() && s2.is_empty() {
        return 1.0;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let s1_len = s1_chars.len();
    let s2_len = s2_chars.len();
    let match_distance = (s1_len.max(s2_len) / 2).saturating_sub(1);
    let mut s1_matched = vec![false; s1_len];
    let mut s2_matched = vec![false; s2_len];
    let mut matches: f64 = 0.0;
    for i in 0..s1_len {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(s2_len);
        for j in start..end {
            if !s2_matched[j] && s1_chars[i] == s2_chars[j] {
                s1_matched[i] = true;
                s2_matched[j] = true;
                matches += 1.0;
                break;
            }
        }
    }
    if matches == 0.0 {
        return 0.0;
    }
    let mut transpositions = 0.0;
    let mut k = 0;
    for i in 0..s1_len {
        if !s1_matched[i] {
            continue;
        }
        while !s2_matched[k] {
            k += 1;
        }
        if s1_chars[i] != s2_chars[k] {
            transpositions += 1.0;
        }
        k += 1;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "string lengths won't exceed 2^52 in practice"
    )]
    let s1_f = s1_len as f64;
    #[expect(
        clippy::cast_precision_loss,
        reason = "string lengths won't exceed 2^52 in practice"
    )]
    let s2_f = s2_len as f64;
    (matches / s1_f + matches / s2_f + (matches - transpositions / 2.0) / matches) / 3.0
}

/// Check if two alias lists share any common entry (case-insensitive).
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn aliases_overlap(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    // WHY(#5670): O(a+b) HashSet membership replaces the previous O(a×b)
    // nested loop, which became a hotspot inside the O(N²) candidate loop.
    let normalized: std::collections::HashSet<String> =
        a.iter().map(|alias| alias.to_lowercase()).collect();
    b.iter()
        .map(|alias| alias.to_lowercase())
        .any(|alias| normalized.contains(&alias))
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
///
/// Returns 0.0 for mismatched dimensions, empty vectors, or any vector with
/// zero magnitude. The result is *clamped* to `[0.0, 1.0]` so the dedup
/// composite score stays in its declared domain even when an embedding
/// provider returns slightly out-of-range values due to floating-point
/// drift. (`embed_sim` is the second-largest weight in the merge formula —
/// a slightly-negative value here would silently lower the score.)
#[cfg_attr(
    not(any(feature = "mneme-engine", test)),
    expect(
        dead_code,
        reason = "exposed for the dedup pipeline gated behind mneme-engine"
    )
)]
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
    (dot / denom).clamp(0.0, 1.0)
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
    /// Cached embedding of [`Self::name`] loaded from the entities relation
    /// (`name_embedding` column, added in schema v13). `None` for entities
    /// inserted before the v13 migration, or while no `EmbeddingProvider`
    /// was in scope; `make_embedding_lookup` returns 0.0 for any pair
    /// involving a `None`.
    pub(crate) name_embedding: Option<Vec<f32>>,
}

/// Build a pairwise cosine-similarity lookup over `entities.name_embedding`.
///
/// Returns a closure suitable for [`generate_candidates`]; the closure
/// computes [`cosine_similarity`] between the cached `name_embedding`s of
/// the two entities and returns 0.0 when either is absent or the lookup
/// misses (defensive: an unknown id should not silently inflate
/// `embed_sim`).
///
/// This is the production replacement for the historical `|_, _| 0.0`
/// closures at `find_duplicate_entities` and `run_entity_dedup` that made
/// `MergeDecision::AutoMerge` (≥ 0.90) structurally unreachable (#4165).
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn make_embedding_lookup(
    entities: &[EntityInfo],
) -> impl Fn(&EntityId, &EntityId) -> f64 + '_ {
    use std::collections::HashMap;
    let map: HashMap<&str, &[f32]> = entities
        .iter()
        .filter_map(|e| e.name_embedding.as_deref().map(|v| (e.id.as_str(), v)))
        .collect();
    move |a, b| match (map.get(a.as_str()), map.get(b.as_str())) {
        (Some(va), Some(vb)) => cosine_similarity(va, vb),
        _ => 0.0,
    }
}

/// Phase 1: Generate candidate merge pairs from a list of entities.
///
/// Finds pairs within the same `entity_type` where at least one of:
/// - Exact name match (case-insensitive)
/// - Jaro-Winkler similarity ≥ `tuning.jw_threshold`
/// - Embedding similarity ≥ `tuning.embed_threshold`
/// - Any shared alias or name-in-alias match
///
/// The composite score uses the weights on `tuning`. Embedding similarity
/// is supplied by the closure — for pairs whose stored `name_embedding`
/// is `None`, [`make_embedding_lookup`] returns `0.0` so the pipeline
/// degrades to the pre-fix score range (#4165 Path A).
///
/// # Complexity
///
/// Previous implementation compared every same-type pair (O(N²)). This
/// version blocks entities by type, then emits candidate pairs from:
///
/// 1. A normalized token index (names + aliases) — covers exact names,
///    alias overlap, and name-in-alias matches in O(N·k) index work.
/// 2. A sorted-name sliding window — covers approximate Jaro-Winkler
///    matches in O(N·window) comparisons per type.
///
/// The full similarity and embedding checks run only over the emitted
/// candidate pairs.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn generate_candidates(
    entities: &[EntityInfo],
    embed_similarities: &dyn Fn(&EntityId, &EntityId) -> f64,
    tuning: &DedupTuning,
) -> Vec<EntityMergeCandidate> {
    use std::collections::{HashMap, HashSet};

    // HOW: number of lexicographic neighbours examined per entity. A fixed
    // window makes the sorted-name scan O(N·window) while still catching
    // real-world Jaro-Winkler matches, which tend to cluster in sorted order.
    const SORTED_WINDOW: usize = 100;

    let mut candidates = Vec::new();

    // Group indices by entity_type so cross-type pairs are never considered.
    let mut by_type: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, entity) in entities.iter().enumerate() {
        by_type.entry(&entity.entity_type).or_default().push(i);
    }

    for indices in by_type.values() {
        let count = indices.len();
        if count < 2 {
            continue;
        }

        // Token index: each normalized name and alias maps to the entities
        // that carry it. Any bucket with k entries contributes O(k²) pairs,
        // but each pair is materialised only once via the deduplicating set.
        let mut token_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
        for &idx in indices {
            let entity = &entities[idx];
            token_to_indices
                .entry(entity.name.to_lowercase())
                .or_default()
                .push(idx);
            for alias in &entity.aliases {
                token_to_indices
                    .entry(alias.to_lowercase())
                    .or_default()
                    .push(idx);
            }
        }

        // Sorted sliding window over lower-cased names for approximate matches.
        let mut sorted: Vec<(usize, String)> = indices
            .iter()
            .map(|&idx| (idx, entities[idx].name.to_lowercase()))
            .collect();
        sorted.sort_by(|a, b| a.1.cmp(&b.1));

        let mut pair_set: HashSet<(usize, usize)> = HashSet::new();

        for bucket in token_to_indices.values() {
            for i in 0..bucket.len() {
                for &j in &bucket[i + 1..] {
                    let a = bucket[i];
                    let b = j;
                    if a < b {
                        pair_set.insert((a, b));
                    } else {
                        pair_set.insert((b, a));
                    }
                }
            }
        }

        for i in 0..sorted.len() {
            let idx_a = sorted[i].0;
            let window_end = (i + 1 + SORTED_WINDOW).min(sorted.len());
            for &entry in &sorted[i + 1..window_end] {
                let idx_b = entry.0;
                if idx_a < idx_b {
                    pair_set.insert((idx_a, idx_b));
                } else {
                    pair_set.insert((idx_b, idx_a));
                }
            }
        }

        for &(idx_a, idx_b) in &pair_set {
            let a = &entities[idx_a];
            let b = &entities[idx_b];

            let name_sim = jaro_winkler_ci(&a.name, &b.name);
            let alias_overlap = aliases_overlap(&a.aliases, &b.aliases)
                || name_in_aliases(&a.name, &b.aliases, &b.name, &a.aliases);
            let is_exact_match = a.name.to_lowercase() == b.name.to_lowercase();
            let is_jw_match = name_sim >= tuning.jw_threshold;
            let embed_sim = embed_similarities(&a.id, &b.id);
            let is_embed_match = embed_sim >= tuning.embed_threshold;

            if !is_exact_match && !is_jw_match && !is_embed_match && !alias_overlap {
                continue;
            }

            let merge_score =
                compute_merge_score(name_sim, embed_sim, true, alias_overlap, tuning);

            candidates.push(EntityMergeCandidate {
                entity_a: a.id.clone(),
                entity_b: b.id.clone(),
                name_a: a.name.clone(),
                name_b: b.name.clone(),
                name_similarity: name_sim,
                embed_similarity: embed_sim,
                type_match: true,
                alias_overlap,
                merge_score,
            });
        }
    }

    candidates
}

/// Phase 2: Classify candidates into auto-merge, review, or skip under `tuning`.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn classify_candidates(
    candidates: Vec<EntityMergeCandidate>,
    tuning: &DedupTuning,
) -> (Vec<EntityMergeCandidate>, Vec<EntityMergeCandidate>) {
    let mut auto_merge = Vec::new();
    let mut review = Vec::new();

    for c in candidates {
        match MergeDecision::from_score(c.merge_score, tuning) {
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
#[path = "dedup_tests.rs"]
mod dedup_tests;
