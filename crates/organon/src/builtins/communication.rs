//! Communication tool executors: message, sessions_ask, sessions_send.

use std::future::Future;
use std::pin::Pin;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use super::workspace::{extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

const MESSAGE_MAX_LEN: usize = 4000;

struct MessageExecutor;

impl ToolExecutor for MessageExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(services) = ctx.services.as_ref() else {
                return Ok(ToolResult::error("message service not available"));
            };
            let Some(messenger) = services.messenger.as_ref() else {
                return Ok(ToolResult::error("message service not configured"));
            };

            let to = extract_str(&input.arguments, "to", &input.name)?;
            let text = extract_str(&input.arguments, "text", &input.name)?;

            if text.len() > MESSAGE_MAX_LEN {
                return Ok(ToolResult::error(format!(
                    "Message exceeds {MESSAGE_MAX_LEN} character limit ({} chars)",
                    text.len()
                )));
            }

            match messenger.send_message(to, text, ctx.nous_id.as_str()).await {
                Ok(()) => Ok(ToolResult::text(format!("Message sent to {to}"))),
                Err(e) => Ok(ToolResult::error(format!("Send failed: {e}"))),
            }
        })
    }
}

struct SessionsAskExecutor;

impl ToolExecutor for SessionsAskExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(services) = ctx.services.as_ref() else {
                return Ok(ToolResult::error("cross-nous service not available"));
            };
            let Some(cross) = services.cross_nous.as_ref() else {
                return Ok(ToolResult::error("cross-nous service not configured"));
            };

            let agent_id = extract_str(&input.arguments, "agentId", &input.name)?;
            let message = extract_str(&input.arguments, "message", &input.name)?;
            let default_session = format!("ask:{}", ctx.nous_id.as_str());
            let session_key = input
                .arguments
                .get("sessionKey")
                .and_then(|v| v.as_str())
                .unwrap_or(&default_session);
            let timeout = extract_opt_u64(&input.arguments, "timeoutSeconds").unwrap_or(120);

            match cross
                .ask(
                    ctx.nous_id.as_str(),
                    agent_id,
                    session_key,
                    message,
                    timeout,
                )
                .await
            {
                Ok(reply) => Ok(ToolResult::text(reply)),
                Err(e) => Ok(ToolResult::error(format!("Ask {agent_id} failed: {e}"))),
            }
        })
    }
}

struct SessionsSendExecutor;

impl ToolExecutor for SessionsSendExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(services) = ctx.services.as_ref() else {
                return Ok(ToolResult::error("cross-nous service not available"));
            };
            let Some(cross) = services.cross_nous.as_ref() else {
                return Ok(ToolResult::error("cross-nous service not configured"));
            };

            let agent_id = extract_str(&input.arguments, "agentId", &input.name)?;
            let message = extract_str(&input.arguments, "message", &input.name)?;
            let session_key = input
                .arguments
                .get("sessionKey")
                .and_then(|v| v.as_str())
                .unwrap_or("main");

            match cross
                .send(ctx.nous_id.as_str(), agent_id, session_key, message)
                .await
            {
                Ok(()) => Ok(ToolResult::text(format!(
                    "Message sent to {agent_id} (session: {session_key})"
                ))),
                Err(e) => Ok(ToolResult::error(format!(
                    "Failed to send to {agent_id}: {e}"
                ))),
            }
        })
    }
}

/// Register communication tools.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(message_def(), Box::new(MessageExecutor))?;
    registry.register(sessions_ask_def(), Box::new(SessionsAskExecutor))?;
    registry.register(sessions_send_def(), Box::new(SessionsSendExecutor))?;
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

