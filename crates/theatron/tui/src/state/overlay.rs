use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};
use crate::msg::MessageActionKind;

use super::settings::SettingsOverlay;

#[derive(Debug)]
#[non_exhaustive]
pub enum Overlay {
    Help,
    AgentPicker {
        cursor: usize,
    },
    SessionPicker(SessionPickerOverlay),
    SystemStatus,
    ContextBudget,
    Settings(SettingsOverlay),
    ToolApproval(ToolApprovalOverlay),
    PlanApproval(PlanApprovalOverlay),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "overlay set by action dispatcher; lint fires in lib but not test target"
        )
    )]
    ContextActions(ContextActionsOverlay),
    DiffView(crate::diff::DiffViewState),
    SessionSearch(SessionSearchOverlay),
    DecisionCard(DecisionCardOverlay),
    NotificationHistory {
        scroll: usize,
    },
}

#[derive(Debug)]
pub struct SessionSearchOverlay {
    pub query: String,
    pub cursor: usize,
    pub results: Vec<SearchResult>,
    pub selected: usize,
}

impl SessionSearchOverlay {
    pub(crate) fn new() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub agent_id: NousId,
    pub agent_name: String,
    pub session_id: SessionId,
    pub session_label: String,
    pub snippet: String,
    pub kind: SearchResultKind,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SearchResultKind {
    SessionName,
    MessageContent { role: String },
}

#[derive(Debug)]
pub struct ContextActionsOverlay {
    pub actions: Vec<ContextAction>,
    pub cursor: usize,
}

impl ContextActionsOverlay {
    pub(crate) fn selected_action(&self) -> Option<&ContextAction> {
        self.actions.get(self.cursor)
    }
}

#[derive(Debug, Clone)]
pub struct ContextAction {
    pub label: &'static str,
    pub kind: MessageActionKind,
}

#[derive(Debug)]
pub struct SessionPickerOverlay {
    pub cursor: usize,
    pub show_archived: bool,
}

#[derive(Debug)]
pub struct ToolApprovalOverlay {
    pub turn_id: TurnId,
    pub tool_id: ToolId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct PlanApprovalOverlay {
    pub plan_id: PlanId,
    pub steps: Vec<PlanStepApproval>,
    pub total_cost_cents: u32,
    pub cursor: usize,
}

#[derive(Debug)]
pub struct PlanStepApproval {
    pub id: u32,
    pub label: String,
    pub role: String,
    pub checked: bool,
}

/// Which field has keyboard focus in the decision card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DecisionField {
    #[default]
    Options,
    CustomAnswer,
    Notes,
}

/// A single presented option in a decision card.
#[derive(Debug, Clone)]
pub struct DecisionOption {
    pub label: String,
    pub description: Option<String>,
    pub is_recommendation: bool,
}

/// Decision card overlay: agent presents structured choices for the user.
#[derive(Debug)]
pub struct DecisionCardOverlay {
    pub question: String,
    pub options: Vec<DecisionOption>,
    pub cursor: usize,
    pub custom_answer: String,
    pub custom_cursor: usize,
    pub notes: String,
    pub notes_cursor: usize,
    pub focused_field: DecisionField,
}

impl DecisionCardOverlay {
    pub(crate) fn new(question: String, options: Vec<DecisionOption>) -> Self {
        Self {
            question,
            options,
            cursor: 0,
            custom_answer: String::new(),
            custom_cursor: 0,
            notes: String::new(),
            notes_cursor: 0,
            focused_field: DecisionField::Options,
        }
    }

    pub(crate) fn chosen_label(&self) -> &str {
        if !self.custom_answer.is_empty() {
            &self.custom_answer
        } else {
            self.options
                .get(self.cursor)
                .map(|o| o.label.as_str())
                .unwrap_or("")
        }
    }

    pub(crate) fn next_field(&mut self) {
        self.focused_field = match self.focused_field {
            DecisionField::Options => DecisionField::CustomAnswer,
            DecisionField::CustomAnswer => DecisionField::Notes,
            DecisionField::Notes => DecisionField::Options,
        };
    }

    pub(crate) fn prev_field(&mut self) {
        self.focused_field = match self.focused_field {
            DecisionField::Options => DecisionField::Notes,
            DecisionField::CustomAnswer => DecisionField::Options,
            DecisionField::Notes => DecisionField::CustomAnswer,
        };
    }
}

/// A decision that has been submitted by the user, stored for fact-card rendering.
#[derive(Debug, Clone)]
pub struct SubmittedDecision {
    pub question: String,
    pub chosen_label: String,
    pub notes: String,
    #[expect(
        dead_code,
        reason = "planned TUI feature: used for future expiry/age display"
    )]
    pub submitted_at: std::time::Instant,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn overlay_help_debug() {
        let overlay = Overlay::Help;
        let debug = format!("{:?}", overlay);
        assert!(debug.contains("Help"));
    }

    #[test]
    fn overlay_agent_picker_has_cursor() {
        let overlay = Overlay::AgentPicker { cursor: 3 };
        let Overlay::AgentPicker { cursor } = overlay else {
            unreachable!("expected AgentPicker");
        };
        assert_eq!(cursor, 3);
    }

    #[test]
    fn tool_approval_overlay_fields() {
        let overlay = ToolApprovalOverlay {
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "write_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test"}),
            risk: "high".to_string(),
            reason: "writes files".to_string(),
        };
        assert_eq!(overlay.tool_name, "write_file");
        assert_eq!(overlay.risk, "high");
    }

    #[test]
    fn plan_approval_overlay_fields() {
        let overlay = PlanApprovalOverlay {
            plan_id: "p1".into(),
            steps: vec![PlanStepApproval {
                id: 1,
                label: "Step 1".to_string(),
                role: "analyst".to_string(),
                checked: true,
            }],
            total_cost_cents: 100,
            cursor: 0,
        };
        assert_eq!(overlay.steps.len(), 1);
        assert!(overlay.steps[0].checked);
        assert_eq!(overlay.total_cost_cents, 100);
    }

    #[test]
    fn context_actions_overlay_selected_action() {
        let overlay = ContextActionsOverlay {
            actions: vec![
                ContextAction {
                    label: "Copy text",
                    kind: MessageActionKind::Copy,
                },
                ContextAction {
                    label: "Quote in reply",
                    kind: MessageActionKind::QuoteInReply,
                },
            ],
            cursor: 1,
        };
        let selected = overlay.selected_action().unwrap();
        assert_eq!(selected.kind, MessageActionKind::QuoteInReply);
        assert_eq!(selected.label, "Quote in reply");
    }

    #[test]
    fn context_actions_overlay_empty_returns_none() {
        let overlay = ContextActionsOverlay {
            actions: vec![],
            cursor: 0,
        };
        assert!(overlay.selected_action().is_none());
    }
}
