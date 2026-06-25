//! Explainable recall scoring for fact search.
//!
//! Builds [`ScoredResult`] candidates from a raw fact stream so that HTTP
//! search endpoints can share the same ranking path as the turn recall
//! pipeline and expose why each candidate was selected, dropped, or filtered.

use crate::knowledge::{Fact, FactType, Visibility};

use super::{FactorScores, RecallEngine, RecallWeights, ScoredResult};

/// Decision applied to a candidate during the explain path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CandidateDecision {
    /// Included in the returned result set.
    Selected,
    /// Removed because it did not meet a hard gate.
    Dropped,
    /// Removed by a policy filter such as forgetting or visibility.
    Filtered,
}

/// A recall candidate together with the human-readable decision that produced it.
#[derive(Debug, Clone)]
pub struct ExplainedCandidate {
    /// Underlying recall candidate.
    pub result: ScoredResult,
    /// Whether the candidate was selected, dropped, or filtered.
    pub decision: CandidateDecision,
    /// Short machine-readable reasons for the decision.
    pub reasons: Vec<String>,
    /// Source confidence carried from the fact provenance.
    pub confidence: f64,
    /// Source epistemic tier carried from the fact provenance.
    pub tier: String,
    /// Source fact type carried from the fact.
    pub fact_type: String,
}

impl ExplainedCandidate {
    /// Whether this candidate was included in the returned result set.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.decision == CandidateDecision::Selected
    }
}

/// Full explanation of a recall scoring pass over a set of facts.
#[derive(Debug, Clone)]
pub struct RecallExplanation {
    /// Weights used to compute the composite score.
    pub weights: RecallWeights,
    /// Sum of the active weights (the normalization denominator).
    pub total_weight: f64,
    /// Every candidate seen, including those that were filtered or dropped.
    pub candidates: Vec<ExplainedCandidate>,
}

impl RecallExplanation {
    /// Build a recall explanation, computing `total_weight` from the provided weights.
    pub fn new(weights: RecallWeights, candidates: Vec<ExplainedCandidate>) -> Self {
        let total_weight = weights.total();
        Self {
            weights,
            total_weight,
            candidates,
        }
    }

    /// Number of candidates that were included in the result set.
    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.is_selected()).count()
    }
}

/// Score and explain a fact stream for a free-text query.
///
/// This is a pure function: it does not read the knowledge store or the
/// embedding index. Vector similarity is approximated from term overlap so
/// the public search endpoint can reuse the same multi-factor ranking path
/// even when vector search is not available.
#[must_use]
pub fn explain_recall(
    engine: &RecallEngine,
    facts: &[Fact],
    query: &str,
    query_nous_id: Option<&str>,
    now: jiff::Timestamp,
    limit: usize,
) -> RecallExplanation {
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();
    let query_nous = query_nous_id.unwrap_or("");

    let mut explained: Vec<ExplainedCandidate> = Vec::with_capacity(facts.len());
    let mut selected: Vec<ScoredResult> = Vec::new();
    let mut meta_by_id: std::collections::HashMap<String, (f64, String, String)> =
        std::collections::HashMap::new();

    for fact in facts {
        let (confidence, tier, fact_type) = candidate_meta(fact);

        if fact.lifecycle.is_forgotten {
            explained.push(ExplainedCandidate {
                result: unscored_result(fact),
                decision: CandidateDecision::Filtered,
                reasons: vec!["forgotten".to_owned()],
                confidence,
                tier,
                fact_type,
            });
            continue;
        }

        let vector_similarity = term_match_score(&fact.content, &terms);
        if vector_similarity == 0.0 {
            explained.push(ExplainedCandidate {
                result: unscored_result(fact),
                decision: CandidateDecision::Dropped,
                reasons: vec!["no_term_match".to_owned()],
                confidence,
                tier,
                fact_type,
            });
            continue;
        }

        if let Some(query_nous) = query_nous_id {
            let blocked_by_visibility =
                !matches!(fact.visibility, Visibility::Shared | Visibility::Published)
                    && fact.nous_id != query_nous;
            if blocked_by_visibility {
                explained.push(ExplainedCandidate {
                    result: unscored_result(fact),
                    decision: CandidateDecision::Filtered,
                    reasons: vec!["visibility".to_owned()],
                    confidence,
                    tier,
                    fact_type,
                });
                continue;
            }
        }

        let factors = build_factors(engine, fact, vector_similarity, query_nous, now);
        let source_id = fact.id.to_string();
        meta_by_id.insert(source_id.clone(), (confidence, tier, fact_type));
        selected.push(ScoredResult {
            content: fact.content.clone(),
            source_type: "fact".to_owned(),
            source_id,
            nous_id: fact.nous_id.clone(),
            factors,
            score: 0.0,
            sensitivity: fact.sensitivity,
            visibility: fact.visibility,
            scope: fact.scope,
            project_id: fact.project_id.clone(),
        });
    }

    let ranked = engine.rank(selected);
    let mut returned = 0usize;
    for result in ranked {
        let (decision, reasons) = if returned < limit {
            returned += 1;
            (CandidateDecision::Selected, vec!["selected".to_owned()])
        } else {
            (
                CandidateDecision::Dropped,
                vec!["limit_truncation".to_owned()],
            )
        };
        let (confidence, tier, fact_type) = meta_by_id.get(&result.source_id).cloned().unwrap_or((
            0.0,
            String::new(),
            String::new(),
        ));
        explained.push(ExplainedCandidate {
            result,
            decision,
            reasons,
            confidence,
            tier,
            fact_type,
        });
    }

    RecallExplanation {
        weights: engine.weights().clone(),
        total_weight: engine.weights().total(),
        candidates: explained,
    }
}

