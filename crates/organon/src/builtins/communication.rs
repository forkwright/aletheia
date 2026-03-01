//! Communication tool stubs: message, `sessions_ask`.

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

/// Register communication tool stubs.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(message_def(), Box::new(Stub))?;
    registry.register(sessions_ask_def(), Box::new(Stub))?;
    Ok(())
}

fn message_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("message").expect("valid tool name"),
        description: "Send a message to a user or group via Signal".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "to".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Recipient: phone (+1234567890), group (group:ID), or username (u:handle)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Message text to send (markdown supported)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["to".to_owned(), "text".to_owned()],
        },
        category: ToolCategory::Communication,
        auto_activate: false,
    }
}

fn sessions_ask_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("sessions_ask").expect("valid tool name"),
        description: "Ask another agent a question and wait for their response".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "agentId".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target nous ID (e.g., 'syn', 'eiron', 'arbor')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "message".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Question or request to send".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "sessionKey".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target session key (default: 'ask:<caller>')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeoutSeconds".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Max wait time in seconds (default: 120)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(120)),
                    },
                ),
            ]),
            required: vec!["agentId".to_owned(), "message".to_owned()],
        },
        category: ToolCategory::Communication,
        auto_activate: false,
    }
}
