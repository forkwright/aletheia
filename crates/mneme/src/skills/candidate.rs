//! Skill candidate tracking — Rule of Three promotion.
//!
//! A [`CandidateTracker`] accumulates tool call sequences that pass the
//! heuristic filter.  When the same pattern recurs 3 or more times, it is
//! **promoted** — signalling that an LLM extraction pass should turn it into
//! a proper [`crate::skill::SkillContent`].
//!
//! ## Storage
//!
//! Candidates are kept in memory as [`SkillCandidate`] values.  The struct is
//! fully serialisable so callers can persist it as a `Fact` with
//! `fact_type = "skill_candidate"` in the knowledge store.
//!
//! ## Similarity threshold
//!
//! Two signatures are considered the *same pattern* when
//! [`crate::skills::signature_similarity`] ≥ 0.8.

use serde::{Deserialize, Serialize};

use crate::skills::{
    heuristics::score_sequence,
    signature::{sequence_signature, signature_similarity},
    SequenceSignature, ToolCallRecord,
};

/// Minimum recurrence count to promote a candidate to a skill.
pub const PROMOTION_THRESHOLD: u32 = 3;

/// Similarity threshold for merging two sequences into the same candidate.
pub const SIMILARITY_THRESHOLD: f64 = 0.8;

// ---------------------------------------------------------------------------
// SkillCandidate
// ---------------------------------------------------------------------------

/// A tracked pattern that has been seen at least once and may be promoted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    /// Unique identifier (ULID as string).
    pub id: String,
    /// Which nous this candidate belongs to.
    pub nous_id: String,
    /// Normalised signature of the representative tool call sequence.
    pub signature: SequenceSignature,
    /// Number of sessions where this pattern appeared.
    pub recurrence_count: u32,
    /// Session IDs where the pattern appeared.
    pub session_refs: Vec<String>,
    /// Timestamp of first observation.
    pub first_seen: jiff::Timestamp,
    /// Timestamp of most recent observation.
    pub last_seen: jiff::Timestamp,
    /// Heuristic score from the first observation.
    pub heuristic_score: f64,
    /// Detected pattern type from the first observation.
    pub pattern_type: Option<crate::skills::PatternType>,
}

impl SkillCandidate {
    /// Serialise to JSON for storage as a `Fact.content` field.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialisation fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialise from JSON stored in a `Fact.content` field.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the JSON is malformed or the schema changed.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

// ---------------------------------------------------------------------------
// TrackResult
// ---------------------------------------------------------------------------

/// Outcome from [`CandidateTracker::track_sequence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackResult {
    /// Sequence failed the heuristic gates — not tracked.
    Rejected,
    /// New candidate created (first occurrence).
    New,
    /// Existing candidate updated.  Contains the new recurrence count.
    Tracked(u32),
    /// Candidate promoted (`recurrence_count` reached [`PROMOTION_THRESHOLD`]).
    /// Contains the candidate ID.
    Promoted(String),
}

// ---------------------------------------------------------------------------
// CandidateTracker
// ---------------------------------------------------------------------------

/// In-memory store for skill candidates with Rule-of-Three promotion.
///
/// Thread-safe via an internal [`std::sync::Mutex`].
/// Serialize each [`SkillCandidate`] to JSON and persist as a fact with
/// `fact_type = "skill_candidate"` for durable storage.
pub struct CandidateTracker {
    candidates: std::sync::Mutex<Vec<SkillCandidate>>,
}

impl std::fmt::Debug for CandidateTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.candidates.lock().expect("lock not poisoned");
        f.debug_struct("CandidateTracker")
            .field("count", &guard.len())
            .finish()
    }
}

