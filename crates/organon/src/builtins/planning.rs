//! Planning tool executors for dianoia project management.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use super::workspace::{extract_opt_bool, extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PlanningService, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

fn require_planning(
    ctx: &ToolContext,
) -> std::result::Result<&Arc<dyn PlanningService>, ToolResult> {
    ctx.services
        .as_deref()
        .and_then(|s| s.planning.as_ref())
        .ok_or_else(|| ToolResult::error("planning service not configured"))
}

// --- Executors ---

struct PlanCreateExecutor;

impl ToolExecutor for PlanCreateExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let name = extract_str(&input.arguments, "name", &input.name)?;
            let description = extract_str(&input.arguments, "description", &input.name)?;
            let scope = input.arguments.get("scope").and_then(|v| v.as_str());
            let mode = input
                .arguments
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("full");
            #[expect(
                clippy::cast_possible_truncation,
                reason = "appetite_minutes fits in u32"
            )]
            let appetite_minutes =
                extract_opt_u64(&input.arguments, "appetite_minutes").map(|v| v as u32);

            match planning
                .create_project(
                    name,
                    description,
                    scope,
                    mode,
                    appetite_minutes,
                    ctx.nous_id.as_str(),
                )
                .await
            {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanResearchExecutor;

impl ToolExecutor for PlanResearchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let skip = extract_opt_bool(&input.arguments, "skip").unwrap_or(false);

            let transition = if skip {
                "skip_research"
            } else {
                "start_research"
            };
            match planning.transition_project(project_id, transition).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanRequirementsExecutor;

impl ToolExecutor for PlanRequirementsExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let action = extract_str(&input.arguments, "action", &input.name)?;

            let transition = match action {
                "start_scoping" => "start_scoping",
                "complete" => "start_planning",
                other => return Ok(ToolResult::error(format!("unknown action: {other}"))),
            };
            match planning.transition_project(project_id, transition).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanRoadmapExecutor;

impl ToolExecutor for PlanRoadmapExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let action = extract_str(&input.arguments, "action", &input.name)?;

            match action {
                "add_phase" => {
                    let phase_name = extract_str(&input.arguments, "phase_name", &input.name)?;
                    let phase_goal = extract_str(&input.arguments, "phase_goal", &input.name)?;
                    match planning.add_phase(project_id, phase_name, phase_goal).await {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e)),
                    }
                }
                "start_discussion" => {
                    match planning
                        .transition_project(project_id, "start_discussion")
                        .await
                    {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e)),
                    }
                }
                "start_execution" => {
                    match planning
                        .transition_project(project_id, "start_execution")
                        .await
                    {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e)),
                    }
                }
                other => Ok(ToolResult::error(format!("unknown action: {other}"))),
            }
        })
    }
}

struct PlanDiscussExecutor;

impl ToolExecutor for PlanDiscussExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let action = extract_str(&input.arguments, "action", &input.name)?;

            let transition = match action {
                "complete" => "start_execution",
                other => return Ok(ToolResult::error(format!("unknown action: {other}"))),
            };
            match planning.transition_project(project_id, transition).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanExecuteExecutor;

impl ToolExecutor for PlanExecuteExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let action = extract_str(&input.arguments, "action", &input.name)?;

            let transition = match action {
                "start" => "start_execution",
                "pause" => "pause",
                "resume" => "resume",
                "abandon" => "abandon",
                "start_verification" => "start_verification",
                other => return Ok(ToolResult::error(format!("unknown action: {other}"))),
            };
            match planning.transition_project(project_id, transition).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanVerifyExecutor;

impl ToolExecutor for PlanVerifyExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let action = extract_str(&input.arguments, "action", &input.name)?;

            let transition = match action {
                "complete" => "complete",
                "revert" => {
                    let revert_to = extract_str(&input.arguments, "revert_to", &input.name)?;
                    match revert_to {
                        "scoping" => "revert_to_scoping",
                        "planning" => "revert_to_planning",
                        "executing" => "revert_to_executing",
                        other => {
                            return Ok(ToolResult::error(format!(
                                "invalid revert target: {other}"
                            )));
                        }
                    }
                }
                other => return Ok(ToolResult::error(format!("unknown action: {other}"))),
            };
            match planning.transition_project(project_id, transition).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanStatusExecutor;

