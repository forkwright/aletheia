//! Conflict detection pipeline for fact insertion.
//!
//! Before every fact insertion, the pipeline detects whether a new fact
//! contradicts, refines, duplicates, or supplements existing facts: and
//! resolves the conflict automatically where confidence allows.
//!
//! ## 4-Phase Pipeline
//!
//! 1. **Intra-batch dedup**: string-exact or cosine ≥ 0.95 duplicates within the batch
//! 2. **Candidate retrieval**: HNSW cosine + BM25 subject match against existing facts
//! 3. **LLM classification**: classify (new_fact, candidate) pairs
//! 4. **Action resolution**: determine insert/supersede/drop based on classification
#![cfg_attr(
    any(feature = "mneme-engine", test),
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]
#![cfg_attr(
    feature = "mneme-engine",
    expect(
        clippy::as_conversions,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::id::FactId;
use crate::knowledge::EpistemicTier;

/// Errors from the conflict detection pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location) are self-documenting via display format"
)]
pub enum ConflictError {
    /// The LLM classification call failed.
    #[snafu(display("conflict classification failed: {message}"))]
    Classification {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge store query failed during candidate retrieval.
    #[snafu(display("candidate retrieval failed: {message}"))]
    CandidateRetrieval {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// How a new fact relates to an existing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConflictClassification {
    /// The new fact directly contradicts the existing fact.
    Contradicts,
    /// The new fact is a more specific/updated version.
    Refines,
    /// The new fact adds information without contradicting.
    Supplements,
    /// Despite textual similarity, these are about different things.
    Unrelated,
}

impl ConflictClassification {
    /// Parse an LLM response string into a classification.
    #[cfg(any(feature = "mneme-engine", test))]
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim().to_uppercase();
        if trimmed.starts_with("CONTRADICTS") {
            Some(Self::Contradicts)
        } else if trimmed.starts_with("REFINES") {
            Some(Self::Refines)
        } else if trimmed.starts_with("SUPPLEMENTS") {
            Some(Self::Supplements)
        } else if trimmed.starts_with("UNRELATED") {
            Some(Self::Unrelated)
        } else {
            None
        }
    }
}

/// An existing fact that may conflict with a new fact.
#[derive(Debug, Clone)]
pub struct ConflictCandidate {
    /// ID of the existing fact.
    pub existing_fact_id: FactId,
    /// Content of the existing fact.
    pub existing_content: String,
    /// Confidence of the existing fact.
    pub existing_confidence: f64,
    /// Epistemic tier of the existing fact.
    pub existing_tier: EpistemicTier,
    /// Cosine similarity between the new and existing fact.
    pub cosine_similarity: f64,
}

/// The resolved action for a new fact after conflict detection.
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    /// What to do with the new fact.
    pub action: ConflictAction,
    /// How the new fact relates to the best-matching candidate.
    pub classification: ConflictClassification,
    /// If superseding, the ID of the fact being replaced.
    pub superseded_fact_id: Option<FactId>,
}

/// Action to take after conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictAction {
    /// Insert the new fact (no conflict or supplements existing).
    Insert,
    /// Supersede the old fact with the new one.
    Supersede {
        /// ID of the fact being superseded.
        old_id: FactId,
    },
    /// Drop the new fact (duplicate or lower quality).
    Drop,
}

/// Minimal LLM interface for conflict classification.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the configured LLM provider.
pub trait ConflictClassifier: Send + Sync {
    /// Classify the relationship between an existing fact and a new fact.
    ///
    /// Returns a raw string that will be parsed into a [`ConflictClassification`].
    fn classify(
        &self,
        existing_content: &str,
        existing_confidence: f64,
        existing_tier: &str,
        new_content: &str,
        new_confidence: f64,
        new_tier: &str,
    ) -> Result<String, ConflictError>;
}

