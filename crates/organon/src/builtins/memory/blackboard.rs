//! Shared blackboard tool executor.
#![expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain"
)]

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use crate::builtins::workspace::{extract_opt_u64, extract_str};

use super::require_services;

struct BlackboardExecutor;

impl ToolExecutor for BlackboardExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let Some(bb_store) = services.blackboard_store.as_ref() else {
                return Ok(ToolResult::error("blackboard store not configured"));
            };

            let action = extract_str(&input.arguments, "action", &input.name)?;

            match action {
                "write" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    let value = extract_str(&input.arguments, "value", &input.name)?;
                    let ttl = extract_opt_u64(&input.arguments, "ttl_seconds").unwrap_or(3600);

                    let ttl_i64 = i64::try_from(ttl).unwrap_or(i64::MAX);
                    match bb_store.write(key, value, ctx.nous_id.as_str(), ttl_i64) {
                        Ok(()) => Ok(ToolResult::text(format!(
                            "Blackboard [{key}] written (TTL: {ttl}s)"
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to write blackboard: {e}"
                        ))),
                    }
                }
                "read" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    match bb_store.read(key) {
                        Ok(Some(entry)) => Ok(ToolResult::text(format!(
                            "[{key}] = {} (by {}, expires: {})",
                            entry.value,
                            entry.author_nous_id,
                            entry.expires_at.as_deref().unwrap_or("never")
                        ))),
                        Ok(None) => Ok(ToolResult::text(format!("No entry for key: {key}"))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to read blackboard: {e}"))),
                    }
                }
                "list" => match bb_store.list() {
                    Ok(entries) if entries.is_empty() => {
                        Ok(ToolResult::text("Blackboard is empty."))
                    }
                    Ok(entries) => {
                        let lines: Vec<String> = entries
                            .iter()
                            .map(|e| format!("[{}] = {} (by {})", e.key, e.value, e.author_nous_id))
                            .collect();
                        Ok(ToolResult::text(lines.join("\n")))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to list blackboard: {e}"))),
                },
                "delete" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    match bb_store.delete(key, ctx.nous_id.as_str()) {
                        Ok(true) => Ok(ToolResult::text(format!("Blackboard [{key}] deleted."))), // kanon:ignore STORAGE/sql-string-concat
                        Ok(false) => Ok(ToolResult::text(format!(
                            "No entry for key: {key} (or not your entry)"
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to delete blackboard entry: {e}"
                        ))),
                    }
                }
                _ => Ok(ToolResult::error(format!("Unknown action: {action}"))),
            }
        })
    }
}

fn blackboard_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("blackboard").expect("valid tool name"), // kanon:ignore RUST/expect
        description: "Read and write shared state visible to all agents".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action: 'write', 'read', 'list', 'delete'".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "key".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Blackboard key (required for write/read/delete)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "value".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Value to write (required for write action)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "ttl_seconds".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Time-to-live in seconds (default 3600)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(3600)),
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: true,
    }
}

pub(super) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(blackboard_def(), Box::new(BlackboardExecutor))?;
    Ok(())
}
