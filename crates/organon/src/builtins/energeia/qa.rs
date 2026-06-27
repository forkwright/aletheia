//! QA tools (dokimasia + diorthosis).
//!
//! - dokimasia (δοκιμασία — examination): run QA evaluation of a PR
//! - diorthosis (διόρθωσις — correction): generate corrective prompt specs

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use energeia::qa::corrective::generate_corrective;
use energeia::qa::run_qa;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::shared::{opt_str, opt_u64, require_str, to_json_text};

// ── dokimasia (δοκιμασία — examination) ────────────────────────────────────

pub(super) fn dokimasia_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dokimasia"),
        description: "Run mechanical QA checks against a caller-provided pull-request diff. \
            Semantic acceptance-criteria evaluation requires orchestrator-side prompt and LLM \
            wiring; empty diffs return no-work rather than a pass verdict."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that generated this PR".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "pr_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub pull request number to evaluate".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional GitHub project slug (owner/repo), reserved for \
                            future QA result persistence"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "diff".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Unified PR diff to evaluate; empty diffs return no-work."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![
                "prompt_number".to_owned(),
                "pr_number".to_owned(),
                "diff".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify],
    }
}

pub(super) struct DokimasiaExecutor;

impl ToolExecutor for DokimasiaExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let prompt_number = match opt_u64(args, "prompt_number") {
                Some(n) => u32::try_from(n).unwrap_or(0),
                None => return Ok(ToolResult::error("missing required field 'prompt_number'")),
            };
            let Some(pr_number) = opt_u64(args, "pr_number") else {
                return Ok(ToolResult::error("missing required field 'pr_number'"));
            };
            let project = opt_str(args, "project");
            if project.is_some_and(|p| p.trim().is_empty()) {
                return Ok(ToolResult::error("field 'project' must not be empty"));
            }
            let diff = match require_str(args, "diff") {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::error(e)),
            };
            if diff.trim().is_empty() {
                let output = serde_json::json!({
                    "status": "no_work",
                    "reason": "no diff to QA",
                    "project": project,
                    "prompt_number": prompt_number,
                    "pr_number": pr_number,
                });
                return Ok(to_json_text(&output));
            }

            // WHY: Build a minimal QA prompt spec from the prompt number. Full
            // prompt spec loading (with real acceptance criteria) requires file
            // I/O outside the tool's scope. Mechanical checks run against the
            // caller-provided diff.
            let qa_prompt =
                energeia::qa::PromptSpec::new(prompt_number, format!("Prompt #{prompt_number}"));

            // WHY: No LLM provider available in the tool context — runs
            // mechanical-only evaluation. Semantic evaluation requires the
            // orchestrator which has access to hermeneus providers.
            let qa_result = run_qa(diff, &qa_prompt, pr_number, None).await;

            Ok(to_json_text(&qa_result))
        })
    }
}

// ── diorthosis (διόρθωσις — correction) ────────────────────────────────────

pub(super) fn diorthosis_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("diorthosis"),
        description: "Generate a corrective prompt spec from a failed QA result. \
            Stateless transformation: takes the QA result and original prompt, \
            returns a revised prompt spec targeting the identified deficiencies."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "qa_result_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "ID of the QA result from a previous dokimasia run, \
                            or inline JSON-encoded QaResult"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "original_prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that produced the failing PR".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![
                "qa_result_id".to_owned(),
                "original_prompt_number".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Edit],
    }
}

pub(super) struct DiorthosisExecutor;

impl ToolExecutor for DiorthosisExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let qa_result_id = match require_str(args, "qa_result_id") {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::error(e)),
            };
            let original_prompt_number = match opt_u64(args, "original_prompt_number") {
                Some(n) => u32::try_from(n).unwrap_or(0),
                None => {
                    return Ok(ToolResult::error(
                        "missing required field 'original_prompt_number'",
                    ));
                }
            };

            // WHY: qa_result_id accepts inline JSON-encoded QaResult (the output from
            // dokimasia) so callers can chain dokimasia -> diorthosis without a
            // persistent QA result store. A future store extension will support opaque
            // IDs for server-side lookup.
            let qa_result: energeia::types::QaResult = match serde_json::from_str(qa_result_id) {
                Ok(r) => r,
                Err(_) => {
                    return Ok(ToolResult::error(
                        "diorthosis: qa_result_id must be a JSON-encoded QaResult \
                            (copy the JSON output from a dokimasia call)",
                    ));
                }
            };

            let original = energeia::qa::PromptSpec::new(
                original_prompt_number,
                format!("Prompt #{original_prompt_number}"),
            );

            match generate_corrective(&qa_result, &original) {
                Some(corrective) => {
                    let output = serde_json::json!({
                        "description": corrective.description,
                        "prompt_number": corrective.prompt_number,
                        "acceptance_criteria": corrective.acceptance_criteria,
                        "blast_radius": corrective.blast_radius,
                    });
                    Ok(to_json_text(&output))
                }
                None => Ok(ToolResult::text(
                    "diorthosis: no corrective needed (verdict is Pass or no failed criteria)",
                )),
            }
        })
    }
}