/// A fact prepared for conflict detection with its embedding.
#[cfg(any(feature = "mneme-engine", test))]
#[derive(Debug, Clone)]
pub struct FactForConflictCheck {
    /// The content of the fact (subject + predicate + object).
    pub content: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// Epistemic tier.
    pub tier: EpistemicTier,
    /// Subject entity name (for BM25 matching).
    pub subject: String,
    /// Whether this is a correction fact.
    pub is_correction: bool,
    /// Pre-computed embedding vector for similarity search.
    pub embedding: Vec<f32>,
}

/// Result of running the full conflict detection pipeline on a batch.
#[cfg(any(feature = "mneme-engine", test))]
#[derive(Debug, Clone)]
pub struct BatchConflictResult {
    /// Facts that should be inserted (after dedup), with their resolved actions.
    pub resolved: Vec<(FactForConflictCheck, ConflictAction)>,
    /// Number of facts dropped during intra-batch dedup.
    pub batch_duplicates_dropped: usize,
}

/// Maximum number of LLM classification calls per fact.
#[cfg(any(feature = "mneme-engine", test))]
const MAX_LLM_CALLS_PER_FACT: usize = 3;

/// Cosine similarity threshold for intra-batch dedup.
#[cfg(any(feature = "mneme-engine", test))]
const INTRA_BATCH_DEDUP_THRESHOLD: f64 = 0.95;

/// Cosine similarity threshold for candidate retrieval.
///
/// HNSW returns cosine distance (1 - similarity), so we convert:
/// similarity ≥ 0.72 means distance ≤ 0.28.
#[cfg(feature = "mneme-engine")]
const CANDIDATE_DISTANCE_THRESHOLD: f64 = 0.28;

/// Maximum candidates to consider per fact.
#[cfg(feature = "mneme-engine")]
const MAX_CANDIDATES: usize = 5;

/// Remove duplicates within an extraction batch.
///
/// Two facts are considered duplicates if they have string-exact content
/// or cosine similarity ≥ 0.95. The highest-confidence fact is kept.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn intra_batch_dedup(
    facts: Vec<FactForConflictCheck>,
) -> (Vec<FactForConflictCheck>, usize) {
    if facts.len() <= 1 {
        return (facts, 0);
    }

    let mut kept: Vec<FactForConflictCheck> = Vec::with_capacity(facts.len());
    let mut dropped = 0usize;

    for fact in facts {
        let mut is_dup = false;
        let mut replace_idx = None;

        for (i, existing) in kept.iter().enumerate() {
            if fact.content == existing.content {
                if fact.confidence > existing.confidence {
                    replace_idx = Some(i);
                }
                is_dup = true;
                break;
            }
            let sim = cosine_similarity(&fact.embedding, &existing.embedding);
            if sim >= INTRA_BATCH_DEDUP_THRESHOLD {
                if fact.confidence > existing.confidence {
                    replace_idx = Some(i);
                }
                is_dup = true;
                break;
            }
        }

        if is_dup {
            if let Some(idx) = replace_idx
                && let Some(slot) = kept.get_mut(idx)
            {
                *slot = fact;
            }
            dropped += 1;
        } else {
            kept.push(fact);
        }
    }

    (kept, dropped)
}

/// Compute cosine similarity between two vectors.
#[cfg(any(feature = "mneme-engine", test))]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f64::EPSILON {
        return 0.0;
    }
    dot / denom
}

