//! Evidence-gap tracking for iterative retrieval.
//!
//! Based on MemR3 (Dec 2025, arxiv 2512.20237). During memory retrieval,
//! explicitly track what evidence is still missing and support iterative
//! retrieval rounds with reflection between them.
//!
//! ## Flow
//!
//! 1. Decompose the original query into sub-questions (heuristic, no LLM).
//! 2. After each retrieval round, call [`EvidenceGapTracker::record_evidence`]
//!    for each sub-question that was (partially) answered.
//! 3. Check [`EvidenceGapTracker::remaining_gaps`] or
//!    [`EvidenceGapTracker::is_satisfied`] to decide whether another round is
//!    needed.
//! 4. Use [`EvidenceGapTracker::suggest_refinement`] to generate the next
//!    retrieval query targeting the biggest gaps.

/// A sub-question that has been answered with supporting evidence.
#[derive(Debug, Clone)]
pub struct AnsweredQuestion {
    /// The sub-question text.
    pub question: String,
    /// Fact IDs from the knowledge store that support the answer.
    pub evidence_ids: Vec<String>,
    /// How well the evidence covers this question (0.0 = no coverage, 1.0 = fully answered).
    pub confidence: f64,
}

/// Decomposed query with tracking of answered and unanswered sub-questions.
#[derive(Debug, Clone)]
pub struct EvidenceQuery {
    /// The user's original information need.
    pub original_query: String,
    /// Heuristically decomposed sub-questions.
    pub sub_questions: Vec<String>,
    /// Sub-questions with supporting evidence.
    pub answered: Vec<AnsweredQuestion>,
    /// Sub-questions still unanswered.
    pub gaps: Vec<String>,
}

/// Tracks evidence coverage across iterative retrieval rounds.
///
/// Initialise with [`EvidenceGapTracker::new`], record evidence after each
/// retrieval round, and query coverage / remaining gaps to decide whether
/// another round is warranted.
#[derive(Debug, Clone)]
pub struct EvidenceGapTracker {
    query: EvidenceQuery,
}

impl EvidenceGapTracker {
    /// Create a new tracker from the original query.
    ///
    /// The query is heuristically decomposed into sub-questions (see
    /// [`decompose_query`]). All sub-questions start as unanswered gaps.
    ///
    /// # Complexity
    ///
    /// O(N) where N is the character length of `query` (string scanning for
    /// conjunction/interrogative splits).
    #[must_use]
    pub fn new(query: &str) -> Self {
        let sub_questions = decompose_query(query);
        let gaps = sub_questions.clone();
        Self {
            query: EvidenceQuery {
                original_query: query.to_owned(),
                sub_questions,
                answered: Vec::new(),
                gaps,
            },
        }
    }

    /// Record evidence for a sub-question.
    ///
    /// If the sub-question at `question_idx` is still in the gap list, it is
    /// moved to the answered list. If it was already answered, the new evidence
    /// is merged (IDs appended, confidence updated to the max of old and new).
    ///
    /// Out-of-bounds `question_idx` values are silently ignored.
    ///
    /// # Complexity
    ///
    /// O(G + E) where G is the gap count and E is the existing evidence-ID
    /// count for the question (when merging).
    pub fn record_evidence(&mut self, question_idx: usize, fact_id: &str, confidence: f64) {
        let confidence = confidence.clamp(0.0, 1.0);

        let Some(question_text) = self.query.sub_questions.get(question_idx) else {
            return;
        };
        let question_text = question_text.clone();

        // Check if already answered -- merge if so.
        if let Some(existing) = self
            .query
            .answered
            .iter_mut()
            .find(|a| a.question == question_text)
        {
            if !existing.evidence_ids.contains(&fact_id.to_owned()) {
                existing.evidence_ids.push(fact_id.to_owned());
            }
            if confidence > existing.confidence {
                existing.confidence = confidence;
            }
            return;
        }

        // Remove from gaps, add to answered.
        self.query.gaps.retain(|g| g != &question_text);
        self.query.answered.push(AnsweredQuestion {
            question: question_text,
            evidence_ids: vec![fact_id.to_owned()],
            confidence,
        });
    }

    /// Returns the sub-questions that have not been answered yet.
    ///
    /// # Complexity
    ///
    /// O(1) -- returns a slice reference.
    #[must_use]
    pub fn remaining_gaps(&self) -> &[String] {
        &self.query.gaps
    }