fn candidate_meta(fact: &Fact) -> (f64, String, String) {
    (
        fact.provenance.confidence,
        fact.provenance.tier.as_str().to_owned(),
        fact.fact_type.clone(),
    )
}

fn unscored_result(fact: &Fact) -> ScoredResult {
    ScoredResult {
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: fact.id.to_string(),
        nous_id: fact.nous_id.clone(),
        factors: FactorScores::default(),
        score: 0.0,
        sensitivity: fact.sensitivity,
        visibility: fact.visibility,
        scope: fact.scope,
        project_id: fact.project_id.clone(),
    }
}

fn term_match_score(content: &str, terms: &[&str]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let content_lower = content.to_lowercase();
    let hits = terms
        .iter()
        .filter(|term| content_lower.contains(**term))
        .count();
    usize_to_f64(hits) / usize_to_f64(terms.len())
}

fn usize_to_f64(value: usize) -> f64 {
    u32::try_from(value).map_or(f64::from(u32::MAX), f64::from)
}

fn i64_to_f64_saturating(value: i64) -> f64 {
    i32::try_from(value).map_or_else(
        |_| {
            if value.is_negative() {
                f64::from(i32::MIN)
            } else {
                f64::from(i32::MAX)
            }
        },
        f64::from,
    )
}

