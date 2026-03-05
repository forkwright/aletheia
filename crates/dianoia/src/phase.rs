//! Phase types within a project.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::plan::{Plan, PlanState};

/// A phase within a project (e.g., "Foundation", "Core Features").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Unique phase identifier.
    pub id: Ulid,
    /// Human-readable phase name (e.g., "Foundation", "Core Features").
    pub name: String,
    /// What this phase aims to accomplish.
    pub goal: String,
    /// Preconditions that must hold before this phase can begin.
    pub requirements: Vec<String>,
    /// Executable plans within this phase.
    pub plans: Vec<Plan>,
    /// Current lifecycle state.
    pub state: PhaseState,
    /// Ordering position within the project (lower runs first).
    pub order: u32,
}

/// Phase lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseState {
    /// Phase has not started yet.
    Pending,
    /// Phase is active and plans are being prepared.
    Active,
    /// Plans within this phase are being executed.
    Executing,
    /// Execution is done, verifying outcomes.
    Verifying,
    /// All plans completed successfully.
    Complete,
    /// Phase failed, with a flag indicating whether retry is possible.
    Failed { can_retry: bool },
}

impl Phase {
    /// Create a new phase in the `Pending` state.
    #[must_use]
    pub fn new(name: String, goal: String, order: u32) -> Self {
        Self {
            id: Ulid::new(),
            name,
            goal,
            requirements: Vec::new(),
            plans: Vec::new(),
            state: PhaseState::Pending,
            order,
        }
    }

    /// Add an executable plan to this phase.
    pub fn add_plan(&mut self, plan: Plan) {
        self.plans.push(plan);
    }

    /// Whether this phase has reached the `Complete` state.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.state == PhaseState::Complete
    }

    /// Percentage of plans in a terminal state (complete, skipped, failed, stuck).
    #[must_use]
    pub fn completion_percentage(&self) -> f64 {
        if self.plans.is_empty() {
            return 0.0;
        }
        let done = self
            .plans
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    PlanState::Complete | PlanState::Skipped | PlanState::Failed | PlanState::Stuck
                )
            })
            .count();
        #[expect(clippy::cast_precision_loss, reason = "plan counts are small")]
        let pct = done as f64 / self.plans.len() as f64 * 100.0;
        pct
    }
}
