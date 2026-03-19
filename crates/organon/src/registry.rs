//! Tool registry: the single source of truth for available tools.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use indexmap::IndexMap;
use snafu::ensure;
use tracing::info_span;

use aletheia_koina::id::ToolName;

use crate::error::{self, Result};
use crate::types::{ToolCategory, ToolContext, ToolDef, ToolInput, ToolResult};

/// The trait tool implementations must satisfy.
///
/// Uses `Pin<Box<dyn Future>>` for object-safety with async dispatch.
pub trait ToolExecutor: Send + Sync {
    /// Execute the tool with the given input and context.
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}

struct RegisteredTool {
    def: ToolDef,
    executor: Box<dyn ToolExecutor>,
}

/// Registry of available tools.
///
/// Tools are registered at startup and looked up by name during execution.
/// The registry is the single source of truth for what tools an agent can use.
pub struct ToolRegistry {
    tools: IndexMap<ToolName, RegisteredTool>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: IndexMap::new(),
        }
    }

    /// Register a tool. Fails if a tool with the same name already exists.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::DuplicateTool`] if a tool with the same name is already registered.
    pub fn register(&mut self, def: ToolDef, executor: Box<dyn ToolExecutor>) -> Result<()> {
        ensure!(
            !self.tools.contains_key(&def.name),
            error::DuplicateToolSnafu {
                name: def.name.clone()
            }
        );
        self.tools
            .insert(def.name.clone(), RegisteredTool { def, executor });
        Ok(())
    }

    /// Look up a tool definition by name.
    #[must_use]
    pub fn get_def(&self, name: &ToolName) -> Option<&ToolDef> {
        self.tools.get(name).map(|t| &t.def)
    }

    /// Execute a tool by name.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ToolNotFound`] if no tool with the given name is registered.
    /// Returns the tool executor's error if execution fails.
    pub async fn execute(&self, input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult> {
        let tool = self.tools.get(&input.name).ok_or_else(|| {
            error::ToolNotFoundSnafu {
                name: input.name.clone(),
            }
            .build()
        })?;
        let span = info_span!("tool_execute",
            tool.name = %input.name,
            tool.duration_ms = tracing::field::Empty,
            tool.status = tracing::field::Empty,
        );
        let _guard = span.enter();
        let start = Instant::now();
        let result = tool.executor.execute(input, ctx).await;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: tool duration fits in u64"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;
        span.record("tool.duration_ms", duration_ms);
        match &result {
            Ok(r) if !r.is_error => span.record("tool.status", "ok"),
            _ => span.record("tool.status", "error"),
        };
        crate::metrics::record_invocation(
            input.name.as_str(),
            start.elapsed().as_secs_f64(),
            matches!(&result, Ok(r) if !r.is_error),
        );
        result
    }

    /// All registered tool definitions, in insertion order.
    #[must_use]
    pub fn definitions(&self) -> Vec<&ToolDef> {
        self.tools.values().map(|t| &t.def).collect()
    }

    /// Tool definitions filtered by category.
    #[must_use]
    pub fn definitions_for_category(&self, category: ToolCategory) -> Vec<&ToolDef> {
        self.tools
            .values()
            .filter(|t| t.def.category == category)
            .map(|t| &t.def)
            .collect()
    }

    /// Convert registered tools to the LLM wire format.
    ///
    /// Produces `ToolDefinition` structs suitable for `CompletionRequest::tools`.
    pub fn to_hermeneus_tools(&self) -> Vec<aletheia_hermeneus::types::ToolDefinition> {
        self.tools
            .values()
            .map(|t| aletheia_hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format, filtered by activation state.
    ///
    /// Includes tools where:
    /// - `auto_activate == true` (always-on essentials)
    /// - name is in the `active` set (dynamically activated via `enable_tool`)
    /// - name is `enable_tool` (always available so agents can activate more)
    pub fn to_hermeneus_tools_filtered(
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<aletheia_hermeneus::types::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| {
                t.def.auto_activate
                    || active.contains(&t.def.name)
                    || t.def.name.as_str() == "enable_tool"
            })
            .map(|t| aletheia_hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Catalog of lazy tools (`auto_activate=false`) for the `enable_tool` executor.
    #[must_use]
    pub fn lazy_tool_catalog(&self) -> Vec<(ToolName, String)> {
        self.tools
            .values()
            .filter(|t| !t.def.auto_activate && t.def.name.as_str() != "enable_tool")
            .map(|t| (t.def.name.clone(), t.def.description.clone()))
            .collect()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use aletheia_koina::id::{NousId, SessionId};

    use super::*;
    use crate::types::{InputSchema, PropertyDef, PropertyType};

    /// Mock executor that captures calls for verification.
    struct MockExecutor {
        calls: Arc<Mutex<Vec<ToolName>>>,
        response: String,
    }

    impl ToolExecutor for MockExecutor {
        fn execute<'a>(
            &'a self,
            input: &'a ToolInput,
            _ctx: &'a ToolContext,
        ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
            Box::pin(async {
                #[expect(
                    clippy::expect_used,
                    reason = "test mock: poisoned lock means a test bug"
                )]
                self.calls
                    .lock()
                    .expect("lock poisoned")
                    .push(input.name.clone());
                Ok(ToolResult::text(self.response.clone()))
            })
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn test_def(name: &str, category: ToolCategory) -> ToolDef {
        ToolDef {
            name: ToolName::new(name).expect("valid"),
            description: format!("Test tool: {name}"),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::new(),
                required: vec![],
            },
            category,
            auto_activate: false,
        }
    }

    fn mock_executor(response: &str) -> (Box<dyn ToolExecutor>, Arc<Mutex<Vec<ToolName>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = Box::new(MockExecutor {
            calls: Arc::clone(&calls),
            response: response.to_owned(),
        });
        (executor, calls)
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        reg.register(test_def("read", ToolCategory::Workspace), exec)
            .expect("register");

        let name = ToolName::new("read").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(def.name.as_str(), "read");
    }

    #[test]
    fn duplicate_rejection() {
        let mut reg = ToolRegistry::new();
        let (exec1, _) = mock_executor("ok");
        let (exec2, _) = mock_executor("ok");
        reg.register(test_def("read", ToolCategory::Workspace), exec1)
            .expect("first register");
        let err = reg
            .register(test_def("read", ToolCategory::Workspace), exec2)
            .expect_err("duplicate");
        assert!(err.to_string().contains("duplicate tool: read"));
    }

    #[test]
    fn lookup_missing() {
        let reg = ToolRegistry::new();
        let name = ToolName::new("nonexistent").expect("valid");
        assert!(reg.get_def(&name).is_none());
    }

    #[tokio::test]
    async fn execute_dispatches_correctly() {
        let mut reg = ToolRegistry::new();
        let (exec, calls) = mock_executor("hello");
        reg.register(test_def("greet", ToolCategory::System), exec)
            .expect("register");

        let input = ToolInput {
            name: ToolName::new("greet").expect("valid"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert_eq!(result.content.text_summary(), "hello");
        assert!(!result.is_error);
        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let call_count = calls.lock().expect("lock poisoned").len();
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn execute_not_found() {
        let reg = ToolRegistry::new();
        let input = ToolInput {
            name: ToolName::new("missing").expect("valid"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({}),
        };
        let err = reg.execute(&input, &test_ctx()).await.expect_err("missing");
        assert!(err.to_string().contains("tool not found: missing"));
    }

    #[test]
    fn category_filtering() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        let (e3, _) = mock_executor("ok");
        reg.register(test_def("read", ToolCategory::Workspace), e1)
            .expect("register");
        reg.register(test_def("write", ToolCategory::Workspace), e2)
            .expect("register");
        reg.register(test_def("note", ToolCategory::Memory), e3)
            .expect("register");

        let ws = reg.definitions_for_category(ToolCategory::Workspace);
        assert_eq!(ws.len(), 2);
        let mem = reg.definitions_for_category(ToolCategory::Memory);
        assert_eq!(mem.len(), 1);
        let comm = reg.definitions_for_category(ToolCategory::Communication);
        assert!(comm.is_empty());
    }

    #[test]
    fn definitions_returns_all() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        reg.register(test_def("a", ToolCategory::Workspace), e1)
            .expect("register");
        reg.register(test_def("b", ToolCategory::Memory), e2)
            .expect("register");
        assert_eq!(reg.definitions().len(), 2);
    }

    #[test]
    fn to_hermeneus_tools_produces_valid_definitions() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        let def = ToolDef {
            name: ToolName::new("read").expect("valid"),
            description: "Read a file".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::from([(
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                )]),
                required: vec!["path".to_owned()],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        };
        reg.register(def, exec).expect("register");

        let tools = reg.to_hermeneus_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read");
        assert_eq!(tools[0].description, "Read a file");
        assert_eq!(tools[0].input_schema["type"], "object");
        assert_eq!(
            tools[0].input_schema["properties"]["path"]["type"],
            "string"
        );
    }

    #[tokio::test]
    async fn context_passed_to_executor() {
        struct CtxCapture {
            captured_nous_id: Arc<Mutex<Option<String>>>,
        }
        impl ToolExecutor for CtxCapture {
            fn execute<'a>(
                &'a self,
                _input: &'a ToolInput,
                ctx: &'a ToolContext,
            ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
                let nous_id = ctx.nous_id.as_str().to_owned();
                let captured = Arc::clone(&self.captured_nous_id);
                Box::pin(async move {
                    #[expect(
                        clippy::expect_used,
                        reason = "test mock: poisoned lock means a test bug"
                    )]
                    {
                        *captured.lock().expect("lock poisoned") = Some(nous_id);
                    }
                    Ok(ToolResult::text("ok"))
                })
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let executor = Box::new(CtxCapture {
            captured_nous_id: Arc::clone(&captured),
        });

        let mut reg = ToolRegistry::new();
        reg.register(test_def("ctx-test", ToolCategory::System), executor)
            .expect("register");

        let input = ToolInput {
            name: ToolName::new("ctx-test").expect("valid"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({}),
        };
        reg.execute(&input, &test_ctx()).await.expect("execute");

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let id = captured.lock().expect("lock poisoned").clone();
        assert_eq!(id.as_deref(), Some("test-agent"));
    }

    fn test_def_with_activate(name: &str, category: ToolCategory, auto_activate: bool) -> ToolDef {
        ToolDef {
            name: ToolName::new(name).expect("valid"),
            description: format!("Test tool: {name}"),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::new(),
                required: vec![],
            },
            category,
            auto_activate,
        }
    }

    #[test]
    fn filtered_tools_respects_auto_activate() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        let (e3, _) = mock_executor("ok");
        reg.register(
            test_def_with_activate("read", ToolCategory::Workspace, true),
            e1,
        )
        .expect("register");
        reg.register(
            test_def_with_activate("web_search", ToolCategory::Research, false),
            e2,
        )
        .expect("register");
        reg.register(
            test_def_with_activate("enable_tool", ToolCategory::System, true),
            e3,
        )
        .expect("register");

        let active = HashSet::new();
        let tools = reg.to_hermeneus_tools_filtered(&active);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"enable_tool"));
        assert!(!names.contains(&"web_search"));
    }

    #[test]
    fn filtered_tools_includes_active_set() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        reg.register(
            test_def_with_activate("read", ToolCategory::Workspace, true),
            e1,
        )
        .expect("register");
        reg.register(
            test_def_with_activate("web_search", ToolCategory::Research, false),
            e2,
        )
        .expect("register");

        let mut active = HashSet::new();
        active.insert(ToolName::new("web_search").expect("valid"));
        let tools = reg.to_hermeneus_tools_filtered(&active);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"web_search"));
    }

    #[test]
    fn filtered_tools_always_includes_enable_tool() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        // enable_tool with auto_activate=false should still appear
        reg.register(
            test_def_with_activate("enable_tool", ToolCategory::System, false),
            e1,
        )
        .expect("register");

        let active = HashSet::new();
        let tools = reg.to_hermeneus_tools_filtered(&active);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "enable_tool");
    }

    #[test]
    fn empty_registry_has_no_definitions() {
        let reg = ToolRegistry::new();
        assert!(reg.definitions().is_empty());
    }

    #[test]
    fn default_registry_equals_new_registry() {
        let reg1 = ToolRegistry::new();
        let reg2 = ToolRegistry::default();
        assert_eq!(reg1.definitions().len(), reg2.definitions().len());
    }

    #[test]
    fn definitions_preserves_insertion_order() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        let (e3, _) = mock_executor("ok");
        reg.register(test_def("alpha", ToolCategory::Workspace), e1)
            .expect("register");
        reg.register(test_def("beta", ToolCategory::Workspace), e2)
            .expect("register");
        reg.register(test_def("gamma", ToolCategory::Workspace), e3)
            .expect("register");

        let names: Vec<&str> = reg.definitions().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn schema_includes_required_fields() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        let def = ToolDef {
            name: ToolName::new("read").expect("valid"),
            description: "Read a file".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::from([(
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                )]),
                required: vec!["path".to_owned()],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        };
        reg.register(def, exec).expect("register");
        let tools = reg.to_hermeneus_tools();
        let schema = &tools[0].input_schema;
        assert_eq!(schema["required"][0], "path");
    }

    #[test]
    fn schema_includes_enum_values() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        let def = ToolDef {
            name: ToolName::new("find").expect("valid"),
            description: "Find files".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::from([(
                    "type".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Type filter".to_owned(),
                        enum_values: Some(vec!["f".to_owned(), "d".to_owned()]),
                        default: None,
                    },
                )]),
                required: vec![],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        };
        reg.register(def, exec).expect("register");
        let tools = reg.to_hermeneus_tools();
        let schema = &tools[0].input_schema;
        let enum_vals = &schema["properties"]["type"]["enum"];
        assert_eq!(enum_vals[0], "f");
        assert_eq!(enum_vals[1], "d");
    }

    #[test]
    fn schema_includes_default_values() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        let def = ToolDef {
            name: ToolName::new("grep").expect("valid"),
            description: "Grep".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::from([(
                    "caseSensitive".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Case sensitive".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(true)),
                    },
                )]),
                required: vec![],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        };
        reg.register(def, exec).expect("register");
        let tools = reg.to_hermeneus_tools();
        let schema = &tools[0].input_schema;
        assert_eq!(schema["properties"]["caseSensitive"]["default"], true);
    }

    #[test]
    fn lazy_catalog_includes_description() {
        let mut reg = ToolRegistry::new();
        let (exec, _) = mock_executor("ok");
        reg.register(
            ToolDef {
                name: ToolName::new("web_search").expect("valid"),
                description: "Search the web".to_owned(),
                extended_description: None,
                input_schema: InputSchema {
                    properties: IndexMap::new(),
                    required: vec![],
                },
                category: ToolCategory::Research,
                auto_activate: false,
            },
            exec,
        )
        .expect("register");

        let catalog = reg.lazy_tool_catalog();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].1, "Search the web");
    }

    #[test]
    fn definitions_for_category_returns_empty_when_no_match() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        reg.register(test_def("read", ToolCategory::Workspace), e1)
            .expect("register");
        let planning = reg.definitions_for_category(ToolCategory::Planning);
        assert!(planning.is_empty());
    }

    #[tokio::test]
    async fn execute_returns_tool_not_found_for_unknown_name() {
        let reg = ToolRegistry::new();
        let input = ToolInput {
            name: ToolName::new("ghost").expect("valid"),
            tool_use_id: "toolu_x".to_owned(),
            arguments: serde_json::json!({}),
        };
        let err = reg
            .execute(&input, &test_ctx())
            .await
            .expect_err("not found");
        assert!(err.to_string().contains("tool not found: ghost"));
    }

    #[test]
    fn lazy_tool_catalog_excludes_auto_activate_and_enable_tool() {
        let mut reg = ToolRegistry::new();
        let (e1, _) = mock_executor("ok");
        let (e2, _) = mock_executor("ok");
        let (e3, _) = mock_executor("ok");
        reg.register(
            test_def_with_activate("read", ToolCategory::Workspace, true),
            e1,
        )
        .expect("register");
        reg.register(
            test_def_with_activate("web_search", ToolCategory::Research, false),
            e2,
        )
        .expect("register");
        reg.register(
            test_def_with_activate("enable_tool", ToolCategory::System, true),
            e3,
        )
        .expect("register");

        let catalog = reg.lazy_tool_catalog();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].0.as_str(), "web_search");
    }
}
