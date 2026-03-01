//! Memory tool stubs: `mem0_search`, note, blackboard.

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

struct Stub;

impl ToolExecutor for Stub {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            Ok(ToolResult {
                content: format!("stub: {} not implemented", input.name),
                is_error: false,
            })
        })
    }
}

/// Register memory tool stubs.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(mem0_search_def(), Box::new(Stub))?;
    registry.register(note_def(), Box::new(Stub))?;
    registry.register(blackboard_def(), Box::new(Stub))?;
    Ok(())
}

fn mem0_search_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("mem0_search").expect("valid tool name"),
        description: "Search long-term memory for facts, preferences, and relationships"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "query".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Semantic search query".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "limit".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Max results (default 10)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(10)),
                    },
                ),
            ]),
            required: vec!["query".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

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
        auto_activate: false,
    }
}

fn blackboard_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("blackboard").expect("valid tool name"),
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
        auto_activate: false,
    }
}
