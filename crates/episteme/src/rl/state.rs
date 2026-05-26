//! State boundary types for memory-policy training experiments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Action;

/// Feature-bag state for a memory-policy decision point.
///
/// The concrete 66-constant schema referenced by the Phase 06b issue is not
/// present in this repository. A named feature map keeps the scaffold useful
/// for reward-loader and harness work without freezing placeholder constants.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryState {
    /// Stable identifier for the memory item or policy decision point.
    pub subject_id: String,
    /// Named numeric features available to the policy.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, f64>,
}

impl MemoryState {
    /// Create an empty state for a subject.
    pub fn new(subject_id: impl Into<String>) -> Self {
        Self {
            subject_id: subject_id.into(),
            features: BTreeMap::new(),
        }
    }

    /// Add or replace a named numeric feature.
    #[must_use]
    pub fn with_feature(mut self, name: impl Into<String>, value: f64) -> Self {
        self.features.insert(name.into(), value);
        self
    }
}

/// A single memory-policy transition emitted by a future training environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryTransition {
    /// State before the action.
    pub previous: MemoryState,
    /// Action chosen by the policy.
    pub action: Action,
    /// State after the action.
    pub next: MemoryState,
}
