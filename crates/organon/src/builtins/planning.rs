//! Planning tool executors for dianoia project management.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::workspace::{extract_opt_bool, extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    PlanningPlanInput, PlanningService, RollbackSupport, ToolCapabilityMetadata, ToolContext,
    ToolInput, ToolResult, ToolStability,
};

#[path = "planning_defs.rs"]
mod defs;
use defs::{
    plan_create_def, plan_discuss_def, plan_execute_def, plan_requirements_def, plan_research_def,
    plan_roadmap_def, plan_status_def, plan_step_complete_def, plan_step_fail_def,
    plan_verify_criteria_def, plan_verify_def,
};

#[expect(
    clippy::result_large_err,
    reason = "ToolResult grew by receipt field; boxing would change public API"
)]
fn require_planning(
    ctx: &ToolContext,
) -> std::result::Result<&Arc<dyn PlanningService>, ToolResult> {
    ctx.services
        .as_deref()
        .and_then(|s| s.planning.as_ref())
        .ok_or_else(|| ToolResult::error("planning service not configured"))
}

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
            let appetite_minutes = extract_opt_u64(&input.arguments, "appetite_minutes")
                .map(|v| u32::try_from(v).unwrap_or(u32::MAX));

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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                        Err(e) => Ok(ToolResult::error(e.to_string())),
                    }
                }
                "add_plan" => {
                    let phase_id = extract_str(&input.arguments, "phase_id", &input.name)?;
                    let title = extract_str(&input.arguments, "plan_title", &input.name)?;
                    let description =
                        extract_str(&input.arguments, "plan_description", &input.name)?;
                    let wave = extract_opt_u64(&input.arguments, "wave")
                        .map_or(1, |v| u32::try_from(v).unwrap_or(u32::MAX));
                    let depends_on = input
                        .arguments
                        .get("depends_on")
                        .and_then(serde_json::Value::as_array)
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let max_iterations = extract_opt_u64(&input.arguments, "max_iterations")
                        .map(|v| u32::try_from(v).unwrap_or(u32::MAX));
                    match planning
                        .add_plan(
                            project_id,
                            phase_id,
                            PlanningPlanInput {
                                title,
                                description,
                                wave,
                                depends_on: &depends_on,
                                max_iterations,
                            },
                        )
                        .await
                    {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e.to_string())),
                    }
                }
                "start_discussion" => {
                    match planning
                        .transition_project(project_id, "start_discussion")
                        .await
                    {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e.to_string())),
                    }
                }
                "start_execution" => {
                    match planning
                        .transition_project(project_id, "start_execution")
                        .await
                    {
                        Ok(json) => Ok(ToolResult::text(json)),
                        Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

struct PlanVerifyCriteriaExecutor;

impl ToolExecutor for PlanVerifyCriteriaExecutor {
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
            let criteria = extract_str(&input.arguments, "criteria", &input.name)?;

            if let Some(refusal) = unverifiable_met_criteria(criteria) {
                return Ok(ToolResult::error(refusal));
            }

            match planning
                .verify_criteria(project_id, phase_id, criteria)
                .await
            {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

/// Refuse a criterion marked `met` whose text cannot be verified by evidence.
///
/// WHY(#6742) this tool and not a docs lint: its stated job is to verify success
/// criteria *with evidence*, and "the system is robust" admits no evidence. Marking
/// such a criterion `met` is a false capability claim, which is the one thing this
/// tool exists to prevent. `aletheia-lexica::adjectives` encoded exactly that
/// vocabulary and had no consumer anywhere in the workspace -- the list was built for
/// a check nobody wired, and this is that check.
///
/// WHY only `met`: writing an aspirational criterion is legitimate, and so is
/// reporting one as `not-met` or `partially-met` -- those are statements of intent or
/// of shortfall. Only `met` asserts that verification happened. Refusing the others
/// would block honest reporting of an inherited criterion, which would push callers
/// to reword rather than to measure.
///
/// Returns the refusal message, or None when nothing is being falsely certified. An
/// unparseable `criteria` payload returns None: this guard is not the JSON validator,
/// and reporting a parse error here would send the caller to the wrong problem.
fn unverifiable_met_criteria(criteria: &str) -> Option<String> {
    let parsed: Vec<serde_json::Value> = serde_json::from_str(criteria).ok()?;
    let mut offences = Vec::new();
    for entry in &parsed {
        if entry.get("status").and_then(serde_json::Value::as_str) != Some("met") {
            continue;
        }
        let Some(text) = entry.get("criterion").and_then(serde_json::Value::as_str) else {
            continue;
        };
        for adjective in aletheia_lexica::adjectives::UNFALSIFIABLE_ADJECTIVES {
            if contains_word(text, adjective) {
                offences.push(format!("  \"{text}\" -- \"{adjective}\""));
                break;
            }
        }
    }
    if offences.is_empty() {
        return None;
    }
    Some(format!(
        "cannot mark a criterion `met` when its text is not verifiable by evidence:\n{}\n\n         These adjectives assert a quality with no measurement attached, so no evidence \
         can establish them and none can refute them. Restate each as the thing you \
         actually measured -- a number, a threshold, an observed behaviour -- or report \
         it as `partially-met` or `not-met`, which are honest about an unmeasured claim.",
        offences.join("\n")
    ))
}

/// Whether `needle` appears in `haystack` as a whole word, case-insensitively.
///
/// WHY tokenise rather than search for a substring: "scalable" is a substring of
/// "unscalable", which asserts the opposite, so a `contains` would refuse a criterion
/// for saying the honest thing.
///
/// WHY the splitter keeps `-`: the vocabulary has hyphenated entries like
/// "world-class". Splitting on every non-alphanumeric would break that into two
/// tokens and match the "class" of an unrelated phrase; a regex `\b` has the same
/// problem, since it treats a hyphen as a boundary.
///
/// WHY no byte indexing: the earlier version walked `find` offsets and sliced around
/// them, which panics when an offset lands inside a multi-byte character. Criteria
/// text is arbitrary input, and `to_lowercase` can change a string's byte length.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let need = needle.to_lowercase();
    haystack
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .any(|token| token.trim_matches('-') == need)
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

/// Register planning tools into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
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
    registry.register(
        plan_verify_criteria_def(),
        Box::new(PlanVerifyCriteriaExecutor),
    )?;
    declare_capabilities(registry)?;
    Ok(())
}

/// Governance metadata for the planning tools.
///
/// Split out of [`register`] (rather than interleaved per-tool) to keep that
/// function under clippy's `too_many_lines` threshold; `declare_capability`
/// rejects an unregistered name, so the only ordering requirement is
/// that this runs after the `registry.register` calls above. All planning
/// mutations flow through the `PlanningService` trait to the filesystem-backed
/// project workspace (persisted project state files), which has no general
/// undo mechanism.
fn declare_capabilities(registry: &mut ToolRegistry) -> Result<()> {
    let declare = |registry: &mut ToolRegistry, name: &'static str, rollback: RollbackSupport| {
        registry.declare_capability(
            koina::id::ToolName::from_static(name),
            ToolCapabilityMetadata {
                owner: "organon::builtins::planning".to_owned(),
                stability: ToolStability::Stable,
                rollback,
                ..ToolCapabilityMetadata::default()
            },
        )
    };
    declare(
        registry,
        "plan_create",
        RollbackSupport::Unsupported {
            reason: "creates a project workspace directory and persisted state files; no \
                     delete path exists through this tool"
                .to_owned(),
        },
    )?;
    // WHY a loop: plan_research, plan_requirements, and plan_discuss are all
    // thin lifecycle-transition executors with identical rollback semantics;
    // one shared declaration keeps this function under clippy's
    // `too_many_lines` threshold.
    for name in ["plan_research", "plan_requirements", "plan_discuss"] {
        declare(
            registry,
            name,
            RollbackSupport::PartialSupport {
                reason: "persists a lifecycle transition to the project state files; in-band \
                         inverse transitions exist only out of the verifying state \
                         (plan_verify's revert_to_*)"
                    .to_owned(),
            },
        )?;
    }
    declare(
        registry,
        "plan_roadmap",
        RollbackSupport::PartialSupport {
            reason: "add_phase/add_plan append records to the persisted project workspace \
                     with no removal path; the lifecycle actions share the limited \
                     revert_to_* inverses"
                .to_owned(),
        },
    )?;
    declare(
        registry,
        "plan_execute",
        RollbackSupport::PartialSupport {
            reason: "pause and resume are mutual inverses, but abandon and \
                     start_verification move the persisted lifecycle to states with no \
                     inverse transition"
                .to_owned(),
        },
    )?;
    declare(
        registry,
        "plan_verify",
        RollbackSupport::PartialSupport {
            reason: "revert_to_* restores an earlier lifecycle state, but complete is \
                     terminal; every transition is persisted to the project state files"
                .to_owned(),
        },
    )?;
    declare(registry, "plan_status", RollbackSupport::Supported)?;
    declare(
        registry,
        "plan_step_complete",
        RollbackSupport::Unsupported {
            reason: "marks a plan complete in the persisted project state; no un-complete \
                     transition exists"
                .to_owned(),
        },
    )?;
    declare(
        registry,
        "plan_step_fail",
        RollbackSupport::Unsupported {
            reason: "records a plan failure with its reason in the persisted project state; \
                     no inverse transition exists"
                .to_owned(),
        },
    )?;
    declare(registry, "plan_verify_criteria", RollbackSupport::Supported)?;
    Ok(())
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;
