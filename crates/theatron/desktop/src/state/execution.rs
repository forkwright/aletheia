//! Execution state for wave-based plan progress.

use serde::Deserialize;

/// Status of a wave or individual plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum StepStatus {
    /// Not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// Successfully completed.
    Complete,
    /// Execution failed.
    Failed,
    /// Intentionally skipped.
    Skipped,
}

/// Status of a wave in the execution sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum WaveStatus {
    /// Not yet started.
    Pending,
    /// Currently executing.
    Active,
    /// All plans in wave completed.
    Complete,
}

/// A single step within an execution plan.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct PlanStep {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) status: StepStatus,
    /// Step output or result summary.
    #[serde(default)]
    pub(crate) output: Option<String>,
    /// Duration in seconds, if completed.
    #[serde(default)]
    pub(crate) duration_secs: Option<f64>,
    /// Error message, if failed.
    #[serde(default)]
    pub(crate) error: Option<String>,
}

/// An execution plan assigned to an agent within a wave.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ExecutionPlan {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: String,
    /// Agent assigned to this plan.
    #[serde(default)]
    pub(crate) agent_name: String,
    /// Agent status indicator.
    #[serde(default)]
    pub(crate) agent_status: String,
    pub(crate) steps: Vec<PlanStep>,
    /// Elapsed seconds since plan started.
    #[serde(default)]
    pub(crate) elapsed_secs: Option<f64>,
    /// Estimated remaining seconds.
    #[serde(default)]
    pub(crate) estimated_remaining_secs: Option<f64>,
}

impl ExecutionPlan {
    /// Number of steps completed (Complete, Failed, or Skipped).
    #[must_use]
    pub(crate) fn completed_steps(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    StepStatus::Complete | StepStatus::Failed | StepStatus::Skipped
                )
            })
            .count()
    }

    /// Overall step progress as a percentage (0–100).
    #[must_use]
    pub(crate) fn progress_pct(&self) -> u8 {
        if self.steps.is_empty() {
            return 0;
        }
        let done = self.completed_steps();
        ((done * 100) / self.steps.len()).min(100) as u8
    }

    /// Whether any step has failed.
    #[must_use]
    pub(crate) fn has_failure(&self) -> bool {
        self.steps.iter().any(|s| s.status == StepStatus::Failed)
    }
}

/// A wave: a sequential group of parallelizable plans.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Wave {
    pub(crate) wave_number: u32,
    pub(crate) status: WaveStatus,
    pub(crate) plans: Vec<ExecutionPlan>,
    #[serde(default)]
    pub(crate) start_time: Option<String>,
    #[serde(default)]
    pub(crate) end_time: Option<String>,
}

impl Wave {
    /// Wave progress as a percentage across all plans.
    #[must_use]
    pub(crate) fn progress_pct(&self) -> u8 {
        let total_steps: usize = self.plans.iter().map(|p| p.steps.len()).sum();
        if total_steps == 0 {
            return if self.status == WaveStatus::Complete {
                100
            } else {
                0
            };
        }
        let done: usize = self.plans.iter().map(|p| p.completed_steps()).sum();
        ((done * 100) / total_steps).min(100) as u8
    }
}

/// Full execution state for a project.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ExecutionState {
    pub(crate) project_id: String,
    pub(crate) waves: Vec<Wave>,
}

/// Store for execution state of the active project.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExecutionStore {
    pub(crate) state: Option<ExecutionState>,
}

impl ExecutionStore {
    /// Total number of waves.
    #[must_use]
    pub(crate) fn wave_count(&self) -> usize {
        self.state.as_ref().map_or(0, |s| s.waves.len())
    }

    /// The currently active wave number (1-indexed), or None if no wave is active.
    #[must_use]
    pub(crate) fn active_wave_number(&self) -> Option<u32> {
        self.state.as_ref().and_then(|s| {
            s.waves
                .iter()
                .find(|w| w.status == WaveStatus::Active)
                .map(|w| w.wave_number)
        })
    }

