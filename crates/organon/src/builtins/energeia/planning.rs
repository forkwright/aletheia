//! Planning tools (prographe + schedion).
//!
//! - prographe (προγραφή — template): render prompt specs from issues/descriptions
//! - schedion (σχέδιον — plan/graph): DAG visualization + frontier computation

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use energeia::dag::{PromptDag, compute_frontier};
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::shared::{opt_str, opt_u64, require_str, to_json_text};

// ── prographe (προγραφή — template) ────────────────────────────────────────

pub(super) fn prographe_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("prographe"),
        description: "Render a prompt spec template from a GitHub issue number or \
            description. This tool does not allocate queue numbers or write files; \
            it returns YAML with number 0 for the caller to save."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "from_issue".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub issue number used only to seed template text"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug echoed in the result; no project files \
                            are read or written"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Free-form task description (alternative to from_issue)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "criteria".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Array,
                        description: "Explicit acceptance criteria strings to embed in the spec"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Format],
    }
}

pub(super) struct ProographeExecutor;

impl ToolExecutor for ProographeExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let description = opt_str(args, "description").map_or_else(
                || {
                    let issue_num = opt_u64(args, "from_issue").unwrap_or(0);
                    if issue_num > 0 {
                        format!("Implement GitHub issue #{issue_num}")
                    } else {
                        "Task description".to_owned()
                    }
                },
                ToOwned::to_owned,
            );

            let criteria: Vec<String> = args
                .get("criteria")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();

            let project = opt_str(args, "project").unwrap_or("(unspecified)");

            // Build YAML frontmatter for the prompt spec.
            // WHY: prompt_number 0 signals "to be assigned" — the operator
            // replaces it with the next queue number before dispatching.
            let criteria_yaml = if criteria.is_empty() {
                "  - \"(to be defined)\"\n".to_owned()
            } else {
                let mut yaml = String::new();
                for criterion in &criteria {
                    yaml.push_str("  - \"");
                    yaml.push_str(criterion);
                    yaml.push_str("\"\n");
                }
                yaml
            };

            let spec_yaml = format!(
                "---\nnumber: 0\ndescription: \"{description}\"\ndepends_on: []\n\
                acceptance_criteria:\n{criteria_yaml}blast_radius:\n  - \"\"\n---\n\n\
                # Task\n\n{description}\n"
            );

            let output = serde_json::json!({
                "project": project,
                "spec": spec_yaml,
                "criteria_count": criteria.len(),
                "prompt_number_assigned": false,
                "files_written": [],
                "note": "Template only: number 0 is a placeholder and no files were written.",
            });

            Ok(to_json_text(&output))
        })
    }
}

// ── schedion (σχέδιον — plan/graph) ────────────────────────────────────────

pub(super) fn schedion_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("schedion"),
        description: "Compute the execution frontier for an empty prompt dependency DAG. \
            The tool does not load prompt specs from a project path yet, so callers should \
            use the CLI dispatch pipeline for file-backed DAGs."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "project".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "GitHub project slug echoed in the result; no prompt files are \
                        loaded from it"
                        .to_owned(),
                    enum_values: None,
                    default: None,
                    ..Default::default()
                },
            )]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan],
    }
}

pub(super) struct SchedionExecutor;

impl ToolExecutor for SchedionExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;
            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            // WHY: Prompt files aren't accessible from the project slug alone —
            // that requires a configured prompts-directory mapping. The tool
            // computes the frontier on an empty DAG and notes the limitation.
            // Full file-backed DAG construction is available via the CLI dispatch
            // pipeline which knows where the prompts directory is.
            let dag = PromptDag::new();
            let frontier = compute_frontier(&dag);

            let output = serde_json::json!({
                "project": project,
                "node_count": 0,
                "frontier_group_count": frontier.len(),
                "frontier": frontier,
                "loaded_prompt_specs": false,
                "note": "No prompt spec files found via tool call. \
                    Use the CLI dispatch pipeline to load prompts from the filesystem.",
            });

            Ok(to_json_text(&output))
        })
    }
}
