use crate::id::{ApiNousId, ApiSessionId, ToolId, TurnId};
use crate::msg::MessageActionKind;

use super::settings::SettingsOverlay;

#[derive(Debug)]
#[non_exhaustive]
pub enum Overlay {
    /// `scroll` is a raw line offset into the flattened keybinding list;
    /// clamped against actual content/viewport height at render time
    /// (`view::overlay::render_help`), mirroring `DiffView`'s pattern (#7221).
    Help {
        scroll: usize,
    },
    AgentPicker {
        cursor: usize,
    },
    SessionPicker(SessionPickerOverlay),
    SystemStatus,
    ContextBudget,
    Settings(SettingsOverlay),
    ToolApproval(ToolApprovalOverlay),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "context action overlay, constructed in tests only"
        )
    )]
    ContextActions(ContextActionsOverlay),
    DiffView(crate::diff::DiffViewState),
    SessionSearch(SessionSearchOverlay),
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
    pub agent_id: ApiNousId,
    pub agent_name: String,
    pub session_id: ApiSessionId,
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
    pub new_session_status: ControlMutationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ControlMutationStatus {
    #[default]
    Idle,
    Pending {
        action_id: String,
    },
    Succeeded {
        action_id: String,
    },
    Failed {
        action_id: String,
        message: String,
    },
}

impl ControlMutationStatus {
    pub(crate) fn pending(action_id: String) -> Self {
        Self::Pending { action_id }
    }

    pub(crate) fn succeeded(action_id: String) -> Self {
        Self::Succeeded { action_id }
    }

    pub(crate) fn failed(action_id: String, message: String) -> Self {
        Self::Failed { action_id, message }
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }
}

#[derive(Debug)]
pub struct ToolApprovalOverlay {
    /// WHY(#7202): required by the session-scoped `POST
    /// /api/v1/sessions/{id}/approvals` route so pylon can verify the
    /// caller's token owns the nous this approval belongs to -- the legacy
    /// per-turn route this overlay used to call has no session id at all,
    /// which is exactly why pylon rejects it outright for any scoped token
    /// (`approvals.rs` `SECURITY(#5340)`). Captured at construction time
    /// from the focused session rather than read back from `App` when the
    /// operator acts, so a session switch while the dialog is open cannot
    /// silently retarget the approval. `None` only if a tool-approval stream
    /// event somehow arrived with no focused session -- structurally
    /// shouldn't happen (the stream belongs to that session's turn), but the
    /// approve/deny action refuses rather than sending a fabricated id.
    pub session_id: Option<ApiSessionId>,
    pub turn_id: TurnId,
    pub tool_id: ToolId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub reason: String,
    pub status: ControlMutationStatus,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn overlay_help_debug() {
        let overlay = Overlay::Help { scroll: 0 };
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
            session_id: Some("s1".into()),
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "write_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test"}),
            risk: "high".to_string(),
            reason: "writes files".to_string(),
            status: ControlMutationStatus::Idle,
        };
        assert_eq!(overlay.tool_name, "write_file");
        assert_eq!(overlay.risk, "high");
    }

    #[test]
    fn control_mutation_status_tracks_pending_and_failure() {
        let pending = ControlMutationStatus::pending("action-1".to_string());
        assert!(pending.is_pending());

        let failed =
            ControlMutationStatus::failed("action-1".to_string(), "request failed".to_string());
        assert!(!failed.is_pending());
        assert!(matches!(
            failed,
            ControlMutationStatus::Failed {
                ref action_id,
                ref message
            } if action_id == "action-1" && message == "request failed"
        ));
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