/// Retrieve conflict candidates from the knowledge store for a single fact.
///
/// Uses HNSW vector search with cosine distance ≤ 0.28 (similarity ≥ 0.72),
/// filtered to currently valid facts for the given nous.
#[cfg(feature = "mneme-engine")]
pub(crate) fn retrieve_candidates(
    store: &crate::knowledge_store::KnowledgeStore,
    fact: &FactForConflictCheck,
    nous_id: &str,
) -> Result<Vec<ConflictCandidate>, ConflictError> {
    use std::collections::BTreeMap;

    use crate::engine::DataValue;

    let max_candidates = i64::try_from(MAX_CANDIDATES).unwrap_or(i64::from(i32::MAX));
    let recall_results = store
        .search_vectors(fact.embedding.clone(), max_candidates, 50)
        .map_err(|e| {
            CandidateRetrievalSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

    if recall_results.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for result in &recall_results {
        if result.source_type != "fact" {
            continue;
        }
        if result.distance > CANDIDATE_DISTANCE_THRESHOLD {
            continue;
        }
        let fact_id = &result.source_id;
        let script = r"
            ?[id, content, confidence, tier, valid_to, nous_id] :=
                *facts{id, content, confidence, tier, valid_to, nous_id, valid_from},
                id == $fact_id
        ";
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_str().into()),
        );

        let query_result = store.run_query(script, params).map_err(|e| {
            CandidateRetrievalSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        for row in &query_result.rows {
            let row_nous_id = row.get(5).and_then(|v| v.get_str()).unwrap_or("");
            if row_nous_id != nous_id {
                continue;
            }
            let valid_to = row.get(4).and_then(|v| v.get_str()).unwrap_or("");
            if !valid_to.is_empty() && !valid_to.starts_with("9999-") {
                continue;
            }

            let content = row
                .get(1)
                .and_then(|v| v.get_str())
                .unwrap_or("")
                .to_owned();
            let confidence = row.get(2).and_then(DataValue::get_float).unwrap_or(0.0);
            let tier_str = row.get(3).and_then(|v| v.get_str()).unwrap_or("assumed");
            let tier = match tier_str {
                "verified" => EpistemicTier::Verified,
                "inferred" => EpistemicTier::Inferred,
                _ => EpistemicTier::Assumed,
            };

            let existing_fact_id = FactId::new(fact_id.clone()).map_err(|e| {
                CandidateRetrievalSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            candidates.push(ConflictCandidate {
                existing_fact_id,
                existing_content: content,
                existing_confidence: confidence,
                existing_tier: tier,
                cosine_similarity: 1.0 - result.distance,
            });
        }
    }

    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

/// Build the classification prompt for the LLM.
#[cfg(test)]
fn build_classification_prompt(
    existing_content: &str,
    existing_confidence: f64,
    existing_tier: &str,
    new_content: &str,
    new_confidence: f64,
    new_tier: &str,
) -> (String, String) {
    let system = "You are a fact consistency checker. Given an existing fact and a new fact, \
                  classify their relationship.\n\n\
                  Respond with exactly one of:\n\
                  - CONTRADICTS: The new fact directly contradicts the existing fact\n\
                  - REFINES: The new fact is a more specific/updated version\n\
                  - SUPPLEMENTS: The new fact adds information without contradicting\n\
                  - UNRELATED: Despite textual similarity, these are about different things"
        .to_owned();

    let user_message = format!(
        "Existing: \"{existing_content}\" (confidence: {existing_confidence:.2}, tier: {existing_tier})\n\
         New: \"{new_content}\" (confidence: {new_confidence:.2}, tier: {new_tier})\n\n\
         Classification:"
    );

    (system, user_message)
}

/// Classify new fact against candidates using the LLM provider.
///
/// Returns the classification for the highest-similarity candidate,
/// or `None` if there are no candidates.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn classify_against_candidates(
    classifier: &dyn ConflictClassifier,
    fact: &FactForConflictCheck,
    candidates: &[ConflictCandidate],
) -> Result<Option<(ConflictClassification, usize)>, ConflictError> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut sorted_candidates: Vec<(usize, &ConflictCandidate)> =
        candidates.iter().enumerate().collect();
    sorted_candidates.sort_by(|a, b| {
        b.1.cosine_similarity
            .partial_cmp(&a.1.cosine_similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for &(idx, candidate) in sorted_candidates.iter().take(MAX_LLM_CALLS_PER_FACT) {
        let response = classifier.classify(
            &candidate.existing_content,
            candidate.existing_confidence,
            candidate.existing_tier.as_str(),
            &fact.content,
            fact.confidence,
            fact.tier.as_str(),
        )?;

        if let Some(classification) = ConflictClassification::parse(&response) {
            // WHY: Return first non-Unrelated classification; Unrelated is weakest signal.
            if classification != ConflictClassification::Unrelated {
                return Ok(Some((classification, idx)));
            }
        }
    }

    Ok(Some((ConflictClassification::Unrelated, 0)))
}

/// Determine the action based on classification, confidence, and tier.
///
/// Rules:
/// - CONTRADICTS → supersede (lower confidence loses; newer info wins ties)
/// - REFINES → supersede (refinement always wins)
/// - SUPPLEMENTS → insert (both live)
/// - UNRELATED → insert
/// - Exception: `Verified` tier never superseded by `Assumed` tier
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn resolve_action(
    classification: &ConflictClassification,
    candidate: &ConflictCandidate,
    new_fact: &FactForConflictCheck,
) -> ConflictAction {
    match classification {
        ConflictClassification::Contradicts => {
            // WHY: Verified facts are never superseded by Assumed-tier facts.
            if candidate.existing_tier == EpistemicTier::Verified
                && new_fact.tier == EpistemicTier::Assumed
            {
                return ConflictAction::Drop;
            }
            if new_fact.is_correction {
                return ConflictAction::Supersede {
                    old_id: candidate.existing_fact_id.clone(),
                };
            }
            if new_fact.confidence >= candidate.existing_confidence {
                ConflictAction::Supersede {
                    old_id: candidate.existing_fact_id.clone(),
                }
            } else {
                ConflictAction::Drop
            }
        }
        ConflictClassification::Refines => {
            // WHY: Verified facts are never superseded by Assumed-tier facts.
            if candidate.existing_tier == EpistemicTier::Verified
                && new_fact.tier == EpistemicTier::Assumed
            {
                return ConflictAction::Drop;
            }
            ConflictAction::Supersede {
                old_id: candidate.existing_fact_id.clone(),
            }
        }
        ConflictClassification::Supplements | ConflictClassification::Unrelated => {
            ConflictAction::Insert
        }
    }
}

/// Run the full 4-phase conflict detection pipeline on a batch of facts.
///
/// Phase 1: Intra-batch dedup (no DB, no LLM)
/// Phase 2: Candidate retrieval (DB, no LLM)
/// Phase 3: LLM classification
/// Phase 4: Action resolution
#[cfg(feature = "mneme-engine")]
pub fn detect_conflicts(
    facts: Vec<FactForConflictCheck>,
    store: &crate::knowledge_store::KnowledgeStore,
    nous_id: &str,
    classifier: &dyn ConflictClassifier,
) -> Result<BatchConflictResult, ConflictError> {
    let (deduped, batch_duplicates_dropped) = intra_batch_dedup(facts);
    let mut resolved = Vec::with_capacity(deduped.len());
    for fact in deduped {
        let candidates = retrieve_candidates(store, &fact, nous_id)?;

        let action = if candidates.is_empty() {
            ConflictAction::Insert
        } else {
            match classify_against_candidates(classifier, &fact, &candidates)? {
                Some((classification, candidate_idx)) => {
                    resolve_action(&classification, &candidates[candidate_idx], &fact)
                }
                None => ConflictAction::Insert,
            }
        };

        resolved.push((fact, action));
    }

    Ok(BatchConflictResult {
        resolved,
        batch_duplicates_dropped,
    })
}

/// Patterns that indicate a correction fact.
#[cfg(any(feature = "mneme-engine", test))]
const CORRECTION_PATTERNS: &[&str] = &[
    "actually, it's",
    "actually it's",
    "i was wrong about",
    "correction:",
    "actually, the",
    "actually the",
    "not actually",
    "i was mistaken",
    "that's incorrect",
    "that was incorrect",
    "i need to correct",
    "let me correct",
];

/// Check if a fact's content indicates it is a correction of prior information.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn is_correction_heuristic(content: &str) -> bool {
    let lower = content.to_lowercase();
    CORRECTION_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Apply correction confidence boost: +0.2, capped at 1.0.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn apply_correction_boost(confidence: f64) -> f64 {
    (confidence + 0.2).min(1.0)
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod tests;
