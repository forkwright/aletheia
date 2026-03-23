//! Enhanced tool call, approval, and planning state for the desktop chat UI.
//!
//! These types augment the minimal streaming state in [`super::events`] with
//! richer data needed for expandable tool panels, inline approval dialogs,
//! and planning cards.

use theatron_core::id::{PlanId, ToolId, TurnId};

/// Status of a single tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolStatus {
    /// Tool call received but not yet executing.
    Pending,
    /// Tool is currently executing.
    Running,
    /// Tool completed successfully.
    Success,
    /// Tool completed with an error.
    Error,
}

impl ToolStatus {
    /// Whether the tool call has finished (success or error).
    #[must_use]
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Error)
    }
}

/// Rich tool call state for the expandable panel display.
///
/// This extends the minimal [`super::events::ToolCallInfo`] with input/output
/// data needed by the tool panel component.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallState {
    /// Unique identifier for this tool call.
    pub tool_id: ToolId,
    /// Name of the tool.
    pub tool_name: String,
    /// Current execution status.
    pub status: ToolStatus,
    /// Tool input parameters as JSON.
    pub input: Option<serde_json::Value>,
    /// Tool output text on success.
    pub output: Option<String>,
    /// Error message if the tool failed.
    pub error_message: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Risk level for a tool requiring approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RiskLevel {
    /// Low risk: safe, read-only operations.
    Low,
    /// Medium risk: writes to local state.
    Medium,
    /// High risk: destructive or external-facing operations.
    High,
    /// Critical risk: irreversible or security-sensitive operations.
    Critical,
}

impl RiskLevel {
    /// Parse a risk level string from the server.
    #[must_use]
    pub(crate) fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "critical" => Self::Critical,
            _ => Self::Medium,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }
}

/// State for a tool awaiting user approval.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolApprovalState {
    /// Turn that owns this tool call.
    pub turn_id: TurnId,
    /// Unique identifier for this tool call.
    pub tool_id: ToolId,
    /// Name of the tool requesting approval.
    pub tool_name: String,
    /// Tool input parameters.
    pub input: serde_json::Value,
    /// Risk level assigned by the server.
    pub risk: RiskLevel,
    /// Human-readable reason for requiring approval.
    pub reason: String,
    /// Whether the approval has been resolved (approved or denied).
    pub resolved: bool,
}

/// Status of a single plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StepStatus {
    /// Step not yet started.
    Pending,
    /// Step currently executing.
    InProgress,
    /// Step completed successfully.
    Complete,
    /// Step failed.
    Failed,
}

/// State of a single plan step for the planning card display.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanStepState {
    /// Step index within the plan.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Current step status.
    pub status: StepStatus,
    /// Result summary after completion.
    pub result: Option<String>,
}

/// Overall plan status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanStatus {
    /// Plan has been proposed but not started.
    Proposed,
    /// Plan is actively executing steps.
    InProgress,
    /// Plan has completed (carries final status string from server).
    Complete {
        /// Server-provided completion status label.
        status: String,
    },
}

/// State for a planning card within the chat.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanCardState {
    /// Plan identifier.
    pub plan_id: PlanId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStepState>,
    /// Overall plan status.
    pub status: PlanStatus,
}

impl PlanCardState {
    /// Number of completed steps.
    #[must_use]
    pub(crate) fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Complete))
            .count()
    }

    /// Total number of steps.
    #[must_use]
    pub(crate) fn total_steps(&self) -> usize {
        self.steps.len()
    }

    /// Whether all steps have reached a terminal state (complete or failed).
    #[must_use]
    pub(crate) fn is_finished(&self) -> bool {
        matches!(self.status, PlanStatus::Complete { .. })
    }

    /// Progress as a fraction in [0.0, 1.0].
    #[must_use]
    pub(crate) fn progress_fraction(&self) -> f64 {
        let total = self.steps.len();
        if total == 0 {
            return 0.0;
        }
        let done = self.completed_count();
        // SAFETY(numeric): both values are usize; division is exact for small counts.
        #[expect(
            clippy::cast_precision_loss,
            reason = "step counts are small enough that f64 is exact"
        )]
        let fraction = done as f64 / total as f64;
        fraction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_from_str_lossy_known() {
        assert_eq!(RiskLevel::from_str_lossy("low"), RiskLevel::Low);
        assert_eq!(RiskLevel::from_str_lossy("HIGH"), RiskLevel::High);
        assert_eq!(RiskLevel::from_str_lossy("Critical"), RiskLevel::Critical);
        assert_eq!(RiskLevel::from_str_lossy("medium"), RiskLevel::Medium);
    }

    #[test]
    fn risk_level_from_str_lossy_unknown_defaults_to_medium() {
        assert_eq!(RiskLevel::from_str_lossy("unknown"), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_str_lossy(""), RiskLevel::Medium);
    }

    #[test]
    fn risk_level_labels() {
        assert_eq!(RiskLevel::Low.label(), "Low");
        assert_eq!(RiskLevel::Medium.label(), "Medium");
        assert_eq!(RiskLevel::High.label(), "High");
        assert_eq!(RiskLevel::Critical.label(), "Critical");
    }

    #[test]
    fn tool_status_is_terminal() {
        assert!(!ToolStatus::Pending.is_terminal());
        assert!(!ToolStatus::Running.is_terminal());
        assert!(ToolStatus::Success.is_terminal());
        assert!(ToolStatus::Error.is_terminal());
    }

    #[test]
    fn plan_card_progress_empty_steps() {
        let card = PlanCardState {
            plan_id: "p1".into(),
            steps: Vec::new(),
            status: PlanStatus::Proposed,
        };
        assert_eq!(card.completed_count(), 0);
        assert_eq!(card.total_steps(), 0);
        assert!((card.progress_fraction() - 0.0).abs() < f64::EPSILON);
        assert!(!card.is_finished());
    }

    #[test]
    fn plan_card_progress_partial() {
        let card = PlanCardState {
            plan_id: "p1".into(),
            steps: vec![
                PlanStepState {
                    id: 0,
                    label: "step 1".to_string(),
                    status: StepStatus::Complete,
                    result: None,
                },
                PlanStepState {
                    id: 1,
                    label: "step 2".to_string(),
                    status: StepStatus::InProgress,
                    result: None,
                },
                PlanStepState {
                    id: 2,
                    label: "step 3".to_string(),
                    status: StepStatus::Pending,
                    result: None,
                },
            ],
            status: PlanStatus::InProgress,
        };
        assert_eq!(card.completed_count(), 1);
        assert_eq!(card.total_steps(), 3);
        assert!((card.progress_fraction() - 1.0 / 3.0).abs() < 0.01);
        assert!(!card.is_finished());
    }

    #[test]
    fn plan_card_is_finished_when_complete() {
        let card = PlanCardState {
            plan_id: "p1".into(),
            steps: vec![PlanStepState {
                id: 0,
                label: "done".to_string(),
                status: StepStatus::Complete,
                result: None,
            }],
            status: PlanStatus::Complete {
                status: "success".to_string(),
            },
        };
        assert!(card.is_finished());
        assert!((card.progress_fraction() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn step_status_variants_distinct() {
        assert_ne!(StepStatus::Pending, StepStatus::InProgress);
        assert_ne!(StepStatus::Complete, StepStatus::Failed);
    }
}
