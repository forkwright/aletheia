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
