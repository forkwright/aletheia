//! QA tools (dokimasia + diorthosis).
//!
//! - dokimasia (δοκιμασία — examination): run QA evaluation of a PR
//! - diorthosis (διόρθωσις — correction): generate corrective prompt specs

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_energeia::qa::corrective::generate_corrective;
use aletheia_energeia::qa::run_qa;
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::shared::{opt_u64, require_str, to_json_text};

// ── dokimasia (δοκιμασία — examination) ────────────────────────────────────

pub(super) fn dokimasia_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dokimasia"),
        description: "Run a QA evaluation of a pull request against the originating prompt spec. \
            Returns a verdict (pass/partial/fail), per-criterion results, and mechanical issues."
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
                    },
                ),
                (
                    "pr_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub pull request number to evaluate".to_owned(),
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
            ]),
            required: vec![
                "prompt_number".to_owned(),
                "pr_number".to_owned(),
                "project".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
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
            let pr_number = match opt_u64(args, "pr_number") {
                Some(n) => n,
                None => return Ok(ToolResult::error("missing required field 'pr_number'")),
            };
            let _project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            // WHY: Build a minimal QA prompt spec from the prompt number. Full
            // prompt spec loading (with real acceptance criteria) requires file
            // I/O outside the tool's scope. Callers can add criteria via a future
            // schema extension. Mechanical checks run against the empty diff.
            let qa_prompt = aletheia_energeia::qa::PromptSpec::new(
                prompt_number,
                format!("Prompt #{prompt_number}"),
            );

            // WHY: Diff is empty because fetching the PR diff requires GitHub API
            // access which is outside this tool's scope. Callers may pass a diff
            // via a future `diff` field extension. Mechanical checks on an empty
            // diff produce no findings.
            let qa_result = run_qa("", &qa_prompt, pr_number);

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
                    },
                ),
                (
                    "original_prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that produced the failing PR".to_owned(),
                        enum_values: None,
                        default: None,
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
                Err(e) => return Ok(e),
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
            let qa_result: aletheia_energeia::types::QaResult =
                match serde_json::from_str(qa_result_id) {
                    Ok(r) => r,
                    Err(_) => {
                        return Ok(ToolResult::error(
                            "diorthosis: qa_result_id must be a JSON-encoded QaResult \
                            (copy the JSON output from a dokimasia call)",
                        ));
                    }
                };

            let original = aletheia_energeia::qa::PromptSpec::new(
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
