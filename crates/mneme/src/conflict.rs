//! Conflict detection pipeline for fact insertion.
//!
//! Before every fact insertion, the pipeline detects whether a new fact
//! contradicts, refines, duplicates, or supplements existing facts — and
//! resolves the conflict automatically where confidence allows.
//!
//! ## 4-Phase Pipeline
//!
//! 1. **Intra-batch dedup** — string-exact or cosine ≥ 0.95 duplicates within the batch
//! 2. **Candidate retrieval** — HNSW cosine + BM25 subject match against existing facts
//! 3. **LLM classification** — classify (new_fact, candidate) pairs
//! 4. **Action resolution** — determine insert/supersede/drop based on classification

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::id::FactId;
use crate::knowledge::EpistemicTier;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the conflict detection pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
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

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a new fact relates to an existing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Phase 1: Intra-batch dedup
// ---------------------------------------------------------------------------

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
            // String-exact check
            if fact.content == existing.content {
                if fact.confidence > existing.confidence {
                    replace_idx = Some(i);
                }
                is_dup = true;
                break;
            }

            // Cosine similarity check
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
            if let Some(idx) = replace_idx {
                kept[idx] = fact;
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

// ---------------------------------------------------------------------------
// Phase 2: Candidate retrieval
// ---------------------------------------------------------------------------

/// Retrieve conflict candidates from the knowledge store for a single fact.
///
/// Uses HNSW vector search with cosine distance ≤ 0.28 (similarity ≥ 0.72),
/// filtered to currently valid facts for the given nous.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::cast_possible_wrap,
    reason = "MAX_CANDIDATES = 5, always fits in i64"
)]
pub(crate) fn retrieve_candidates(
    store: &crate::knowledge_store::KnowledgeStore,
    fact: &FactForConflictCheck,
    nous_id: &str,
) -> Result<Vec<ConflictCandidate>, ConflictError> {
    use crate::engine::DataValue;
    use std::collections::BTreeMap;

    // Use HNSW vector search to find similar facts
    let recall_results = store
        .search_vectors(fact.embedding.clone(), MAX_CANDIDATES as i64, 50)
        .map_err(|e| {
            CandidateRetrievalSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

    if recall_results.is_empty() {
        return Ok(Vec::new());
    }

    // For each vector hit that is a fact, look up the full fact record
    let mut candidates = Vec::new();
    for result in &recall_results {
        if result.source_type != "fact" {
            continue;
        }
        // Distance threshold: cosine distance ≤ 0.28
        if result.distance > CANDIDATE_DISTANCE_THRESHOLD {
            continue;
        }

        // Look up the full fact to check validity and tier
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
            // Check nous_id matches
            let row_nous_id = row.get(5).and_then(|v| v.get_str()).unwrap_or("");
            if row_nous_id != nous_id {
                continue;
            }

            // Check valid_to is far future (fact is currently valid)
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

            candidates.push(ConflictCandidate {
                existing_fact_id: FactId::from(fact_id.clone()),
                existing_content: content,
                existing_confidence: confidence,
                existing_tier: tier,
                cosine_similarity: 1.0 - result.distance,
            });
        }
    }

    // Cap at MAX_CANDIDATES
    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Phase 3: LLM classification
// ---------------------------------------------------------------------------

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

    // Sort by cosine similarity (highest first), take up to MAX_LLM_CALLS_PER_FACT
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
            // Return the first non-Unrelated classification, or the last one
            if classification != ConflictClassification::Unrelated {
                return Ok(Some((classification, idx)));
            }
        }
    }

    // All classified as Unrelated (or unparseable)
    Ok(Some((ConflictClassification::Unrelated, 0)))
}

