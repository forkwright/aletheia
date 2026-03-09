use crate::id::{PlanId, ToolId, TurnId};

use super::settings::SettingsOverlay;

#[non_exhaustive]
#[derive(Debug)]
pub enum Overlay {
    Help,
    AgentPicker { cursor: usize },
    SystemStatus,
    Settings(SettingsOverlay),
    ToolApproval(ToolApprovalOverlay),
    PlanApproval(PlanApprovalOverlay),
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

#[cfg(test)]
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
        if let Overlay::AgentPicker { cursor } = overlay {
            assert_eq!(cursor, 3);
        } else {
            panic!("expected AgentPicker");
        }
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
}