    /// Fraction of sub-questions that have been answered.
    ///
    /// Returns a value in `[0.0, 1.0]`. A query with zero sub-questions
    /// returns `1.0` (vacuously satisfied).
    ///
    /// # Complexity
    ///
    /// O(1) -- integer division.
    #[must_use]
    pub fn coverage_ratio(&self) -> f64 {
        let total = self.query.sub_questions.len();
        if total == 0 {
            return 1.0;
        }
        let answered = self.query.answered.len();
        // Sub-question counts are small (typically <20), so the u32
        // conversion never truncates and f64 is exact for these values.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "sub-question counts are bounded by query length; well within u32"
        )]
        let answered_u32 = answered as u32;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "sub-question counts are bounded by query length; well within u32"
        )]
        let total_u32 = total as u32;
        f64::from(answered_u32) / f64::from(total_u32)
    }

    /// Whether the evidence gathered meets the minimum coverage threshold.
    ///
    /// `min_coverage` is clamped to `[0.0, 1.0]`.
    ///
    /// # Complexity
    ///
    /// O(1).
    #[must_use]
    pub fn is_satisfied(&self, min_coverage: f64) -> bool {
        self.coverage_ratio() >= min_coverage.clamp(0.0, 1.0)
    }

    /// Suggest a refinement query targeting the largest remaining gap.
    ///
    /// Returns `None` when all sub-questions have been answered. Otherwise
    /// returns a query string composed of the unanswered sub-questions,
    /// prefixed with context from the original query.
    ///
    /// # Complexity
    ///
    /// O(G) where G is the number of remaining gaps.
    #[must_use]
    pub fn suggest_refinement(&self) -> Option<String> {
        if self.query.gaps.is_empty() {
            return None;
        }

        if self.query.gaps.len() == 1 {
            return self.query.gaps.first().cloned();
        }

        // Combine unanswered sub-questions into a single refinement query.
        let gap_text = self.query.gaps.join("; ");
        Some(format!(
            "Regarding \"{}\": {}",
            self.query.original_query, gap_text
        ))
    }

    /// Access the underlying [`EvidenceQuery`] state.
    ///
    /// # Complexity
    ///
    /// O(1).
    #[must_use]
    pub fn query(&self) -> &EvidenceQuery {
        &self.query
    }
}

/// Heuristic sub-question decomposition (no LLM required).
///
/// Strategy:
/// 1. Split on conjunctions ("and", "or", "but") at word boundaries.
/// 2. Within each clause, extract interrogative phrases ("who", "what",
///    "when", "where", "why", "how") as separate sub-questions.
/// 3. If the query is simple (single clause, no interrogatives beyond
///    the leading one), use it as-is.
///
/// All results are trimmed and non-empty.
///
/// # Complexity
///
/// O(N) where N is the character length of `query`.
fn decompose_query(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Step 1: split on conjunctions at word boundaries.
    let clauses = split_on_conjunctions(trimmed);

    // Step 2: for each clause, try to extract interrogative sub-questions.
    let mut sub_questions: Vec<String> = Vec::new();
    for clause in &clauses {
        let interrogatives = extract_interrogatives(clause);
        if interrogatives.is_empty() {
            // No embedded interrogatives -- use the clause as-is.
            let cleaned = clause.trim().to_owned();
            if !cleaned.is_empty() {
                sub_questions.push(cleaned);
            }
        } else {
            sub_questions.extend(interrogatives);
        }
    }

    // Step 3: if decomposition produced nothing useful, fall back to original.
    if sub_questions.is_empty() {
        return vec![trimmed.to_owned()];
    }

    sub_questions
}

/// Split a query on conjunctions ("and", "or", "but") that appear as
/// standalone words (word boundaries on both sides).
///
/// Only splits when it produces at least two non-empty parts. Returns the
/// original string as a single-element vec otherwise.
fn split_on_conjunctions(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let conjunctions = [" and ", " or ", " but "];

    let mut split_positions: Vec<(usize, usize)> = Vec::new();
    for conj in &conjunctions {
        let mut start = 0;
        while let Some(pos) = lower.get(start..).and_then(|s| s.find(conj)) {
            let abs_pos = start + pos;
            split_positions.push((abs_pos, abs_pos + conj.len()));
            start = abs_pos + conj.len();
        }
    }

    if split_positions.is_empty() {
        return vec![query.to_owned()];
    }

    split_positions.sort_by_key(|&(start, _)| start);

    let mut parts = Vec::new();
    let mut prev = 0;
    for (conj_start, conj_end) in &split_positions {
        if let Some(part) = query.get(prev..*conj_start) {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_owned());
            }
        }
        prev = *conj_end;
    }
    // Trailing part.
    if let Some(tail) = query.get(prev..) {
        let trimmed = tail.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_owned());
        }
    }

    if parts.len() < 2 {
        return vec![query.to_owned()];
    }

    parts
}

