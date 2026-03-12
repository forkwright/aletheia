//! Session note tool executor.

use std::future::Future;
use std::pin::Pin;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use crate::builtins::workspace::extract_str;

use super::require_services;

struct NoteExecutor;

impl ToolExecutor for NoteExecutor {
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
            let Some(note_store) = services.note_store.as_ref() else {
                return Ok(ToolResult::error("note store not configured"));
            };

            let action = extract_str(&input.arguments, "action", &input.name)?;

            match action {
                "add" => {
                    let content = extract_str(&input.arguments, "content", &input.name)?;
                    let category = input
                        .arguments
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("context");

                    if content.len() > 500 {
                        return Ok(ToolResult::error(
                            "Note content exceeds 500 character limit",
                        ));
                    }

                    match note_store.add_note(
                        &ctx.session_id.to_string(),
                        ctx.nous_id.as_str(),
                        category,
                        content,
                    ) {
                        Ok(id) => Ok(ToolResult::text(format!(
                            "Note #{id} saved ({category}): \"{content}\""
                        ))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to save note: {e}"))),
                    }
                }
                "list" => match note_store.get_notes(&ctx.session_id.to_string()) {
                    Ok(notes) if notes.is_empty() => Ok(ToolResult::text("No session notes.")),
                    Ok(notes) => {
                        let lines: Vec<String> = notes
                            .iter()
                            .map(|n| format!("#{} [{}] {}", n.id, n.category, n.content))
                            .collect();
                        Ok(ToolResult::text(lines.join("\n")))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to list notes: {e}"))),
                },
                "delete" => {
                    let id = input
                        .arguments
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or_else(|| {
                            crate::error::InvalidInputSnafu {
                                name: input.name.clone(),
                                reason: "missing or invalid field: id".to_owned(),
                            }
                            .build()
                        })?;
                    match note_store.delete_note(id) {
                        Ok(_) => Ok(ToolResult::text(format!("Note #{id} deleted."))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to delete note: {e}"))),
                    }
                }
                _ => Ok(ToolResult::error(format!("Unknown action: {action}"))),
            }
        })
    }
}

// --- Tool Definition ---

fn note_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("note").expect("valid tool name"),
        description: "Write a note to persistent session memory that survives distillation"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action: 'add', 'list', 'delete'".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Note content (required for 'add', max 500 chars)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "category".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Note category: task, decision, preference, correction, context"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!("context")),
                    },
                ),
                (
                    "id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Note ID (required for 'delete')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: true,
    }
}

// --- Registration ---

pub(super) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(note_def(), Box::new(NoteExecutor))?;
    Ok(())
}
