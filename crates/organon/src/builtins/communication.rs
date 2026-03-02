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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{ToolContext, ToolInput};

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
        }
    }

    #[tokio::test]
    async fn register_communication_tools() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        assert_eq!(reg.definitions().len(), 2);
    }

    #[tokio::test]
    async fn message_stub_responds() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("message").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"to": "+1234567890", "text": "hello"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.contains("stub"), "expected stub: {}", result.content);
    }

    #[tokio::test]
    async fn sessions_ask_stub_responds() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_ask").expect("valid"),
            tool_use_id: "tu_2".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "hello"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn message_def_requires_to_and_text() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("message").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(def.input_schema.required, vec!["to", "text"]);
    }

    #[tokio::test]
    async fn sessions_ask_def_requires_agent_and_message() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("sessions_ask").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(def.input_schema.required, vec!["agentId", "message"]);
    }
}