/// Extract interrogative sub-questions from a clause.
///
/// Looks for "who", "what", "when", "where", "why", "how" as word
/// prefixes within the clause. If more than one is found, each
/// interrogative phrase (from the interrogative word to the next one or
/// end of string) becomes a sub-question.
///
/// Returns an empty vec if zero or one interrogative is found (meaning
/// the clause is already atomic enough).
#[expect(
    clippy::indexing_slicing,
    reason = "byte index `abs - 1` is guarded by `abs > 0` and ASCII-only content"
)]
fn extract_interrogatives(clause: &str) -> Vec<String> {
    let lower = clause.to_lowercase();
    let interrogatives = ["who ", "what ", "when ", "where ", "why ", "how "];

    let mut positions: Vec<usize> = Vec::new();
    for interr in &interrogatives {
        let mut start = 0;
        while let Some(pos) = lower.get(start..).and_then(|s| s.find(interr)) {
            let abs = start + pos;
            // Ensure word boundary: must be at start or preceded by non-alphanumeric.
            if abs == 0 || !lower.as_bytes()[abs - 1].is_ascii_alphanumeric() {
                positions.push(abs);
            }
            start = abs + interr.len();
        }
    }

    if positions.len() < 2 {
        return Vec::new();
    }

    positions.sort_unstable();
    positions.dedup();

    let mut parts = Vec::new();
    for (i, &pos) in positions.iter().enumerate() {
        let end = positions.get(i + 1).copied().unwrap_or(clause.len());
        if let Some(slice) = clause.get(pos..end) {
            let part = slice.trim().trim_end_matches(',').trim();
            if !part.is_empty() {
                parts.push(part.to_owned());
            }
        }
    }

    parts
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions on collections with known length")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    // ---- Sub-question decomposition ----

    #[test]
    fn simple_query_stays_intact() {
        let tracker = EvidenceGapTracker::new("What is the capital of France?");
        let q = tracker.query();
        assert_eq!(q.sub_questions.len(), 1);
        assert_eq!(q.sub_questions[0], "What is the capital of France?");
        assert_eq!(q.gaps.len(), 1);
        assert!(q.answered.is_empty());
    }

    #[test]
    fn compound_query_splits_on_conjunction() {
        let tracker =
            EvidenceGapTracker::new("What is the capital of France and what is its population?");
        let q = tracker.query();
        assert_eq!(q.sub_questions.len(), 2);
        assert!(q.sub_questions[0].contains("capital"));
        assert!(q.sub_questions[1].contains("population"));
    }

    #[test]
    fn multiple_conjunctions_split() {
        let tracker = EvidenceGapTracker::new(
            "Explain the cause and describe the effect but mention the timeline",
        );
        let q = tracker.query();
        assert!(
            q.sub_questions.len() >= 3,
            "expected at least 3 sub-questions, got {:?}",
            q.sub_questions
        );
    }

    #[test]
    fn empty_query_produces_no_subquestions() {
        let tracker = EvidenceGapTracker::new("");
        assert!(tracker.query().sub_questions.is_empty());
        // Vacuously satisfied.
        assert!((tracker.coverage_ratio() - 1.0).abs() < f64::EPSILON);
    }

    // ---- Evidence recording ----

    #[test]
    fn record_evidence_moves_gap_to_answered() {
        let mut tracker = EvidenceGapTracker::new("A and B");
        assert_eq!(tracker.remaining_gaps().len(), 2);

        tracker.record_evidence(0, "fact-001", 0.9);
        assert_eq!(tracker.remaining_gaps().len(), 1);
        assert_eq!(tracker.query().answered.len(), 1);
        assert_eq!(tracker.query().answered[0].evidence_ids, vec!["fact-001"]);
        assert!((tracker.query().answered[0].confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn record_evidence_merges_on_same_question() {
        let mut tracker = EvidenceGapTracker::new("A and B");
        tracker.record_evidence(0, "fact-001", 0.5);
        tracker.record_evidence(0, "fact-002", 0.8);

        // Still only one answered question.
        assert_eq!(tracker.query().answered.len(), 1);
        let answered = &tracker.query().answered[0];
        assert_eq!(answered.evidence_ids.len(), 2);
        // Confidence is max(0.5, 0.8).
        assert!((answered.confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn record_evidence_ignores_out_of_bounds() {
        let mut tracker = EvidenceGapTracker::new("just one question");
        tracker.record_evidence(999, "fact-001", 1.0);
        // Nothing changed.
        assert_eq!(tracker.remaining_gaps().len(), 1);
        assert!(tracker.query().answered.is_empty());
    }

    #[test]
    fn record_evidence_clamps_confidence() {
        let mut tracker = EvidenceGapTracker::new("single");
        tracker.record_evidence(0, "fact-001", 5.0);
        assert!((tracker.query().answered[0].confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn duplicate_fact_id_not_added_twice() {
        let mut tracker = EvidenceGapTracker::new("A and B");
        tracker.record_evidence(0, "fact-001", 0.5);
        tracker.record_evidence(0, "fact-001", 0.9);
        assert_eq!(tracker.query().answered[0].evidence_ids.len(), 1);
        // Confidence updated to the higher value.
        assert!((tracker.query().answered[0].confidence - 0.9).abs() < f64::EPSILON);
    }

    // ---- Coverage tracking ----

    #[test]
    fn coverage_ratio_tracks_progress() {
        let mut tracker = EvidenceGapTracker::new("A and B");
        assert!((tracker.coverage_ratio() - 0.0).abs() < f64::EPSILON);

        tracker.record_evidence(0, "fact-001", 0.9);
        assert!((tracker.coverage_ratio() - 0.5).abs() < f64::EPSILON);

        tracker.record_evidence(1, "fact-002", 0.8);
        assert!((tracker.coverage_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn is_satisfied_with_threshold() {
        let mut tracker = EvidenceGapTracker::new("A and B and C and D");
        assert!(!tracker.is_satisfied(0.5));

        // Answer 2 out of 4 sub-questions.
        tracker.record_evidence(0, "f1", 0.9);
        tracker.record_evidence(1, "f2", 0.9);
        assert!(tracker.is_satisfied(0.5));
        assert!(!tracker.is_satisfied(0.75));

        // Answer a third.
        tracker.record_evidence(2, "f3", 0.9);
        assert!(tracker.is_satisfied(0.75));
    }

    // ---- Gap identification ----

    #[test]
    fn remaining_gaps_shrinks_as_evidence_recorded() {
        let mut tracker = EvidenceGapTracker::new("A and B and C");
        let initial_gaps = tracker.remaining_gaps().len();
        assert_eq!(initial_gaps, 3);

        tracker.record_evidence(1, "fact-001", 0.9);
        assert_eq!(tracker.remaining_gaps().len(), 2);
        // The gap that was removed corresponds to sub_questions[1].
        let removed_q = &tracker.query().sub_questions[1];
        assert!(!tracker.remaining_gaps().contains(removed_q));
    }

    // ---- Refinement suggestion ----

    #[test]
    fn suggest_refinement_returns_none_when_satisfied() {
        let mut tracker = EvidenceGapTracker::new("just one");
        tracker.record_evidence(0, "fact-001", 1.0);
        assert!(tracker.suggest_refinement().is_none());
    }

    #[test]
    fn suggest_refinement_returns_single_gap_directly() {
        let mut tracker = EvidenceGapTracker::new("A and B");
        tracker.record_evidence(0, "fact-001", 0.9);
        let suggestion = tracker.suggest_refinement().expect("should have a suggestion");
        // Single remaining gap returned directly (no wrapping).
        assert_eq!(suggestion, tracker.remaining_gaps()[0]);
    }

    #[test]
    fn suggest_refinement_combines_multiple_gaps() {
        let tracker = EvidenceGapTracker::new("A and B and C");
        let suggestion = tracker.suggest_refinement().expect("should have a suggestion");
        // Multiple gaps: includes original query context.
        assert!(suggestion.contains("Regarding"));
        assert!(suggestion.contains(&tracker.query().original_query));
    }

    // ---- Decomposition internals ----

    #[test]
    fn interrogative_extraction_splits_compound_wh_questions() {
        let tracker =
            EvidenceGapTracker::new("who invented the telephone and when was it invented");
        let q = tracker.query();
        // "and" splits first, then each clause has a single interrogative -- stays as-is.
        assert!(
            q.sub_questions.len() >= 2,
            "expected >= 2, got {:?}",
            q.sub_questions
        );
    }

    #[test]
    fn or_conjunction_splits() {
        let tracker = EvidenceGapTracker::new("Is it rain or shine tomorrow");
        let q = tracker.query();
        assert_eq!(
            q.sub_questions.len(),
            2,
            "expected 2, got {:?}",
            q.sub_questions
        );
    }
}
