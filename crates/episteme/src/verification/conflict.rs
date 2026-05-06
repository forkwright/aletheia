//! Conflict resolution primitives.
//!
//! Vector-similarity detection (`detect_conflict`) requires a
//! `KnowledgeStore` reference plumbed into the extraction engine; that
//! wire-up is its own change. This module ships the pure-data scoring
//! path so callers in `nous` (PR3) can already exchange resolution
//! outcomes.

use eidos::id::FactId;
use eidos::knowledge::{ConflictResolution, Fact};
use serde::{Deserialize, Serialize};

/// Categorical kind of cross-fact conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConflictKind {
    /// Two facts make incompatible claims about the same subject.
    Contradiction,
    /// Two facts express the same claim (deduplication target).
    Duplicate,
    /// Same name resolves to different entity types across nouses.
    EntityCollision,
}

/// A detected conflict between two facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Newly-extracted fact under consideration.
    pub incoming: FactId,
    /// Existing fact in the store that conflicts.
    pub existing: FactId,
    /// Conflict classification.
    pub kind: ConflictKind,
    /// Vector-similarity score in `[0.0, 1.0]` (1.0 = identical embedding).
    pub similarity: f64,
}

/// Errors returned when conflict resolution preconditions fail.
#[derive(Debug, snafu::Snafu, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// `facts` slice was empty.
    #[snafu(display("resolve_conflict requires at least one fact"))]
    Empty,
    /// `facts` and `supporters` slices had different lengths.
    #[snafu(display(
        "facts and supporters slices must be the same length; got facts={facts}, supporters={supporters}"
    ))]
    LengthMismatch {
        /// Length of the `facts` slice.
        facts: usize,
        /// Length of the `supporters` slice.
        supporters: usize,
    },
}

/// Resolve a conflict among multiple competing facts using composite scoring.
///
/// `supporters[i]` is the count of distinct nouses backing `facts[i]` (typically
/// `verification_count + 1` to include the publisher). `now` parameterizes
/// recency for deterministic testing.
///
/// Losers retain their `contested_by` provenance — callers must NOT delete
/// loser facts as a side effect of resolution.
///
/// # Errors
///
/// Returns [`ResolveError::Empty`] if `facts` is empty, or
/// [`ResolveError::LengthMismatch`] if `facts.len() != supporters.len()`.
pub fn resolve_conflict(
    facts: &[&Fact],
    supporters: &[u32],
    now: jiff::Timestamp,
) -> Result<ConflictResolution, ResolveError> {
    if facts.is_empty() {
        return Err(ResolveError::Empty);
    }
    if facts.len() != supporters.len() {
        return Err(ResolveError::LengthMismatch {
            facts: facts.len(),
            supporters: supporters.len(),
        });
    }

    let mut best_idx: usize = 0;
    let mut best_score = f64::NEG_INFINITY;
    for (i, (f, s)) in facts.iter().zip(supporters.iter()).enumerate() {
        let score = ConflictResolution::compute_score(f, *s, now);
        if i == 0 || score > best_score {
            best_score = score;
            best_idx = i;
        }
    }

    let mut winner_id: Option<FactId> = None;
    let mut losers: Vec<FactId> = Vec::with_capacity(facts.len().saturating_sub(1));
    for (i, f) in facts.iter().enumerate() {
        if i == best_idx {
            winner_id = Some(f.id.clone());
        } else {
            losers.push(f.id.clone());
        }
    }

    // SAFETY: best_idx is derived from enumerating the (proven non-empty)
    // facts slice, so winner_id must be Some.
    let winner = winner_id.ok_or(ResolveError::Empty)?;

    Ok(ConflictResolution {
        winner,
        losers,
        winning_score: best_score,
        resolved_at: now,
    })
}
