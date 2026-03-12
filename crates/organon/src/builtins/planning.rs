//! Planning tool executors for dianoia project management.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::workspace::{extract_opt_bool, extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{PlanningService, ToolContext, ToolInput, ToolResult};

#[path = "planning_defs.rs"]
mod defs;
use defs::{
    plan_create_def, plan_discuss_def, plan_execute_def, plan_requirements_def, plan_research_def,
    plan_roadmap_def, plan_status_def, plan_step_complete_def, plan_step_fail_def,
    plan_verify_def,
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
#[path = "planning_tests.rs"]
mod tests;
