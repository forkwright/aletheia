//! Meta-tool for dynamically activating lazy tools per session.

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

use super::workspace::extract_str;

struct EnableToolExecutor;

impl ToolExecutor for EnableToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let name = extract_str(&input.arguments, "name", &input.name)?;

            let Some(services) = ctx.services.as_deref() else {
                return Ok(ToolResult::error("tool services not configured"));
            };

            let Ok(tool_name) = ToolName::new(name) else {
                return Ok(ToolResult::error(format!("invalid tool name: {name}")));
            };

            // Check if it's in the lazy catalog
            let catalog_entry = services
                .lazy_tool_catalog
                .iter()
                .find(|(n, _)| *n == tool_name);

            let Some((_, description)) = catalog_entry else {
                let available: Vec<&str> = services
                    .lazy_tool_catalog
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                return Ok(ToolResult::error(format!(
                    "tool '{name}' not found. Available tools: {}",
                    available.join(", ")
                )));
            };

            // Check if already active
            {
                let active = ctx.active_tools.read().expect("active_tools lock");
                if active.contains(&tool_name) {
                    return Ok(ToolResult::text(format!("'{name}' is already active.")));
                }
            }

            // Activate
            {
                let mut active = ctx.active_tools.write().expect("active_tools lock");
                active.insert(tool_name);
            }

            Ok(ToolResult::text(format!(
                "Activated '{name}': {description}"
            )))
        })
    }
}

fn enable_tool_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("enable_tool").expect("valid tool name"),
        description: "Activate a tool for this session. Some tools are not loaded by default \
                      and must be enabled first. Call with the tool name to activate it."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "name".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Name of the tool to activate".to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec!["name".to_owned()],
        },
        category: ToolCategory::System,
        auto_activate: true,
    }
}

pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(enable_tool_def(), Box::new(EnableToolExecutor))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::types::{ToolContext, ToolInput, ToolServices};

    use super::*;

    fn test_ctx_with_catalog(catalog: Vec<(ToolName, String)>) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: catalog,
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn make_input(tool_name: &str) -> ToolInput {
        ToolInput {
            name: ToolName::new("enable_tool").expect("valid"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"name": tool_name}),
        }
    }

    #[tokio::test]
    async fn activate_known_tool() {
        let ctx = test_ctx_with_catalog(vec![(
            ToolName::new("web_search").expect("valid"),
            "Search the web".to_owned(),
        )]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error);
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'web_search'")
        );

        let active = ctx.active_tools.read().expect("lock");
        assert!(active.contains(&ToolName::new("web_search").expect("valid")));
    }

    #[tokio::test]
    async fn unknown_tool_lists_available() {
        let ctx = test_ctx_with_catalog(vec![
            (
                ToolName::new("web_search").expect("valid"),
                "Search the web".to_owned(),
            ),
            (
                ToolName::new("web_fetch").expect("valid"),
                "Fetch a URL".to_owned(),
            ),
        ]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("nonexistent"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error);
        let text = result.content.text_summary();
        assert!(text.contains("web_search"));
        assert!(text.contains("web_fetch"));
    }

    #[tokio::test]
    async fn double_activate_is_idempotent() {
        let ctx = test_ctx_with_catalog(vec![(
            ToolName::new("web_search").expect("valid"),
            "Search the web".to_owned(),
        )]);

        let executor = EnableToolExecutor;
        executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("first");

        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("second");

        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("already active"));
    }
}