impl ToolExecutor for PlanStatusExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;

            match planning.load_project(project_id).await {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanStepCompleteExecutor;

impl ToolExecutor for PlanStepCompleteExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let phase_id = extract_str(&input.arguments, "phase_id", &input.name)?;
            let plan_id = extract_str(&input.arguments, "plan_id", &input.name)?;
            let achievement = input.arguments.get("achievement").and_then(|v| v.as_str());

            match planning
                .complete_plan(project_id, phase_id, plan_id, achievement)
                .await
            {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

struct PlanStepFailExecutor;

impl ToolExecutor for PlanStepFailExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let planning = match require_planning(ctx) {
                Ok(p) => p,
                Err(r) => return Ok(r),
            };
            let project_id = extract_str(&input.arguments, "project_id", &input.name)?;
            let phase_id = extract_str(&input.arguments, "phase_id", &input.name)?;
            let plan_id = extract_str(&input.arguments, "plan_id", &input.name)?;
            let reason = extract_str(&input.arguments, "reason", &input.name)?;

            match planning
                .fail_plan(project_id, phase_id, plan_id, reason)
                .await
            {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// --- Tool Definitions ---

fn plan_create_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_create").expect("valid tool name"),
        description: "Create a new planning project with phases and plans".to_owned(),
        extended_description: Some(
            "Creates a multi-phase planning project. Modes: 'full' (research through verification), \
             'quick' (time-boxed task with appetite_minutes), 'background' (autonomous processing)."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "name".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project name".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "What this project aims to accomplish".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional scope constraint (e.g., 'crate X only')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "mode".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Planning mode".to_owned(),
                        enum_values: Some(vec![
                            "full".to_owned(),
                            "quick".to_owned(),
                            "background".to_owned(),
                        ]),
                        default: Some(serde_json::json!("full")),
                    },
                ),
                (
                    "appetite_minutes".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Time budget in minutes (only for 'quick' mode)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["name".to_owned(), "description".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_research_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_research").expect("valid tool name"),
        description: "Advance project to research phase or skip research".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "skip".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Skip research and go directly to scoping".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_requirements_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_requirements").expect("valid tool name"),
        description: "Manage requirements scoping phase".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["start_scoping".to_owned(), "complete".to_owned()]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_roadmap_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_roadmap").expect("valid tool name"),
        description: "Manage project roadmap: add phases, start discussion or execution".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec![
                            "add_phase".to_owned(),
                            "start_discussion".to_owned(),
                            "start_execution".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "phase_name".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase name (required for add_phase)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_goal".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase goal (required for add_phase)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_discuss_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_discuss").expect("valid tool name"),
        description: "Complete discussion phase and advance to execution".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned()]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_execute_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_execute").expect("valid tool name"),
        description: "Manage plan execution: start, pause, resume, abandon, or verify".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec![
                            "start".to_owned(),
                            "pause".to_owned(),
                            "resume".to_owned(),
                            "abandon".to_owned(),
                            "start_verification".to_owned(),
                        ]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_verify_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_verify").expect("valid tool name"),
        description: "Complete verification or revert to an earlier phase".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned(), "revert".to_owned()]),
                        default: None,
                    },
                ),
                (
                    "revert_to".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target state for revert (required when action is 'revert')"
                            .to_owned(),
                        enum_values: Some(vec![
                            "scoping".to_owned(),
                            "planning".to_owned(),
                            "executing".to_owned(),
                        ]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_status_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_status").expect("valid tool name"),
        description: "Get current project status including phases and completion".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "project_id".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Project ID".to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_step_complete_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_step_complete").expect("valid tool name"),
        description: "Mark a plan step as successfully completed".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark complete".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "achievement".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Description of what was accomplished".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "project_id".to_owned(),
                "phase_id".to_owned(),
                "plan_id".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

fn plan_step_fail_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_step_fail").expect("valid tool name"),
        description: "Mark a plan step as failed with a reason".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark failed".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "reason".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Why the plan failed".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "project_id".to_owned(),
                "phase_id".to_owned(),
                "plan_id".to_owned(),
                "reason".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

// --- Registration ---

pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(plan_create_def(), Box::new(PlanCreateExecutor))?;
    registry.register(plan_research_def(), Box::new(PlanResearchExecutor))?;
    registry.register(plan_requirements_def(), Box::new(PlanRequirementsExecutor))?;
    registry.register(plan_roadmap_def(), Box::new(PlanRoadmapExecutor))?;
    registry.register(plan_discuss_def(), Box::new(PlanDiscussExecutor))?;
    registry.register(plan_execute_def(), Box::new(PlanExecuteExecutor))?;
    registry.register(plan_verify_def(), Box::new(PlanVerifyExecutor))?;
    registry.register(plan_status_def(), Box::new(PlanStatusExecutor))?;
    registry.register(plan_step_complete_def(), Box::new(PlanStepCompleteExecutor))?;
    registry.register(plan_step_fail_def(), Box::new(PlanStepFailExecutor))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{PlanningService, ToolCategory, ToolContext, ToolInput, ToolServices};

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn test_ctx_with_planning(planning: Arc<dyn PlanningService>) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: Some(planning),
                knowledge: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: vec![],
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[derive(Default)]
    struct MockPlanning {
        create_result: Mutex<Option<Result<String, String>>>,
        load_result: Mutex<Option<Result<String, String>>>,
        transition_calls: Mutex<Vec<(String, String)>>,
        transition_result: Mutex<Option<Result<String, String>>>,
        add_phase_calls: Mutex<Vec<(String, String, String)>>,
        add_phase_result: Mutex<Option<Result<String, String>>>,
        complete_plan_calls: Mutex<Vec<(String, String, String)>>,
        complete_plan_result: Mutex<Option<Result<String, String>>>,
        fail_plan_calls: Mutex<Vec<(String, String, String, String)>>,
        fail_plan_result: Mutex<Option<Result<String, String>>>,
    }

    impl PlanningService for MockPlanning {
        fn create_project(
            &self,
            _name: &str,
            _description: &str,
            _scope: Option<&str>,
            _mode: &str,
            _appetite_minutes: Option<u32>,
            _owner: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            let result = self.create_result.lock().unwrap().take().unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","name":"test","state":"Created"}"#.to_owned(),
            ));
            Box::pin(async move { result })
        }

        fn load_project(
            &self,
            _project_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            let result = self.load_result.lock().unwrap().take().unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","state":"Created"}"#.to_owned(),
            ));
            Box::pin(async move { result })
        }

        fn transition_project(
            &self,
            project_id: &str,
            transition: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            self.transition_calls
                .lock()
                .unwrap()
                .push((project_id.to_owned(), transition.to_owned()));
            let result = self.transition_result.lock().unwrap().take().unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","state":"Researching"}"#.to_owned(),
            ));
            Box::pin(async move { result })
        }

        fn add_phase(
            &self,
            project_id: &str,
            name: &str,
            goal: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            self.add_phase_calls.lock().unwrap().push((
                project_id.to_owned(),
                name.to_owned(),
                goal.to_owned(),
            ));
            let result = self.add_phase_result.lock().unwrap().take().unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","phases":[{"name":"Phase 1"}]}"#.to_owned(),
            ));
            Box::pin(async move { result })
        }

        fn complete_plan(
            &self,
            project_id: &str,
            phase_id: &str,
            plan_id: &str,
            _achievement: Option<&str>,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            self.complete_plan_calls.lock().unwrap().push((
                project_id.to_owned(),
                phase_id.to_owned(),
                plan_id.to_owned(),
            ));
            let result = self
                .complete_plan_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(r#"{"status":"plan completed"}"#.to_owned()));
            Box::pin(async move { result })
        }

        fn fail_plan(
            &self,
            project_id: &str,
            phase_id: &str,
            plan_id: &str,
            reason: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            self.fail_plan_calls.lock().unwrap().push((
                project_id.to_owned(),
                phase_id.to_owned(),
                plan_id.to_owned(),
                reason.to_owned(),
            ));
            let result = self
                .fail_plan_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(r#"{"status":"plan failed"}"#.to_owned()));
            Box::pin(async move { result })
        }

        fn list_projects(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            Box::pin(async { Ok("[]".to_owned()) })
        }
    }

    #[tokio::test]
    async fn register_planning_tools() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let planning_tools = reg.definitions_for_category(ToolCategory::Planning);
        assert_eq!(planning_tools.len(), 10);
    }

    #[tokio::test]
    async fn all_tools_are_lazy() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        for def in reg.definitions_for_category(ToolCategory::Planning) {
            assert!(!def.auto_activate, "{} should be lazy", def.name.as_str());
        }
    }

    #[tokio::test]
    async fn plan_create_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_create").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"name": "test", "description": "test project"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not configured"));
    }

    #[tokio::test]
    async fn plan_create_success() {
        let mock = Arc::new(MockPlanning::default());
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_create").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"name": "my project", "description": "build a thing"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("Created"));
    }

    #[tokio::test]
    async fn plan_create_error_propagates() {
        let mock = Arc::new(MockPlanning::default());
        *mock.create_result.lock().unwrap() = Some(Err("project already exists".to_owned()));
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_create").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"name": "test", "description": "test"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("already exists"));
    }

    #[tokio::test]
    async fn plan_research_skip_dispatches_correctly() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_research").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"project_id": "01J0000000000000000000000", "skip": true}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.transition_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "skip_research");
    }

    #[tokio::test]
    async fn plan_research_no_skip_dispatches_start() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_research").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"project_id": "01J0000000000000000000000"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.transition_calls.lock().unwrap();
        assert_eq!(calls[0].1, "start_research");
    }

    #[tokio::test]
    async fn plan_roadmap_add_phase_calls_add_phase() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_roadmap").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "project_id": "01J0000000000000000000000",
                "action": "add_phase",
                "phase_name": "Foundation",
                "phase_goal": "Set up core infrastructure"
            }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.add_phase_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "Foundation");
        assert_eq!(calls[0].2, "Set up core infrastructure");

        // transition_project should NOT have been called
        let t_calls = mock_ref.transition_calls.lock().unwrap();
        assert!(t_calls.is_empty());
    }

    #[tokio::test]
    async fn plan_status_returns_project_json() {
        let mock = Arc::new(MockPlanning::default());
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_status").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"project_id": "01J0000000000000000000000"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("Created"));
    }

    #[tokio::test]
    async fn plan_step_complete_dispatches() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_step_complete").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "project_id": "proj1",
                "phase_id": "phase1",
                "plan_id": "plan1",
                "achievement": "implemented the feature"
            }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.complete_plan_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ("proj1".to_owned(), "phase1".to_owned(), "plan1".to_owned())
        );
    }

    #[tokio::test]
    async fn plan_step_fail_dispatches() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_step_fail").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "project_id": "proj1",
                "phase_id": "phase1",
                "plan_id": "plan1",
                "reason": "compilation error"
            }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.fail_plan_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].3, "compilation error");
    }

    #[tokio::test]
    async fn plan_verify_revert_dispatches_correctly() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_verify").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "project_id": "proj1",
                "action": "revert",
                "revert_to": "planning"
            }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = mock_ref.transition_calls.lock().unwrap();
        assert_eq!(calls[0].1, "revert_to_planning");
    }

    #[tokio::test]
    async fn plan_execute_maps_actions() {
        let mock = Arc::new(MockPlanning::default());
        let mock_ref = Arc::clone(&mock);
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");

        for (action, expected_transition) in [
            ("start", "start_execution"),
            ("pause", "pause"),
            ("resume", "resume"),
            ("abandon", "abandon"),
            ("start_verification", "start_verification"),
        ] {
            // Reset transition result for each iteration
            *mock_ref.transition_result.lock().unwrap() = Some(Ok(r#"{"state":"ok"}"#.to_owned()));

            let input = ToolInput {
                name: ToolName::new("plan_execute").expect("valid"),
                tool_use_id: "tu_1".to_owned(),
                arguments: serde_json::json!({
                    "project_id": "proj1",
                    "action": action,
                }),
            };
            reg.execute(&input, &ctx).await.expect("execute");

            let calls = mock_ref.transition_calls.lock().unwrap();
            let last = calls.last().expect("should have a call");
            assert_eq!(
                last.1, expected_transition,
                "action '{action}' should map to '{expected_transition}'"
            );
        }
    }

    #[tokio::test]
    async fn unknown_action_returns_error() {
        let mock = Arc::new(MockPlanning::default());
        let ctx = test_ctx_with_planning(mock);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("plan_requirements").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"project_id": "p1", "action": "invalid_action"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("unknown action"));
    }
}
