//! Dispatch tool (dromeus — δρομεύς, runner).
//!
//! Executes dispatch specs via the orchestrator: runs prompt groups in parallel
//! or sequential order, spawning agent sessions per prompt.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use aletheia_energeia::orchestrator::Orchestrator;
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::shared::{opt_bool, opt_u64, require_str, to_json_text};

// ── dromeus (δρομεύς — runner) ─────────────────────────────────────────────

pub(super) fn dromeus_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dromeus"),
        description: "Execute a dispatch spec: run prompt groups in parallel or sequential order, \
            spawning agent sessions per prompt. Returns aggregate outcomes and total cost."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "spec".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Dispatch spec identifier or inline spec JSON".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "budget_usd".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum total spend in USD (default: no limit)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "max_turns".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Maximum turns per session (default: no limit)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Validate the spec without spawning sessions (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["spec".to_owned(), "project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

pub(super) struct DromeusExecutor {
    pub(super) orchestrator: Option<Arc<Orchestrator>>,
}

impl ToolExecutor for DromeusExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref orchestrator) = self.orchestrator else {
                return Ok(ToolResult::error(
                    "dromeus: orchestrator not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let spec_str = match require_str(args, "spec") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let dry_run = opt_bool(args, "dry_run").unwrap_or(false);

            // WHY: spec is a JSON array of PromptSpec objects. Callers build the
            // spec programmatically (e.g. from prographe output) and pass it inline.
            let prompts: Vec<aletheia_energeia::prompt::PromptSpec> =
                match serde_json::from_str(spec_str) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "dromeus: invalid spec JSON: {e}"
                        )));
                    }
                };

            if dry_run {
                return match orchestrator.dry_run(&prompts) {
                    Ok(plan) => Ok(to_json_text(&plan)),
                    Err(e) => Ok(ToolResult::error(format!("dromeus: dry_run failed: {e}"))),
                };
            }

            let prompt_numbers: Vec<u32> = prompts.iter().map(|p| p.number).collect();
            let mut dispatch_spec =
                aletheia_energeia::types::DispatchSpec::new(project.to_owned(), prompt_numbers);
            dispatch_spec.max_parallel =
                opt_u64(args, "max_turns").and_then(|v| u32::try_from(v).ok());

            match orchestrator.dispatch(dispatch_spec, &prompts).await {
                Ok(result) => Ok(to_json_text(&result)),
                Err(e) => Ok(ToolResult::error(format!("dromeus: dispatch failed: {e}"))),
            }
        })
    }
}