impl Default for CandidateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CandidateTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        Self {
            candidates: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Process a new tool call sequence.
    ///
    /// Returns [`TrackResult::Rejected`] when the heuristic gates fail.
    /// Returns [`TrackResult::Promoted`] when the candidate's recurrence
    /// count reaches [`PROMOTION_THRESHOLD`].
    pub fn track_sequence(
        &self,
        tool_calls: &[ToolCallRecord],
        session_id: &str,
        nous_id: &str,
    ) -> TrackResult {
        let score = score_sequence(tool_calls);
        if !score.passed_gates {
            return TrackResult::Rejected;
        }

        let sig = sequence_signature(tool_calls);
        let now = jiff::Timestamp::now();

        let mut candidates = self.candidates.lock().expect("lock not poisoned");

        // Find an existing candidate with a similar signature for the same nous
        if let Some(existing) = candidates
            .iter_mut()
            .find(|c| c.nous_id == nous_id && signature_similarity(&c.signature, &sig) >= SIMILARITY_THRESHOLD)
        {
            existing.recurrence_count += 1;
            existing.last_seen = now;
            if !existing.session_refs.contains(&session_id.to_owned()) {
                existing.session_refs.push(session_id.to_owned());
            }

            let new_count = existing.recurrence_count;
            let id = existing.id.clone();

            if new_count >= PROMOTION_THRESHOLD {
                return TrackResult::Promoted(id);
            }
            return TrackResult::Tracked(new_count);
        }

        // New candidate
        let id = ulid::Ulid::new().to_string();
        candidates.push(SkillCandidate {
            id: id.clone(),
            nous_id: nous_id.to_owned(),
            signature: sig,
            recurrence_count: 1,
            session_refs: vec![session_id.to_owned()],
            first_seen: now,
            last_seen: now,
            heuristic_score: score.total,
            pattern_type: score.pattern_type,
        });

        TrackResult::New
    }

    /// Return all current candidates for a given nous.
    pub fn candidates_for(&self, nous_id: &str) -> Vec<SkillCandidate> {
        let guard = self.candidates.lock().expect("lock not poisoned");
        guard
            .iter()
            .filter(|c| c.nous_id == nous_id)
            .cloned()
            .collect()
    }

    /// Return all promoted candidates (`recurrence_count` ≥ threshold) for a nous.
    pub fn promoted_for(&self, nous_id: &str) -> Vec<SkillCandidate> {
        let guard = self.candidates.lock().expect("lock not poisoned");
        guard
            .iter()
            .filter(|c| c.nous_id == nous_id && c.recurrence_count >= PROMOTION_THRESHOLD)
            .cloned()
            .collect()
    }

    /// Total number of tracked candidates (all nous IDs).
    pub fn len(&self) -> usize {
        self.candidates.lock().expect("lock not poisoned").len()
    }

    /// Returns `true` if no candidates are tracked.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::ToolCallRecord;

    fn tc(name: &str) -> ToolCallRecord {
        ToolCallRecord::new(name, 100)
    }

    /// A sequence that passes all heuristic gates.
    fn good_seq() -> Vec<ToolCallRecord> {
        vec![
            tc("Grep"),
            tc("Read"),
            tc("Read"),
            tc("Edit"),
            tc("Bash"),
            tc("Bash"),
        ]
    }

    /// A slightly different but similar sequence (should merge at 0.8+).
    fn similar_seq() -> Vec<ToolCallRecord> {
        vec![
            tc("Grep"),
            tc("Read"),
            tc("Edit"),
            tc("Edit"),
            tc("Bash"),
            tc("Bash"),
        ]
    }

    /// A clearly different sequence.
    fn different_seq() -> Vec<ToolCallRecord> {
        vec![
            tc("WebSearch"),
            tc("WebFetch"),
            tc("Read"),
            tc("Read"),
            tc("Write"),
            tc("Bash"),
        ]
    }

    // ------------------------------------------------------------------
    // Basic tracking
    // ------------------------------------------------------------------

    #[test]
    fn track_rejected_sequence_returns_rejected() {
        let tracker = CandidateTracker::new();
        // Too short to pass gates
        let short = vec![tc("Read"), tc("Edit"), tc("Bash")];
        assert_eq!(
            tracker.track_sequence(&short, "s1", "nous1"),
            TrackResult::Rejected
        );
        assert!(tracker.is_empty());
    }

    #[test]
    fn first_occurrence_returns_new() {
        let tracker = CandidateTracker::new();
        let result = tracker.track_sequence(&good_seq(), "s1", "nous1");
        assert_eq!(result, TrackResult::New);
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn second_occurrence_returns_tracked_count_two() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        let result = tracker.track_sequence(&good_seq(), "s2", "nous1");
        assert_eq!(result, TrackResult::Tracked(2));
    }

