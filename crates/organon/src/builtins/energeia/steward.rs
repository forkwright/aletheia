//! Steward tool (epitropos — ἐπίτροπος, steward).
//!
//! CI steward tool surface.
//!
//! The current tool executor runs one placeholder steward classification pass.
//! It does not start the polling loop, merge PRs, or queue repair work.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use energeia::steward::service::{StewardConfig, run_once};
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::shared::{opt_bool, require_str, to_json_text};

// ── epitropos (ἐπίτροπος — steward) ───────────────────────────────────────

pub(super) fn epitropos_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("epitropos"),
        description: "Run one placeholder CI steward classification pass. The tool \
            does not poll, merge PRs, or queue repair work until the steward backend \
            is wired."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "once".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Accepted for compatibility; this tool always runs one \
                            classification pass and never starts a polling loop"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Accepted for compatibility; the placeholder pass has no \
                            merge or repair side effects"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan, ToolGroupId::Verify],
        tags: vec![ToolTag::Execute],
    }
}

pub(super) struct EpitroposExecutor;

impl ToolExecutor for EpitroposExecutor {
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
            let once = opt_bool(args, "once").unwrap_or(false);
            let dry_run = opt_bool(args, "dry_run").unwrap_or(false);

            let mut config = StewardConfig::new(project.to_owned());
            config.once = once;
            config.dry_run = dry_run;

            // WHY: Always use run_once in tool context — a polling loop would block
            // the tool executor indefinitely. Callers that need the polling loop
            // should schedule a recurring trigger instead.
            let result = run_once(&config).await;

            let output = serde_json::json!({
                "project": project,
                "dry_run": dry_run,
                "mode": "single_placeholder_pass",
                "polling_loop_started": false,
                "merge_side_effects_enabled": false,
                "repair_queue_side_effects_enabled": false,
                "classified_count": result.classified.len(),
                "merged_count": result.merged.len(),
                "needs_fix_count": result.needs_fix.len(),
                "blocked_count": result.blocked.len(),
                "main_ci_status": format!("{:?}", result.main_ci_status),
                "main_fix_attempted": result.main_fix_attempted,
                "note": "run_once currently returns placeholder classification data.",
            });

            Ok(to_json_text(&output))
        })
    }
}
