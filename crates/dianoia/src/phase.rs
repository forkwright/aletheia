//! Phase types within a project.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::plan::{Plan, PlanState};

/// A phase within a project (e.g., "Foundation", "Core Features").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub id: Ulid,
    pub name: String,
    pub goal: String,
    pub requirements: Vec<String>,
    pub plans: Vec<Plan>,
    pub state: PhaseState,
    pub order: u32,
}

/// Phase lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseState {
    Pending,
    Active,
    Executing,
    Verifying,
    Complete,
    Failed { can_retry: bool },
}

impl Phase {
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

    pub fn add_plan(&mut self, plan: Plan) {
        self.plans.push(plan);
    }

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