fn build_factors(
    engine: &RecallEngine,
    fact: &Fact,
    vector_similarity: f64,
    query_nous_id: &str,
    now: jiff::Timestamp,
) -> FactorScores {
    let reference_time = fact
        .access
        .last_accessed_at
        .unwrap_or(fact.temporal.recorded_at);
    let age_hours = now.since(reference_time).map_or(0.0, |span| {
        i64_to_f64_saturating(i64::from(span.get_hours()))
            + i64_to_f64_saturating(span.get_minutes()) / 60.0
    });

    let fact_type = FactType::from_str_lossy(&fact.fact_type);
    let decay = engine.score_decay(
        age_hours,
        fact_type,
        fact.provenance.tier,
        fact.access.access_count,
    );
    let relevance = engine.score_relevance(&fact.nous_id, query_nous_id);
    let epistemic_tier = engine.score_epistemic_tier(fact.provenance.tier.as_str());
    let access_frequency = engine.score_access_frequency(u64::from(fact.access.access_count));

    FactorScores {
        vector_similarity,
        decay,
        relevance,
        epistemic_tier,
        relationship_proximity: 0.0,
        access_frequency,
        graph_importance: 0.0,
        serendipity: 0.0,
        surprise: 0.0,
        evidence_coverage: 0.0,
        convergence: 0.0,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use crate::id::FactId;
    use crate::knowledge::{
        EpistemicTier, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    };

    fn make_fact(
        id: &str,
        content: &str,
        tier: EpistemicTier,
        recorded_at: jiff::Timestamp,
        access_count: u32,
    ) -> Fact {
        Fact {
            id: FactId::new(id).expect("valid test id"),
            nous_id: "alice".to_owned(),
            fact_type: "knowledge".to_owned(),
            content: content.to_owned(),
            temporal: FactTemporal {
                valid_from: recorded_at,
                valid_to: recorded_at,
                recorded_at,
            },
            provenance: FactProvenance {
                confidence: 0.8,
                tier,
                source_session_id: None,
                stability_hours: 24.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count,
                last_accessed_at: None,
            },
            sensitivity: FactSensitivity::Public,
            scope: None,
            project_id: None,
            visibility: Visibility::Shared,
        }
    }

    fn multi_factor_weights() -> RecallWeights {
        RecallWeights {
            vector_similarity: 0.1,
            decay: 0.3,
            relevance: 0.1,
            epistemic_tier: 0.3,
            relationship_proximity: 0.0,
            access_frequency: 0.2,
            graph_importance: 0.0,
            serendipity: 0.0,
            surprise: 0.0,
            evidence_coverage: 0.0,
            convergence: 0.0,
        }
    }

    #[test]
    fn explain_ranks_by_non_text_factors_when_term_overlap_is_equal() {
        let now = jiff::Timestamp::now();
        let old = now
            .checked_sub(jiff::SignedDuration::from_secs(30 * 24 * 3600))
            .expect("valid timestamp");
        let fresh = now;

        let facts = vec![
            make_fact(
                "a",
                "rust project uses tokio",
                EpistemicTier::Inferred,
                old,
                0,
            ),
            make_fact(
                "b",
                "rust project uses axum",
                EpistemicTier::Verified,
                fresh,
                10,
            ),
        ];

        let engine = RecallEngine::with_weights(multi_factor_weights());
        let explanation = explain_recall(&engine, &facts, "rust project", Some("alice"), now, 10);

        let selected: Vec<&str> = explanation
            .candidates
            .iter()
            .filter(|c| c.decision == CandidateDecision::Selected)
            .map(|c| c.result.source_id.as_str())
            .collect();
        assert_eq!(selected, vec!["b", "a"]);
    }

    #[test]
    fn explain_records_forgotten_and_no_match_candidates() {
        let now = jiff::Timestamp::now();
        let mut forgotten = make_fact("forgotten", "rust project", EpistemicTier::Inferred, now, 0);
        forgotten.lifecycle.is_forgotten = true;
        let no_match = make_fact("nomatch", "python package", EpistemicTier::Inferred, now, 0);

        let engine = RecallEngine::with_weights(multi_factor_weights());
        let explanation = explain_recall(
            &engine,
            &[forgotten, no_match],
            "rust project",
            Some("alice"),
            now,
            10,
        );

        let forgotten_candidate = explanation
            .candidates
            .iter()
            .find(|c| c.result.source_id == "forgotten")
            .expect("forgotten candidate present");
        assert_eq!(forgotten_candidate.decision, CandidateDecision::Filtered);
        assert!(
            forgotten_candidate
                .reasons
                .contains(&"forgotten".to_owned())
        );

        let no_match_candidate = explanation
            .candidates
            .iter()
            .find(|c| c.result.source_id == "nomatch")
            .expect("no-match candidate present");
        assert_eq!(no_match_candidate.decision, CandidateDecision::Dropped);
        assert!(
            no_match_candidate
                .reasons
                .contains(&"no_term_match".to_owned())
        );
    }

    #[test]
    fn explain_truncates_after_limit() {
        let now = jiff::Timestamp::now();
        let facts = vec![
            make_fact("a", "rust project", EpistemicTier::Inferred, now, 0),
            make_fact("b", "rust project", EpistemicTier::Verified, now, 5),
            make_fact("c", "rust project", EpistemicTier::Reflected, now, 3),
        ];

        let engine = RecallEngine::with_weights(multi_factor_weights());
        let explanation = explain_recall(&engine, &facts, "rust project", Some("alice"), now, 1);

        let selected: Vec<&str> = explanation
            .candidates
            .iter()
            .filter(|c| c.decision == CandidateDecision::Selected)
            .map(|c| c.result.source_id.as_str())
            .collect();
        let truncated: Vec<&str> = explanation
            .candidates
            .iter()
            .filter(|c| c.reasons.contains(&"limit_truncation".to_owned()))
            .map(|c| c.result.source_id.as_str())
            .collect();

        assert_eq!(selected.len(), 1);
        assert!(!truncated.is_empty());
    }
}
