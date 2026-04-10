//! Checkpoint approval gate state for the planning project detail view.

use serde::{Deserialize, Serialize};

/// Action taken on a checkpoint gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum CheckpointAction {
    /// Gate approved; execution continues.
    Approve,
    /// Gate skipped without approval; notes required.
    Skip,
    /// Failed gate overridden; notes required.
    Override,
}

/// Current status of a checkpoint gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum CheckpointStatus {
    /// Awaiting review.
    Pending,
    /// Approved by a reviewer.
    Approved,
    /// Skipped with a note.
    Skipped,
    /// Failed gate overridden with a note.
    Overridden,
}

/// A single requirement validated by this checkpoint.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct CheckpointRequirement {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) met: bool,
}

/// An artifact produced or referenced by this checkpoint.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct CheckpointArtifact {
    pub(crate) label: String,
    pub(crate) value: String,
}

/// Decision recorded after a checkpoint action is taken.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct CheckpointDecision {
    pub(crate) action: CheckpointAction,
    pub(crate) actor: String,
    pub(crate) timestamp: String,
    pub(crate) notes: String,
}

/// A single checkpoint approval gate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Checkpoint {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    /// Summary of what was accomplished and what follows if approved.
    pub(crate) context: String,
    pub(crate) requirements: Vec<CheckpointRequirement>,
    pub(crate) artifacts: Vec<CheckpointArtifact>,
    pub(crate) status: CheckpointStatus,
    /// Set once the gate is resolved.
    #[serde(default)]
    pub(crate) decision: Option<CheckpointDecision>,
}

/// Request body for `POST /api/planning/projects/{id}/checkpoints/{id}/action`.
#[derive(Debug, Serialize)]
pub(crate) struct CheckpointActionRequest {
    pub(crate) action: CheckpointAction,
    /// Required for Skip and Override actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
}

/// Store for checkpoints associated with the active project.
#[derive(Debug, Clone, Default)]
pub(crate) struct CheckpointStore {
    pub(crate) checkpoints: Vec<Checkpoint>,
}

impl CheckpointStore {
    /// Count of checkpoints awaiting approval.
    #[must_use]
    pub(crate) fn pending_count(&self) -> usize {
        self.checkpoints
            .iter()
            .filter(|c| c.status == CheckpointStatus::Pending)
            .count()
    }

    /// Checkpoints with pending gates first, then ordered by id.
    #[must_use]
    pub(crate) fn sorted(&self) -> Vec<&Checkpoint> {
        let mut refs: Vec<&Checkpoint> = self.checkpoints.iter().collect();
        refs.sort_by(|a, b| {
            let a_order = u8::from(a.status != CheckpointStatus::Pending);
            let b_order = u8::from(b.status != CheckpointStatus::Pending);
            a_order.cmp(&b_order).then_with(|| a.id.cmp(&b.id))
        });
        refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_checkpoint(id: &str, status: CheckpointStatus) -> Checkpoint {
        Checkpoint {
            id: id.to_string(),
            project_id: "proj1".to_string(),
            title: id.to_string(),
            description: String::new(),
            context: String::new(),
            requirements: vec![],
            artifacts: vec![],
            status,
            decision: None,
        }
    }

    #[test]
    fn pending_count_returns_only_pending() {
        let store = CheckpointStore {
            checkpoints: vec![
                make_checkpoint("a", CheckpointStatus::Pending),
                make_checkpoint("b", CheckpointStatus::Approved),
                make_checkpoint("c", CheckpointStatus::Pending),
                make_checkpoint("d", CheckpointStatus::Skipped),
            ],
        };
        assert_eq!(store.pending_count(), 2);
    }

    #[test]
    fn pending_count_zero_when_empty() {
        assert_eq!(CheckpointStore::default().pending_count(), 0);
    }

    #[test]
    fn sorted_places_pending_first() {
        let store = CheckpointStore {
            checkpoints: vec![
                make_checkpoint("a", CheckpointStatus::Approved),
                make_checkpoint("b", CheckpointStatus::Pending),
                make_checkpoint("c", CheckpointStatus::Skipped),
                make_checkpoint("d", CheckpointStatus::Pending),
            ],
        };
        let sorted = store.sorted();
        assert_eq!(sorted[0].status, CheckpointStatus::Pending);
        assert_eq!(sorted[1].status, CheckpointStatus::Pending);
        assert_ne!(sorted[2].status, CheckpointStatus::Pending);
        assert_ne!(sorted[3].status, CheckpointStatus::Pending);
    }
}
