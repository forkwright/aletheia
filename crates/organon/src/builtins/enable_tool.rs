//! Meta-tool for dynamically activating lazy tools per session.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
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

            let server_catalog = services.server_tool_config.catalog_entries();
            let catalog_entry = services
                .lazy_tool_catalog
                .iter()
                .find(|(n, _)| *n == tool_name)
                .or_else(|| server_catalog.iter().find(|(n, _)| *n == tool_name));

            let Some((_, description)) = catalog_entry else {
                let mut available: Vec<&str> = services
                    .lazy_tool_catalog
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                available.extend(server_catalog.iter().map(|(n, _)| n.as_str()));
                return Ok(ToolResult::error(format!(
                    "tool '{name}' not found. Available tools: {}",
                    available.join(", ")
                )));
            };

            // WHY: Single write lock for the check-and-set: acquiring a read
            // lock to check then dropping it before acquiring a write lock
            // creates a TOCTOU window where a concurrent caller can insert
            // the same tool between the two acquisitions.
            {
                let Ok(mut active) = ctx.active_tools.write() else {
                    return Ok(ToolResult::error(
                        "internal error: active_tools lock poisoned",
                    ));
                };
                if active.contains(&tool_name) {
                    return Ok(ToolResult::text(format!("'{name}' is already active.")));
                }
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
        name: ToolName::from_static("enable_tool"), // kanon:ignore RUST/expect
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
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
    }
}

/// Register the `enable_tool` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(enable_tool_def(), Box::new(EnableToolExecutor))?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::types::{ServerToolConfig, ToolContext, ToolInput, ToolServices};

    use super::*;

    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn mock_ctx_with_catalog(catalog: Vec<(ToolName, String)>) -> ToolContext {
        install_crypto_provider();
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
                knowledge: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: catalog,
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn make_input(tool_name: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("enable_tool"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"name": tool_name}),
        }
    }

    #[tokio::test]
    async fn activate_known_tool() {
        let ctx = mock_ctx_with_catalog(vec![(
            ToolName::from_static("web_search"),
            "Search the web".to_owned(),
        )]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'web_search'")
        );

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(
            active.contains(&ToolName::from_static("web_search")),
            "expected active.contains(&ToolName::from_static(\"web_search\")) to be true"
        );
    }

    #[tokio::test]
    async fn unknown_tool_lists_available() {
        let ctx = mock_ctx_with_catalog(vec![
            (
                ToolName::from_static("web_search"),
                "Search the web".to_owned(),
            ),
            (ToolName::from_static("web_fetch"), "Fetch a URL".to_owned()),
        ]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("nonexistent"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        let text = result.content.text_summary();
        assert!(
            text.contains("web_search"),
            "expected text.contains(\"web_search\") to be true"
        );
        assert!(
            text.contains("web_fetch"),
            "expected text.contains(\"web_fetch\") to be true"
        );
    }

    #[tokio::test]
    async fn double_activate_is_idempotent() {
        let ctx = mock_ctx_with_catalog(vec![(
            ToolName::from_static("web_search"),
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

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result.content.text_summary().contains("already active"),
            "expected result.content.text_summary().contains(\"already active\") to be true"
        );
    }

    fn mock_ctx_with_server_tools(config: ServerToolConfig) -> ToolContext {
        install_crypto_provider();
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
                knowledge: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: config,
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[tokio::test]
    async fn enable_tool_activates_server_web_search() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: true,
            web_search_max_uses: Some(5),
            code_execution: false,
        });

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'web_search'")
        );

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(
            active.contains(&ToolName::from_static("web_search")),
            "expected active.contains(&ToolName::from_static(\"web_search\")) to be true"
        );
    }

    #[tokio::test]
    async fn enable_tool_server_tool_not_in_disabled_config() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig::default());

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("not found"),
            "expected result.content.text_summary().contains(\"not found\") to be true"
        );
    }

    #[tokio::test]
    async fn enable_tool_lists_server_tools_in_available() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: true,
            web_search_max_uses: None,
            code_execution: true,
        });

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("nonexistent"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        let text = result.content.text_summary();
        assert!(
            text.contains("web_search"),
            "expected text.contains(\"web_search\") to be true"
        );
        assert!(
            text.contains("code_execution"),
            "expected text.contains(\"code_execution\") to be true"
        );
    }
}
