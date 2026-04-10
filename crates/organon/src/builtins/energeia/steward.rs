//! Steward tool (epitropos — ἐπίτροπος, steward).
//!
//! CI steward: monitors pull requests, auto-merges passing PRs, and queues
//! failing PRs for repair.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_energeia::steward::service::{StewardConfig, run_once};
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::shared::{opt_bool, require_str, to_json_text};

// ── epitropos (ἐπίτροπος — steward) ───────────────────────────────────────

pub(super) fn epitropos_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("epitropos"),
        description: "CI steward: monitor pull requests, auto-merge passing PRs, \
            queue failing PRs for repair. Runs as a polling loop unless `once` is set."
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
                    },
                ),
                (
                    "once".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Run a single classification pass instead of a polling loop \
                            (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Classify PRs without merging or queuing repairs \
                            (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
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
                Err(e) => return Ok(e),
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
                "classified_count": result.classified.len(),
                "merged_count": result.merged.len(),
                "needs_fix_count": result.needs_fix.len(),
                "blocked_count": result.blocked.len(),
                "main_ci_status": format!("{:?}", result.main_ci_status),
                "main_fix_attempted": result.main_fix_attempted,
            });

            Ok(to_json_text(&output))
        })
    }
}