    /// Overall execution progress across all waves.
    #[must_use]
    pub(crate) fn overall_progress_pct(&self) -> u8 {
        let Some(ref state) = self.state else {
            return 0;
        };
        let total_steps: usize = state
            .waves
            .iter()
            .flat_map(|w| &w.plans)
            .map(|p| p.steps.len())
            .sum();
        if total_steps == 0 {
            return 0;
        }
        let done: usize = state
            .waves
            .iter()
            .flat_map(|w| &w.plans)
            .map(|p| p.completed_steps())
            .sum();
        ((done * 100) / total_steps).min(100) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_step(id: &str, status: StepStatus) -> PlanStep {
        PlanStep {
            id: id.to_string(),
            description: format!("Step {id}"),
            status,
            output: None,
            duration_secs: None,
            error: None,
        }
    }

    fn make_plan(id: &str, steps: Vec<PlanStep>) -> ExecutionPlan {
        ExecutionPlan {
            id: id.to_string(),
            title: format!("Plan {id}"),
            description: String::new(),
            agent_name: "agent-1".to_string(),
            agent_status: "active".to_string(),
            steps,
            elapsed_secs: None,
            estimated_remaining_secs: None,
        }
    }

    fn make_wave(num: u32, status: WaveStatus, plans: Vec<ExecutionPlan>) -> Wave {
        Wave {
            wave_number: num,
            status,
            plans,
            start_time: None,
            end_time: None,
        }
    }

    #[test]
    fn plan_completed_steps_counts_done_states() {
        let plan = make_plan(
            "p1",
            vec![
                make_step("s1", StepStatus::Complete),
                make_step("s2", StepStatus::Running),
                make_step("s3", StepStatus::Failed),
                make_step("s4", StepStatus::Skipped),
                make_step("s5", StepStatus::Pending),
            ],
        );
        assert_eq!(plan.completed_steps(), 3, "complete + failed + skipped = 3");
    }

    #[test]
    fn plan_progress_pct_calculates_correctly() {
        let plan = make_plan(
            "p1",
            vec![
                make_step("s1", StepStatus::Complete),
                make_step("s2", StepStatus::Complete),
                make_step("s3", StepStatus::Pending),
                make_step("s4", StepStatus::Pending),
            ],
        );
        assert_eq!(plan.progress_pct(), 50);
    }

    #[test]
    fn plan_progress_pct_zero_when_no_steps() {
        let plan = make_plan("p1", vec![]);
        assert_eq!(plan.progress_pct(), 0);
    }

    #[test]
    fn plan_has_failure_detects_failed_step() {
        let plan = make_plan(
            "p1",
            vec![
                make_step("s1", StepStatus::Complete),
                make_step("s2", StepStatus::Failed),
            ],
        );
        assert!(plan.has_failure(), "should detect failed step");
    }

    #[test]
    fn plan_has_failure_false_when_all_ok() {
        let plan = make_plan(
            "p1",
            vec![
                make_step("s1", StepStatus::Complete),
                make_step("s2", StepStatus::Pending),
            ],
        );
        assert!(!plan.has_failure(), "no failure when all ok or pending");
    }

    #[test]
    fn wave_progress_pct_across_plans() {
        let wave = make_wave(
            1,
            WaveStatus::Active,
            vec![
                make_plan(
                    "p1",
                    vec![
                        make_step("s1", StepStatus::Complete),
                        make_step("s2", StepStatus::Complete),
                    ],
                ),
                make_plan(
                    "p2",
                    vec![
                        make_step("s1", StepStatus::Pending),
                        make_step("s2", StepStatus::Pending),
                    ],
                ),
            ],
        );
        // 2 done out of 4 total = 50%
        assert_eq!(wave.progress_pct(), 50);
    }

    #[test]
    fn wave_progress_pct_complete_when_no_steps() {
        let wave = make_wave(1, WaveStatus::Complete, vec![]);
        assert_eq!(
            wave.progress_pct(),
            100,
            "complete wave with no steps = 100%"
        );
    }

    #[test]
    fn store_active_wave_number_finds_active() {
        let store = ExecutionStore {
            state: Some(ExecutionState {
                project_id: "p1".to_string(),
                waves: vec![
                    make_wave(1, WaveStatus::Complete, vec![]),
                    make_wave(2, WaveStatus::Active, vec![]),
                    make_wave(3, WaveStatus::Pending, vec![]),
                ],
            }),
        };
        assert_eq!(store.active_wave_number(), Some(2));
    }

    #[test]
    fn store_active_wave_number_none_when_none_active() {
        let store = ExecutionStore {
            state: Some(ExecutionState {
                project_id: "p1".to_string(),
                waves: vec![
                    make_wave(1, WaveStatus::Complete, vec![]),
                    make_wave(2, WaveStatus::Complete, vec![]),
                ],
            }),
        };
        assert_eq!(store.active_wave_number(), None);
    }

    #[test]
    fn store_overall_progress_across_all_waves() {
        let store = ExecutionStore {
            state: Some(ExecutionState {
                project_id: "p1".to_string(),
                waves: vec![
                    make_wave(
                        1,
                        WaveStatus::Complete,
                        vec![make_plan(
                            "p1",
                            vec![
                                make_step("s1", StepStatus::Complete),
                                make_step("s2", StepStatus::Complete),
                            ],
                        )],
                    ),
                    make_wave(
                        2,
                        WaveStatus::Active,
                        vec![make_plan(
                            "p2",
                            vec![
                                make_step("s1", StepStatus::Complete),
                                make_step("s2", StepStatus::Pending),
                            ],
                        )],
                    ),
                ],
            }),
        };
        // 3 done out of 4 total = 75%
        assert_eq!(store.overall_progress_pct(), 75);
    }

    #[test]
    fn store_overall_progress_zero_when_empty() {
        assert_eq!(ExecutionStore::default().overall_progress_pct(), 0);
    }
}
