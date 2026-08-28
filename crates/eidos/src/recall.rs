//! Operator-tunable recall scoring weights.
//!
//! This is the six-factor subset an operator can override through taxis
//! config and that nous threads into the recall stage — distinct from the
//! full eleven-factor engine weights (`mneme::recall::RecallWeights`, defined
//! in episteme), which also carries the always-inert-by-default overlay
//! factors (serendipity, surprise, evidence-gap coverage, convergence) that
//! are not exposed here. Defined once so taxis and nous share exactly one
//! definition and one set of defaults instead of two structs that can (and
//! did) drift.

use serde::{Deserialize, Serialize};

/// Per-factor base scores for the recall pipeline's operator-tunable subset.
///
/// These values are placed directly into the non-vector
/// [`mneme::recall::FactorScores`] fields. Only vector similarity is computed
/// from the actual embedding distance; decay, relevance, tier, proximity,
/// frequency, and graph importance use these configured values as their
/// scores. Operators override the defaults in taxis config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecallWeights {
    /// Temporal decay weight (0.0-1.0).
    pub decay: f64,
    /// Content relevance weight (0.0-1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0-1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0-1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0-1.0).
    pub access_frequency: f64,
    /// Graph `PageRank` importance weight (0.0-1.0).
    pub graph_importance: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.3,
            relationship_proximity: 0.1,
            access_frequency: 0.0,
            graph_importance: 0.1,
        }
    }
}
