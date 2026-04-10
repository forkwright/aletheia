//! Causal relationship types for the knowledge graph.

use serde::{Deserialize, Serialize};

use crate::id::{CausalEdgeId, FactId};

/// Temporal ordering between cause and effect in a causal edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TemporalOrdering {
    /// Cause precedes effect in time.
    Before,
    /// Effect precedes cause in time (retroactive causation).
    After,
    /// Cause and effect are concurrent.
    Concurrent,
}

impl TemporalOrdering {
    /// Return the lowercase string representation of this ordering.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Concurrent => "concurrent",
        }
    }
}

impl std::str::FromStr for TemporalOrdering {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            "concurrent" => Ok(Self::Concurrent),
            other => Err(format!("unknown temporal ordering: {other}")),
        }
    }
}

/// The type of causal relationship between two facts.
///
/// Ordered roughly by causal strength: `Caused` > `Enabled` > `Prevented` > `Correlated`.
/// Confidence propagation uses the same product rule regardless of type, but
/// callers may weight by type when building causal explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CausalRelationType {
    /// X directly caused Y ("the build failed because of the merge").
    Caused,
    /// X created the conditions for Y ("adding the feature flag enabled the rollout").
    Enabled,
    /// X blocked or stopped Y from occurring ("rate limiting prevented the cascade").
    Prevented,
    /// X and Y co-occur but the causal direction is uncertain or indirect.
    Correlated,
}

impl CausalRelationType {
    /// Return the `snake_case` string representation of this relation type.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Caused => "caused",
            Self::Enabled => "enabled",
            Self::Prevented => "prevented",
            Self::Correlated => "correlated",
        }
    }
}

impl std::fmt::Display for CausalRelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for CausalRelationType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "caused" => Ok(Self::Caused),
            "enabled" => Ok(Self::Enabled),
            "prevented" => Ok(Self::Prevented),
            "correlated" => Ok(Self::Correlated),
            other => Err(format!("unknown causal relation type: {other}")),
        }
    }
}

/// A directed causal edge between two fact nodes in the knowledge graph.
///
/// Represents "X caused/enabled/prevented/correlated Y" with a typed relationship,
/// temporal ordering, and extraction confidence. Confidence propagates through
/// causal chains: transitive confidence is the product of individual edge confidences.
///
/// Edges are heuristically extracted from session text during finalize. The
/// `evidence_session_id` records which session produced the evidence so
/// the edge can be traced back to its source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEdge {
    /// Unique identifier for this edge.
    pub id: CausalEdgeId,
    /// Fact ID of the source (cause) node.
    pub source_id: FactId,
    /// Fact ID of the target (effect) node.
    pub target_id: FactId,
    /// Semantic type of the causal relationship.
    pub relationship_type: CausalRelationType,
    /// Temporal ordering between source and target.
    pub ordering: TemporalOrdering,
    /// Confidence that this causal relationship holds (0.0--1.0).
    ///
    /// Reflects both the strength of the evidence and the extraction heuristic
    /// quality. Heuristically extracted edges default to 0.5; user-confirmed
    /// edges may be raised toward 1.0.
    pub confidence: f64,
    /// Session ID where the causal evidence was observed, if known.
    pub evidence_session_id: Option<String>,
    /// When this edge was recorded.
    pub timestamp: jiff::Timestamp,
}
