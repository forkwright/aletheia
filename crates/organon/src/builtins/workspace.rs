//! Workspace tool stubs: read, write, edit, exec.

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

/// Register workspace tool stubs.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(read_def(), Box::new(Stub))?;
    registry.register(write_def(), Box::new(Stub))?;
    registry.register(edit_def(), Box::new(Stub))?;
    registry.register(exec_def(), Box::new(Stub))?;
    Ok(())
}

fn read_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("read").expect("valid tool name"),
        description: "Read a file's contents as text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum lines to return (default: all)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: false,
    }
}

fn write_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("write").expect("valid tool name"),
        description: "Write content to a file, creating parent directories as needed".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Content to write".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "append".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Append instead of overwrite (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["path".to_owned(), "content".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: false,
    }
}

fn edit_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("edit").expect("valid tool name"),
        description: "Replace exact text in a file with new text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "old_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Exact text to find in the file".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "new_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Replacement text".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "path".to_owned(),
                "old_text".to_owned(),
                "new_text".to_owned(),
            ],
        },
        category: ToolCategory::Workspace,
        auto_activate: false,
    }
}

fn exec_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("exec").expect("valid tool name"),
        description: "Execute a shell command in your workspace and return stdout/stderr".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "command".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "The shell command to execute".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeout".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Timeout in milliseconds (default 30000)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30000)),
                    },
                ),
            ]),
            required: vec!["command".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: false,
    }
}
