//! Deferred-schemas feature tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse};
use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::types::{InputSchema, PropertyDef, PropertyType, ToolCategory, ToolDef};

use super::*;

/// Wrapper so we can keep an `Arc<MockProvider>` around after registering it.
struct ArcMock(Arc<MockProvider>);

impl LlmProvider for ArcMock {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.0.complete(request)
    }

    fn supported_models(&self) -> &[&str] {
        self.0.supported_models()
    }

    fn name(&self) -> &str {
        self.0.name()
    }
}

fn make_big_schema_tool_def(name: &str) -> ToolDef {
    let mut properties = indexmap::IndexMap::default();
    for i in 0..50 {
        properties.insert(
            format!("field_{i}"),
            PropertyDef {
                property_type: PropertyType::String,
                description: format!(
                    "A very long description for field {i} that contributes many bytes \
                     to the overall schema size when serialized into the LLM tool block"
                ),
                enum_values: Some(vec![
                    "option_a".to_owned(),
                    "option_b".to_owned(),
                    "option_c".to_owned(),
                    "option_d".to_owned(),
                    "option_e".to_owned(),
                ]),
                default: None,
                ..Default::default()
            },
        );
    }
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool with big schema: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties,
            required: (0..50).map(|i| format!("field_{i}")).collect(),
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::Irreversible,
        auto_activate: true,
        groups: vec![organon::types::ToolGroupId::Read],
        tags: vec![],
    }
}

#[cfg(feature = "deferred-schemas")]
#[tokio::test]
async fn execute_uses_summaries_when_deferred_schemas_enabled() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![make_text_response("ok")]).models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMock(Arc::clone(&mock))));

    let mut registry = ToolRegistry::new();
    registry
        .register(
            make_big_schema_tool_def("big_schema_tool"),
            Box::new(EchoExecutor),
        )
        .expect("register");

    let tool_ctx = test_tool_ctx();
    let _result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &registry,
        &tool_ctx,
        None,
    )
    .await
    .expect("execute");

    let requests = mock.captured_requests();
    assert_eq!(requests.len(), 1, "should have captured one request");
    let captured_tools = &requests.first().expect("one request").tools;

    let active = std::collections::HashSet::new();
    let full_defs = registry.to_hermeneus_tools_filtered(&active);
    let full_bytes = serde_json::to_string(&full_defs).map_or(0, |s| s.len());

    let captured_bytes = serde_json::to_string(captured_tools).map_or(0, |s| s.len());

    assert!(
        captured_bytes * 2 <= full_bytes,
        "captured tool block ({captured_bytes}B) should be at least 50% smaller \
         than full schema ({full_bytes}B)"
    );

    for tool in captured_tools {
        assert_eq!(
            tool.input_schema,
            serde_json::json!({"type": "object", "properties": {}, "required": []}),
            "deferred-schemas mode should emit empty schema placeholder"
        );
    }
}

#[cfg(not(feature = "deferred-schemas"))]
#[tokio::test]
async fn execute_uses_full_schemas_by_default() {
    let mock = Arc::new(
        MockProvider::with_responses(vec![make_text_response("ok")]).models(&["test-model"]),
    );
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcMock(Arc::clone(&mock))));

    let mut registry = ToolRegistry::new();
    registry
        .register(
            make_big_schema_tool_def("big_schema_tool"),
            Box::new(EchoExecutor),
        )
        .expect("register");

    let tool_ctx = test_tool_ctx();
    let _result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &registry,
        &tool_ctx,
        None,
    )
    .await
    .expect("execute");

    let requests = mock.captured_requests();
    assert_eq!(requests.len(), 1, "should have captured one request");
    let captured_tools = &requests.first().expect("one request").tools;

    let active = std::collections::HashSet::new();
    let full_defs = registry.to_hermeneus_tools_filtered(&active);
    let full_bytes = serde_json::to_string(&full_defs).map_or(0, |s| s.len());

    let captured_bytes = serde_json::to_string(captured_tools).map_or(0, |s| s.len());

    assert_eq!(
        captured_bytes, full_bytes,
        "feature-off path should send full schemas unchanged"
    );

    for tool in captured_tools {
        assert_ne!(
            tool.input_schema,
            serde_json::json!({"type": "object", "properties": {}, "required": []}),
            "default path should NOT emit empty schema placeholder"
        );
    }
}