// ---------------------------------------------------------------------------
// Phase 4: Action resolution
// ---------------------------------------------------------------------------

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
            // Tier protection: Verified never superseded by Assumed
            if candidate.existing_tier == EpistemicTier::Verified
                && new_fact.tier == EpistemicTier::Assumed
            {
                return ConflictAction::Drop;
            }

            // Correction facts always win contradictions
            if new_fact.is_correction {
                return ConflictAction::Supersede {
                    old_id: candidate.existing_fact_id.clone(),
                };
            }

            // Higher confidence wins; ties go to the newer fact
            if new_fact.confidence >= candidate.existing_confidence {
                ConflictAction::Supersede {
                    old_id: candidate.existing_fact_id.clone(),
                }
            } else {
                ConflictAction::Drop
            }
        }
        ConflictClassification::Refines => {
            // Tier protection: Verified never superseded by Assumed
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

// ---------------------------------------------------------------------------
// Full pipeline
// ---------------------------------------------------------------------------

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
    // Phase 1: Intra-batch dedup
    let (deduped, batch_duplicates_dropped) = intra_batch_dedup(facts);

    // Phases 2–4: per-fact candidate retrieval + classification + resolution
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

// ---------------------------------------------------------------------------
// Correction detection
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fact(content: &str, confidence: f64, embedding: Vec<f32>) -> FactForConflictCheck {
        FactForConflictCheck {
            content: content.to_owned(),
            confidence,
            tier: EpistemicTier::Inferred,
            subject: "alice".to_owned(),
            is_correction: false,
            embedding,
        }
    }

    fn make_fact_with_tier(
        content: &str,
        confidence: f64,
        tier: EpistemicTier,
        embedding: Vec<f32>,
    ) -> FactForConflictCheck {
        FactForConflictCheck {
            content: content.to_owned(),
            confidence,
            tier,
            subject: "alice".to_owned(),
            is_correction: false,
            embedding,
        }
    }

    fn make_candidate(
        id: &str,
        content: &str,
        confidence: f64,
        tier: EpistemicTier,
        similarity: f64,
    ) -> ConflictCandidate {
        ConflictCandidate {
            existing_fact_id: FactId::from(id),
            existing_content: content.to_owned(),
            existing_confidence: confidence,
            existing_tier: tier,
            cosine_similarity: similarity,
        }
    }

    // --- Classification parsing ---

    #[test]
    fn parse_classification_contradicts() {
        assert_eq!(
            ConflictClassification::parse("CONTRADICTS"),
            Some(ConflictClassification::Contradicts)
        );
        assert_eq!(
            ConflictClassification::parse("  contradicts  "),
            Some(ConflictClassification::Contradicts)
        );
    }

    #[test]
    fn parse_classification_refines() {
        assert_eq!(
            ConflictClassification::parse("REFINES"),
            Some(ConflictClassification::Refines)
        );
    }

    #[test]
    fn parse_classification_supplements() {
        assert_eq!(
            ConflictClassification::parse("SUPPLEMENTS"),
            Some(ConflictClassification::Supplements)
        );
    }

    #[test]
    fn parse_classification_unrelated() {
        assert_eq!(
            ConflictClassification::parse("UNRELATED"),
            Some(ConflictClassification::Unrelated)
        );
    }

    #[test]
    fn parse_classification_invalid() {
        assert_eq!(ConflictClassification::parse("UNKNOWN_TYPE"), None);
        assert_eq!(ConflictClassification::parse(""), None);
    }

    // --- Cosine similarity ---

    #[test]
    fn cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "identical vectors should have sim ~1.0"
        );
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6, "orthogonal vectors should have sim ~0.0");
    }

    #[test]
    fn cosine_similarity_empty() {
        let sim = cosine_similarity(&[], &[]);
        assert!((sim - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cosine_similarity_different_lengths() {
        let sim = cosine_similarity(&[1.0], &[1.0, 2.0]);
        assert!((sim - 0.0).abs() < f64::EPSILON);
    }

    // --- Intra-batch dedup ---

    #[test]
    fn intra_batch_dedup_exact_string_match() {
        let facts = vec![
            make_fact("alice works at acme", 0.8, vec![1.0, 0.0, 0.0]),
            make_fact("alice works at acme", 0.9, vec![1.0, 0.0, 0.0]),
        ];
        let (kept, dropped) = intra_batch_dedup(facts);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 1);
        assert!(
            (kept[0].confidence - 0.9).abs() < f64::EPSILON,
            "highest confidence wins"
        );
    }

    #[test]
    fn intra_batch_dedup_cosine_similar() {
        // Nearly identical embeddings (sim > 0.95)
        let facts = vec![
            make_fact("alice works at acme corp", 0.7, vec![1.0, 0.0, 0.01]),
            make_fact("alice is employed at acme", 0.85, vec![1.0, 0.0, 0.0]),
        ];
        let (kept, dropped) = intra_batch_dedup(facts);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 1);
        assert!(
            (kept[0].confidence - 0.85).abs() < f64::EPSILON,
            "highest confidence wins"
        );
    }

    #[test]
    fn intra_batch_dedup_different_facts_preserved() {
        let facts = vec![
            make_fact("alice works at acme", 0.8, vec![1.0, 0.0, 0.0]),
            make_fact("bob lives in london", 0.9, vec![0.0, 1.0, 0.0]),
        ];
        let (kept, dropped) = intra_batch_dedup(facts);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn intra_batch_dedup_empty() {
        let (kept, dropped) = intra_batch_dedup(vec![]);
        assert!(kept.is_empty());
        assert_eq!(dropped, 0);
    }

    #[test]
    fn intra_batch_dedup_single() {
        let facts = vec![make_fact("sole fact", 0.5, vec![1.0])];
        let (kept, dropped) = intra_batch_dedup(facts);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 0);
    }

    // --- Action resolution ---

    #[test]
    fn resolve_contradicts_new_higher_confidence() {
        let candidate = make_candidate("f-old", "old claim", 0.7, EpistemicTier::Inferred, 0.9);
        let fact = make_fact("new claim", 0.9, vec![]);
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    #[test]
    fn resolve_contradicts_new_lower_confidence() {
        let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
        let fact = make_fact("new claim", 0.5, vec![]);
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(action, ConflictAction::Drop);
    }

    #[test]
    fn resolve_contradicts_equal_confidence_new_wins() {
        let candidate = make_candidate("f-old", "old claim", 0.8, EpistemicTier::Inferred, 0.9);
        let fact = make_fact("new claim", 0.8, vec![]);
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    #[test]
    fn resolve_refines_supersedes() {
        let candidate =
            make_candidate("f-old", "general claim", 0.8, EpistemicTier::Inferred, 0.85);
        let fact = make_fact("specific claim", 0.9, vec![]);
        let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    #[test]
    fn resolve_supplements_inserts() {
        let candidate =
            make_candidate("f-old", "existing claim", 0.8, EpistemicTier::Inferred, 0.8);
        let fact = make_fact("additional info", 0.7, vec![]);
        let action = resolve_action(&ConflictClassification::Supplements, &candidate, &fact);
        assert_eq!(action, ConflictAction::Insert);
    }

    #[test]
    fn resolve_unrelated_inserts() {
        let candidate = make_candidate(
            "f-old",
            "existing claim",
            0.8,
            EpistemicTier::Inferred,
            0.75,
        );
        let fact = make_fact("different topic", 0.9, vec![]);
        let action = resolve_action(&ConflictClassification::Unrelated, &candidate, &fact);
        assert_eq!(action, ConflictAction::Insert);
    }

    // --- Tier protection ---

    #[test]
    fn verified_not_superseded_by_assumed_contradicts() {
        let candidate = make_candidate(
            "f-verified",
            "verified fact",
            0.7,
            EpistemicTier::Verified,
            0.9,
        );
        let fact = make_fact_with_tier(
            "assumed contradiction",
            0.95,
            EpistemicTier::Assumed,
            vec![],
        );
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(action, ConflictAction::Drop);
    }

    #[test]
    fn verified_not_superseded_by_assumed_refines() {
        let candidate = make_candidate(
            "f-verified",
            "verified fact",
            0.7,
            EpistemicTier::Verified,
            0.9,
        );
        let fact = make_fact_with_tier("assumed refinement", 0.95, EpistemicTier::Assumed, vec![]);
        let action = resolve_action(&ConflictClassification::Refines, &candidate, &fact);
        assert_eq!(action, ConflictAction::Drop);
    }

    #[test]
    fn verified_can_be_superseded_by_verified() {
        let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
        let fact = make_fact_with_tier("new verified", 0.95, EpistemicTier::Verified, vec![]);
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    #[test]
    fn verified_can_be_superseded_by_inferred() {
        let candidate = make_candidate("f-old", "old verified", 0.7, EpistemicTier::Verified, 0.9);
        let fact = make_fact_with_tier("new inferred", 0.95, EpistemicTier::Inferred, vec![]);
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    // --- Correction detection ---

    #[test]
    fn correction_heuristic_detects_patterns() {
        assert!(is_correction_heuristic("Actually, it's 42 not 43"));
        assert!(is_correction_heuristic("I was wrong about the date"));
        assert!(is_correction_heuristic("Correction: the value is 100"));
        assert!(is_correction_heuristic("I was mistaken about that"));
        assert!(is_correction_heuristic(
            "that's incorrect, the real answer is X"
        ));
    }

    #[test]
    fn correction_heuristic_rejects_normal() {
        assert!(!is_correction_heuristic("alice works at acme corp"));
        assert!(!is_correction_heuristic("the project uses rust"));
        assert!(!is_correction_heuristic(""));
    }

    #[test]
    fn correction_boost_adds_02() {
        assert!((apply_correction_boost(0.5) - 0.7).abs() < f64::EPSILON);
        assert!((apply_correction_boost(0.8) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn correction_boost_caps_at_1() {
        assert!((apply_correction_boost(0.9) - 1.0).abs() < f64::EPSILON);
        assert!((apply_correction_boost(1.0) - 1.0).abs() < f64::EPSILON);
    }

    // --- Correction in conflict resolution ---

    #[test]
    fn correction_fact_wins_contradiction_regardless_of_confidence() {
        let candidate = make_candidate("f-old", "old claim", 0.95, EpistemicTier::Inferred, 0.9);
        let mut fact = make_fact("corrected claim", 0.5, vec![]);
        fact.is_correction = true;
        let action = resolve_action(&ConflictClassification::Contradicts, &candidate, &fact);
        assert_eq!(
            action,
            ConflictAction::Supersede {
                old_id: FactId::from("f-old")
            }
        );
    }

    // --- Mock classifier for integration tests ---

    struct MockClassifier {
        response: String,
    }

    impl ConflictClassifier for MockClassifier {
        fn classify(
            &self,
            _existing_content: &str,
            _existing_confidence: f64,
            _existing_tier: &str,
            _new_content: &str,
            _new_confidence: f64,
            _new_tier: &str,
        ) -> Result<String, ConflictError> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn classify_no_candidates_returns_none() {
        let classifier = MockClassifier {
            response: "CONTRADICTS".to_owned(),
        };
        let fact = make_fact("test", 0.8, vec![1.0]);
        let result = classify_against_candidates(&classifier, &fact, &[]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn classify_returns_classification() {
        let classifier = MockClassifier {
            response: "REFINES".to_owned(),
        };
        let fact = make_fact("test", 0.8, vec![1.0]);
        let candidates = vec![make_candidate(
            "f-1",
            "existing",
            0.7,
            EpistemicTier::Inferred,
            0.85,
        )];
        let result = classify_against_candidates(&classifier, &fact, &candidates).unwrap();
        assert!(result.is_some());
        let (classification, idx) = result.unwrap();
        assert_eq!(classification, ConflictClassification::Refines);
        assert_eq!(idx, 0);
    }

    #[test]
    fn classify_each_type_produces_correct_action() {
        for (response, expected_action) in [
            (
                "CONTRADICTS",
                ConflictAction::Supersede {
                    old_id: FactId::from("f-1"),
                },
            ),
            (
                "REFINES",
                ConflictAction::Supersede {
                    old_id: FactId::from("f-1"),
                },
            ),
            ("SUPPLEMENTS", ConflictAction::Insert),
            ("UNRELATED", ConflictAction::Insert),
        ] {
            let classifier = MockClassifier {
                response: response.to_owned(),
            };
            let fact = make_fact("new fact", 0.9, vec![1.0]);
            let candidates = vec![make_candidate(
                "f-1",
                "existing",
                0.7,
                EpistemicTier::Inferred,
                0.85,
            )];
            let (classification, idx) =
                classify_against_candidates(&classifier, &fact, &candidates)
                    .unwrap()
                    .unwrap();
            let action = resolve_action(&classification, &candidates[idx], &fact);
            assert_eq!(action, expected_action, "failed for {response}");
        }
    }

    #[test]
    fn no_candidates_results_in_insert() {
        // Simulates Phase 2 returning empty candidates → straight insert
        let fact = make_fact("brand new fact with no matches", 0.8, vec![1.0, 0.0]);
        let candidates: Vec<ConflictCandidate> = vec![];
        let result = classify_against_candidates(
            &MockClassifier {
                response: "CONTRADICTS".to_owned(),
            },
            &fact,
            &candidates,
        )
        .unwrap();
        assert!(
            result.is_none(),
            "no candidates should return None (insert)"
        );
    }

    // --- Build classification prompt ---

    #[test]
    fn classification_prompt_contains_facts() {
        let (system, user) = build_classification_prompt(
            "alice works at acme",
            0.8,
            "inferred",
            "alice works at globex",
            0.9,
            "inferred",
        );
        assert!(system.contains("CONTRADICTS"));
        assert!(system.contains("REFINES"));
        assert!(system.contains("SUPPLEMENTS"));
        assert!(system.contains("UNRELATED"));
        assert!(user.contains("alice works at acme"));
        assert!(user.contains("alice works at globex"));
        assert!(user.contains("0.80"));
        assert!(user.contains("0.90"));
    }

    // --- ConflictAction equality ---

    #[test]
    fn conflict_action_equality() {
        assert_eq!(ConflictAction::Insert, ConflictAction::Insert);
        assert_eq!(ConflictAction::Drop, ConflictAction::Drop);
        assert_eq!(
            ConflictAction::Supersede {
                old_id: FactId::from("a")
            },
            ConflictAction::Supersede {
                old_id: FactId::from("a")
            }
        );
        assert_ne!(ConflictAction::Insert, ConflictAction::Drop);
    }

    // --- Serde roundtrip ---

    #[test]
    fn classification_serde_roundtrip() {
        for class in [
            ConflictClassification::Contradicts,
            ConflictClassification::Refines,
            ConflictClassification::Supplements,
            ConflictClassification::Unrelated,
        ] {
            let json = serde_json::to_string(&class).unwrap();
            let back: ConflictClassification = serde_json::from_str(&json).unwrap();
            assert_eq!(class, back);
        }
    }
}
