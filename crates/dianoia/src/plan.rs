//! Executable plans within a phase.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::{self, Result};

const DEFAULT_MAX_ITERATIONS: u32 = 10;

/// An executable plan within a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Ulid,
    pub title: String,
    pub description: String,
    pub wave: u32,
    pub depends_on: Vec<Ulid>,
    pub state: PlanState,
    pub max_iterations: u32,
    pub iterations: u32,
    pub blockers: Vec<Blocker>,
    pub achievements: Vec<String>,
}

/// Plan lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanState {
    Pending,
    Ready,
    Executing,
    Complete,
    Failed,
    Skipped,
    Stuck,
}

/// A blocker discovered during plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    pub description: String,
    pub plan_id: Ulid,
    pub detected_at: jiff::Timestamp,
}

impl Plan {
    /// Create a new plan with a title, description, and wave number.
    #[must_use]
    pub fn new(title: String, description: String, wave: u32) -> Self {
        Self {
            id: Ulid::new(),
            title,
            description,
            wave,
            depends_on: Vec::new(),
            state: PlanState::Pending,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            iterations: 0,
            blockers: Vec::new(),
            achievements: Vec::new(),
        }
    }

    /// Check if all dependencies are satisfied given completed plan IDs.
    #[must_use]
    pub fn is_ready(&self, completed: &[Ulid]) -> bool {
        self.depends_on.iter().all(|dep| completed.contains(dep))
    }

    /// Record an iteration.
    ///
    /// # Errors
    ///
    /// Returns a stuck-plan error if the iteration count exceeds `max_iterations`.
    pub fn record_iteration(&mut self) -> Result<()> {
        self.iterations += 1;
        if self.iterations > self.max_iterations {
            self.state = PlanState::Stuck;
            return error::PlanStuckSnafu {
                plan_id: self.id.to_string(),
                iterations: self.iterations,
            }
            .fail();
        }
        Ok(())
    }

    /// Mark as stuck with a blocker.
    pub fn mark_stuck(&mut self, blocker: Blocker) {
        self.state = PlanState::Stuck;
        self.blockers.push(blocker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_check() {
        let dep1 = Ulid::new();
        let dep2 = Ulid::new();
        let mut plan = Plan::new("task".into(), "do a thing".into(), 1);
        plan.depends_on = vec![dep1, dep2];

        assert!(!plan.is_ready(&[]));
        assert!(!plan.is_ready(&[dep1]));
        assert!(plan.is_ready(&[dep1, dep2]));
        assert!(plan.is_ready(&[dep2, dep1, Ulid::new()]));
    }

    #[test]
    fn iteration_tracking_and_stuck() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        plan.max_iterations = 3;

        plan.record_iteration().unwrap(); // 1
        plan.record_iteration().unwrap(); // 2
        plan.record_iteration().unwrap(); // 3
        let result = plan.record_iteration(); // 4 > 3
        assert!(result.is_err());
        assert_eq!(plan.state, PlanState::Stuck);
    }

    #[test]
    fn wave_ordering() {
        let first = Plan::new("first".into(), "wave 1".into(), 1);
        let second = Plan::new("second".into(), "wave 1".into(), 1);
        let mut dependent = Plan::new("dependent".into(), "wave 2".into(), 2);
        dependent.depends_on = vec![first.id, second.id];

        // Wave 1 plans have no deps — always ready
        assert!(first.is_ready(&[]));
        assert!(second.is_ready(&[]));

        // Wave 2 not ready until wave 1 complete
        assert!(!dependent.is_ready(&[]));
        assert!(!dependent.is_ready(&[first.id]));
        assert!(dependent.is_ready(&[first.id, second.id]));
    }
}