fn sessions_send_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("sessions_send").expect("valid tool name"),
        description: "Send a message to another agent without waiting for a response".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "agentId".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target nous ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "message".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Message to send".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "sessionKey".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target session key (default: 'main')".to_owned(),
                        enum_values: None,
                        default: None,
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
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{CrossNousService, MessageService, ToolContext, ToolInput, ToolServices};

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
        }
    }

    fn test_ctx_with_services(services: ToolServices) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: Some(Arc::new(services)),
        }
    }

    #[derive(Default)]
    struct MockCrossNous {
        send_calls: Mutex<Vec<(String, String, String, String)>>,
        ask_reply: Mutex<Option<Result<String, String>>>,
    }

    impl CrossNousService for MockCrossNous {
        fn send(
            &self,
            from: &str,
            to: &str,
            session_key: &str,
            content: &str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            self.send_calls.lock().unwrap().push((
                from.to_owned(),
                to.to_owned(),
                session_key.to_owned(),
                content.to_owned(),
            ));
            Box::pin(async { Ok(()) })
        }

        fn ask(
            &self,
            _from: &str,
            _to: &str,
            _session_key: &str,
            _content: &str,
            _timeout_secs: u64,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            let reply = self
                .ask_reply
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok("mock reply".to_owned()));
            Box::pin(async move { reply })
        }
    }

    #[derive(Default)]
    struct MockMessenger {
        send_calls: Mutex<Vec<(String, String, String)>>,
    }

    impl MessageService for MockMessenger {
        fn send_message(
            &self,
            to: &str,
            text: &str,
            from_nous: &str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            self.send_calls.lock().unwrap().push((
                to.to_owned(),
                text.to_owned(),
                from_nous.to_owned(),
            ));
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn register_communication_tools() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        assert_eq!(reg.definitions().len(), 3);
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

    #[tokio::test]
    async fn sessions_send_def_requires_agent_and_message() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("sessions_send").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(def.input_schema.required, vec!["agentId", "message"]);
    }

    #[tokio::test]
    async fn message_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("message").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"to": "+1234567890", "text": "hello"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not available"));
    }

    #[tokio::test]
    async fn sessions_send_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_send").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "hello"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not available"));
    }

    #[tokio::test]
    async fn sessions_ask_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_ask").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "hello"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not available"));
    }

    #[tokio::test]
    async fn message_rejects_over_4000_chars() {
        let messenger = Arc::new(MockMessenger::default());
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: None,
            note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
            messenger: Some(messenger),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("message").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"to": "+1234567890", "text": "x".repeat(4001)}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("4000"));
    }

    #[tokio::test]
    async fn message_sends_via_signal() {
        let messenger = Arc::new(MockMessenger::default());
        let messenger_ref = Arc::clone(&messenger);
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: None,
            note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
            messenger: Some(messenger),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("message").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"to": "+1234567890", "text": "hello world"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("sent"));

        let calls = messenger_ref.send_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "+1234567890");
        assert_eq!(calls[0].1, "hello world");
        assert_eq!(calls[0].2, "test-agent");
    }

    #[tokio::test]
    async fn sessions_send_dispatches() {
        let cross = Arc::new(MockCrossNous::default());
        let cross_ref = Arc::clone(&cross);
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: Some(cross),
            messenger: None,
                    note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_send").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "do the thing"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("sent"));

        let calls = cross_ref.send_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test-agent");
        assert_eq!(calls[0].1, "syn");
        assert_eq!(calls[0].2, "main");
        assert_eq!(calls[0].3, "do the thing");
    }

    #[tokio::test]
    async fn sessions_send_uses_custom_session_key() {
        let cross = Arc::new(MockCrossNous::default());
        let cross_ref = Arc::clone(&cross);
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: Some(cross),
            messenger: None,
                    note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_send").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "hi", "sessionKey": "custom"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);

        let calls = cross_ref.send_calls.lock().unwrap();
        assert_eq!(calls[0].2, "custom");
    }

    #[tokio::test]
    async fn sessions_ask_returns_reply() {
        let cross = Arc::new(MockCrossNous::default());
        *cross.ask_reply.lock().unwrap() = Some(Ok("the answer is 42".to_owned()));
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: Some(cross),
            messenger: None,
                    note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_ask").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "what is the answer?"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert_eq!(result.content.text_summary(), "the answer is 42");
    }

    #[tokio::test]
    async fn sessions_ask_timeout_returns_error() {
        let cross = Arc::new(MockCrossNous::default());
        *cross.ask_reply.lock().unwrap() = Some(Err("timed out after 120s".to_owned()));
        let ctx = test_ctx_with_services(ToolServices {
            cross_nous: Some(cross),
            messenger: None,
                    note_store: None,
            blackboard_store: None,
            http_client: reqwest::Client::new(),
        });
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("sessions_ask").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"agentId": "syn", "message": "hello?"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("timed out"));
    }
}
