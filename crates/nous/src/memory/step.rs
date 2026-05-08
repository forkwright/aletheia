//! Structured Step model — assistant's self-note + observation tagging.
//!
//! Distinguishes the assistant's distilled reasoning (`self_note`) from
//! verbose tool results (`observations`). Required by #193's step-positional
//! degradation policy: i < n-2 → notes only; last 2 → full content.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// A single step in the conversation: assistant reasoning plus any
/// tool results that followed it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Assistant's distilled reasoning / plan for this step.
    /// What survives compaction.
    pub self_note: String,

    /// Verbose tool results, file reads, shell output for this step.
    /// First to drop under context budget pressure.
    pub observations: Vec<Observation>,

    /// Post-compaction fallback when both `self_note` and `observations`
    /// are dropped. Optional.
    pub summary: Option<String>,

    /// Position in the session — used by step-positional degradation.
    pub index: usize,

    /// Step boundary timestamp.
    pub started_at: Timestamp,
}

/// A single observation produced by a tool call within a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Source: tool name, file path, etc.
    pub source: String,

    /// Verbose body — the thing that gets dropped under budget pressure.
    pub body: String,

    /// Token estimate for budget calculations (cheap heuristic, not exact).
    pub token_estimate: usize,
}

impl Step {
    /// Construct from an assistant message + the tool results it triggered.
    #[must_use]
    pub fn from_assistant_turn(
        self_note: impl Into<String>,
        observations: Vec<Observation>,
        index: usize,
    ) -> Self {
        Self {
            self_note: self_note.into(),
            observations,
            summary: None,
            index,
            started_at: Timestamp::now(),
        }
    }

    /// Total token estimate (`self_note` + all observations + summary if present).
    #[must_use]
    pub fn token_estimate(&self) -> usize {
        let note_tokens = self.self_note.len().div_ceil(4);
        let obs_tokens = self
            .observations
            .iter()
            .map(|o| o.token_estimate)
            .sum::<usize>();
        let summary_tokens = self.summary.as_ref().map_or(0, |s| s.len().div_ceil(4));
        note_tokens
            .saturating_add(obs_tokens)
            .saturating_add(summary_tokens)
    }

    /// Compact form: just the `self_note` (and summary if observations present).
    ///
    /// Used by step-positional degradation for steps at `i < n-2`.
    #[must_use]
    pub fn compact(&self) -> String {
        if self.observations.is_empty() {
            return self.self_note.clone();
        }
        match &self.summary {
            Some(summary) => format!("{} | {}", self.self_note, summary),
            None => self.self_note.clone(),
        }
    }
}

impl Observation {
    /// Create a new observation from source and body.
    ///
    /// Token estimate is computed from body length using a 4-character heuristic.
    #[must_use]
    pub fn new(source: impl Into<String>, body: impl Into<String>) -> Self {
        let body = body.into();
        let token_estimate = body.len().div_ceil(4);
        Self {
            source: source.into(),
            body,
            token_estimate,
        }
    }
}