    // ------------------------------------------------------------------
    // Rule of Three
    // ------------------------------------------------------------------

    #[test]
    fn third_occurrence_returns_promoted() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&good_seq(), "s2", "nous1");
        let result = tracker.track_sequence(&good_seq(), "s3", "nous1");
        assert!(
            matches!(result, TrackResult::Promoted(_)),
            "expected Promoted, got {result:?}"
        );
    }

    #[test]
    fn promoted_candidate_has_correct_recurrence_count() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&good_seq(), "s2", "nous1");
        tracker.track_sequence(&good_seq(), "s3", "nous1");
        let promoted = tracker.promoted_for("nous1");
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].recurrence_count, PROMOTION_THRESHOLD);
    }

    #[test]
    fn fourth_occurrence_also_promoted() {
        let tracker = CandidateTracker::new();
        for i in 1..=4 {
            tracker.track_sequence(&good_seq(), &format!("s{i}"), "nous1");
        }
        let candidates = tracker.candidates_for("nous1");
        assert_eq!(candidates[0].recurrence_count, 4);
    }

    // ------------------------------------------------------------------
    // Similar sequence merging
    // ------------------------------------------------------------------

    #[test]
    fn similar_sequences_merge_into_same_candidate() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        // Similar but not identical
        let result = tracker.track_sequence(&similar_seq(), "s2", "nous1");
        // Should find similarity >= 0.8 and merge
        assert_eq!(tracker.len(), 1, "similar sequences should merge");
        assert_eq!(result, TrackResult::Tracked(2));
    }

    #[test]
    fn different_sequences_create_separate_candidates() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&different_seq(), "s2", "nous1");
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn similar_sequences_trigger_promotion_at_three() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&similar_seq(), "s2", "nous1");
        let result = tracker.track_sequence(&good_seq(), "s3", "nous1");
        assert!(
            matches!(result, TrackResult::Promoted(_)),
            "should promote after 3 similar occurrences"
        );
    }

    // ------------------------------------------------------------------
    // nous isolation
    // ------------------------------------------------------------------

    #[test]
    fn different_nous_ids_are_isolated() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&good_seq(), "s1", "nous2");
        // Each nous gets its own candidate
        assert_eq!(tracker.len(), 2);
        assert_eq!(tracker.candidates_for("nous1").len(), 1);
        assert_eq!(tracker.candidates_for("nous2").len(), 1);
    }

    #[test]
    fn same_nous_patterns_dont_cross_contaminate() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&good_seq(), "s2", "nous1");
        // nous2 starts fresh
        let result = tracker.track_sequence(&good_seq(), "s3", "nous2");
        assert_eq!(result, TrackResult::New, "nous2 should start at count 1");
    }

    // ------------------------------------------------------------------
    // Session refs
    // ------------------------------------------------------------------

    #[test]
    fn session_refs_accumulated() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "session-a", "nous1");
        tracker.track_sequence(&good_seq(), "session-b", "nous1");
        let candidates = tracker.candidates_for("nous1");
        let refs = &candidates[0].session_refs;
        assert!(refs.contains(&"session-a".to_owned()));
        assert!(refs.contains(&"session-b".to_owned()));
    }

    #[test]
    fn duplicate_session_not_added_twice() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        tracker.track_sequence(&good_seq(), "s1", "nous1"); // same session
        let candidates = tracker.candidates_for("nous1");
        let session_count = candidates[0]
            .session_refs
            .iter()
            .filter(|s| s.as_str() == "s1")
            .count();
        assert_eq!(session_count, 1, "duplicate session should not be added");
    }

    // ------------------------------------------------------------------
    // Serialisation
    // ------------------------------------------------------------------

    #[test]
    fn skill_candidate_serialises_to_json() {
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&good_seq(), "s1", "nous1");
        let candidates = tracker.candidates_for("nous1");
        let json = candidates[0].to_json().expect("serialisation should succeed");
        let back = SkillCandidate::from_json(&json).expect("deserialisation should succeed");
        assert_eq!(back.id, candidates[0].id);
        assert_eq!(back.recurrence_count, 1);
    }
}
