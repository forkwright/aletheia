use std::sync::{Arc, Mutex, RwLock};

use aletheia_koina::id::{NousId, SessionId};

use super::*;
use crate::types::{InputSchema, PropertyDef, PropertyType, Reversibility};

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

fn mock_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: std::path::PathBuf::from("/tmp/test"),
        allowed_roots: vec![std::path::PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn make_def(name: &str, category: ToolCategory) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid test tool name"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category,
        reversibility: Reversibility::Irreversible,
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
    reg.register(make_def("read", ToolCategory::Workspace), exec)
        .expect("register");

    let name = ToolName::from_static("read");
    let def = reg.get_def(&name).expect("found");
    assert_eq!(
        def.name.as_str(),
        "read",
        "expected def.name.as_str() to equal \"read\""
    );
}

#[test]
fn duplicate_rejection() {
    let mut reg = ToolRegistry::new();
    let (exec1, _) = mock_executor("ok");
    let (exec2, _) = mock_executor("ok");
    reg.register(make_def("read", ToolCategory::Workspace), exec1)
        .expect("first register");
    let err = reg
        .register(make_def("read", ToolCategory::Workspace), exec2)
        .expect_err("duplicate");
    assert!(
        err.to_string().contains("duplicate tool: read"),
        "expected err.to_string().contains(\"duplicate tool: read\") to be true"
    );
}

#[test]
fn lookup_missing() {
    let reg = ToolRegistry::new();
    let name = ToolName::from_static("nonexistent");
    assert!(
        reg.get_def(&name).is_none(),
        "expected reg.get_def(&name).is_none() to be true"
    );
}

#[tokio::test]
async fn execute_dispatches_correctly() {
    let mut reg = ToolRegistry::new();
    let (exec, calls) = mock_executor("hello");
    reg.register(make_def("greet", ToolCategory::System), exec)
        .expect("register");

    let input = ToolInput {
        name: ToolName::from_static("greet"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
    assert_eq!(
        result.content.text_summary(),
        "hello",
        "expected result.content.text_summary() to equal \"hello\""
    );
    assert!(!result.is_error, "expected result.is_error to be false");
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let call_count = calls.lock().expect("lock poisoned").len();
    assert_eq!(call_count, 1, "expected call_count to equal 1");
}

#[tokio::test]
async fn execute_not_found() {
    let reg = ToolRegistry::new();
    let input = ToolInput {
        name: ToolName::from_static("missing"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let err = reg.execute(&input, &mock_ctx()).await.expect_err("missing");
    assert!(
        err.to_string().contains("tool not found: missing"),
        "expected err.to_string().contains(\"tool not found: missing\") to be true"
    );
}

#[test]
fn category_filtering() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(make_def("read", ToolCategory::Workspace), e1)
        .expect("register");
    reg.register(make_def("write", ToolCategory::Workspace), e2)
        .expect("register");
    reg.register(make_def("note", ToolCategory::Memory), e3)
        .expect("register");

    let ws = reg.definitions_for_category(ToolCategory::Workspace);
    assert_eq!(ws.len(), 2, "expected ws.len() to equal 2");
    let mem = reg.definitions_for_category(ToolCategory::Memory);
    assert_eq!(mem.len(), 1, "expected mem.len() to equal 1");
    let comm = reg.definitions_for_category(ToolCategory::Communication);
    assert!(comm.is_empty(), "expected comm.is_empty() to be true");
}

#[test]
fn definitions_returns_all() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    reg.register(make_def("a", ToolCategory::Workspace), e1)
        .expect("register");
    reg.register(make_def("b", ToolCategory::Memory), e2)
        .expect("register");
    assert_eq!(
        reg.definitions().len(),
        2,
        "expected reg.definitions().len() to equal 2"
    );
}

#[test]
fn to_hermeneus_tools_produces_valid_definitions() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    let def = ToolDef {
        name: ToolName::from_static("read"),
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
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    };
    reg.register(def, exec).expect("register");

    let tools = reg.to_hermeneus_tools();
    assert_eq!(tools.len(), 1, "expected tools.len() to equal 1");
    assert_eq!(
        tools[0].name, "read",
        "expected tools[0].name to equal \"read\""
    );
    assert_eq!(
        tools[0].description, "Read a file",
        "expected tools[0].description to equal \"Read a file\""
    );
    assert_eq!(
        tools[0].input_schema["type"], "object",
        "expected tools[0].input_schema[\"type\"] to equal \"object\""
    );
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
    reg.register(make_def("ctx-test", ToolCategory::System), executor)
        .expect("register");

    let input = ToolInput {
        name: ToolName::from_static("ctx-test"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    reg.execute(&input, &mock_ctx()).await.expect("execute");

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let id = captured.lock().expect("lock poisoned").clone();
    assert_eq!(
        id.as_deref(),
        Some("test-agent"),
        "expected id.as_deref() to equal Some(\"test-agent\")"
    );
}

fn make_def_with_activate(name: &str, category: ToolCategory, auto_activate: bool) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid test tool name"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category,
        reversibility: Reversibility::Irreversible,
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
        make_def_with_activate("read", ToolCategory::Workspace, true),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_activate("web_search", ToolCategory::Research, false),
        e2,
    )
    .expect("register");
    reg.register(
        make_def_with_activate("enable_tool", ToolCategory::System, true),
        e3,
    )
    .expect("register");

    let active = HashSet::new();
    let tools = reg.to_hermeneus_tools_filtered(&active);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"read"),
        "expected names.contains(&\"read\") to be true"
    );
    assert!(
        names.contains(&"enable_tool"),
        "expected names.contains(&\"enable_tool\") to be true"
    );
    assert!(
        !names.contains(&"web_search"),
        "expected names.contains(&\"web_search\") to be false"
    );
}

#[test]
fn filtered_tools_includes_active_set() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    reg.register(
        make_def_with_activate("read", ToolCategory::Workspace, true),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_activate("web_search", ToolCategory::Research, false),
        e2,
    )
    .expect("register");

    let mut active = HashSet::new();
    active.insert(ToolName::from_static("web_search"));
    let tools = reg.to_hermeneus_tools_filtered(&active);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"read"),
        "expected names.contains(&\"read\") to be true"
    );
    assert!(
        names.contains(&"web_search"),
        "expected names.contains(&\"web_search\") to be true"
    );
}

#[test]
fn filtered_tools_always_includes_enable_tool() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    reg.register(
        make_def_with_activate("enable_tool", ToolCategory::System, false),
        e1,
    )
    .expect("register");

    let active = HashSet::new();
    let tools = reg.to_hermeneus_tools_filtered(&active);
    assert_eq!(tools.len(), 1, "expected tools.len() to equal 1");
    assert_eq!(
        tools[0].name, "enable_tool",
        "expected tools[0].name to equal \"enable_tool\""
    );
}

#[test]
fn empty_registry_has_no_definitions() {
    let reg = ToolRegistry::new();
    assert!(
        reg.definitions().is_empty(),
        "expected reg.definitions().is_empty() to be true"
    );
}

#[test]
fn default_registry_equals_new_registry() {
    let reg1 = ToolRegistry::new();
    let reg2 = ToolRegistry::default();
    assert_eq!(
        reg1.definitions().len(),
        reg2.definitions().len(),
        "expected reg1.definitions().len() to equal reg2.definitions().len()"
    );
}

#[test]
fn definitions_preserves_insertion_order() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(make_def("alpha", ToolCategory::Workspace), e1)
        .expect("register");
    reg.register(make_def("beta", ToolCategory::Workspace), e2)
        .expect("register");
    reg.register(make_def("gamma", ToolCategory::Workspace), e3)
        .expect("register");

    let names: Vec<&str> = reg.definitions().iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        ["alpha", "beta", "gamma"],
        "expected names to equal [\"alpha\", \"beta\", \"gamma\"]"
    );
}

#[test]
fn schema_includes_required_fields() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    let def = ToolDef {
        name: ToolName::from_static("read"),
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
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    };
    reg.register(def, exec).expect("register");
    let tools = reg.to_hermeneus_tools();
    let schema = &tools[0].input_schema;
    assert_eq!(
        schema["required"][0], "path",
        "expected schema[\"required\"][0] to equal \"path\""
    );
}

#[test]
fn schema_includes_enum_values() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    let def = ToolDef {
        name: ToolName::from_static("find"),
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
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    };
    reg.register(def, exec).expect("register");
    let tools = reg.to_hermeneus_tools();
    let schema = &tools[0].input_schema;
    let enum_vals = &schema["properties"]["type"]["enum"];
    assert_eq!(enum_vals[0], "f", "expected enum_vals[0] to equal \"f\"");
    assert_eq!(enum_vals[1], "d", "expected enum_vals[1] to equal \"d\"");
}

#[test]
fn schema_includes_default_values() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    let def = ToolDef {
        name: ToolName::from_static("grep"),
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
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    };
    reg.register(def, exec).expect("register");
    let tools = reg.to_hermeneus_tools();
    let schema = &tools[0].input_schema;
    assert_eq!(
        schema["properties"]["caseSensitive"]["default"], true,
        "expected schema[\"properties\"][\"caseSensitive\"]... to equal true"
    );
}

#[test]
fn lazy_catalog_includes_description() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    reg.register(
        ToolDef {
            name: ToolName::from_static("web_search"),
            description: "Search the web".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::new(),
                required: vec![],
            },
            category: ToolCategory::Research,
            reversibility: Reversibility::Irreversible,
            auto_activate: false,
        },
        exec,
    )
    .expect("register");

    let catalog = reg.lazy_tool_catalog();
    assert_eq!(catalog.len(), 1, "expected catalog.len() to equal 1");
    assert_eq!(
        catalog[0].1, "Search the web",
        "expected catalog[0].1 to equal \"Search the web\""
    );
}

#[test]
fn definitions_for_category_returns_empty_when_no_match() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    reg.register(make_def("read", ToolCategory::Workspace), e1)
        .expect("register");
    let planning = reg.definitions_for_category(ToolCategory::Planning);
    assert!(
        planning.is_empty(),
        "expected planning.is_empty() to be true"
    );
}

#[tokio::test]
async fn execute_returns_tool_not_found_for_unknown_name() {
    let reg = ToolRegistry::new();
    let input = ToolInput {
        name: ToolName::from_static("ghost"),
        tool_use_id: "toolu_x".to_owned(),
        arguments: serde_json::json!({}),
    };
    let err = reg
        .execute(&input, &mock_ctx())
        .await
        .expect_err("not found");
    assert!(
        err.to_string().contains("tool not found: ghost"),
        "expected err.to_string().contains(\"tool not found: ghost\") to be true"
    );
}

#[test]
fn lazy_tool_catalog_excludes_auto_activate_and_enable_tool() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(
        make_def_with_activate("read", ToolCategory::Workspace, true),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_activate("web_search", ToolCategory::Research, false),
        e2,
    )
    .expect("register");
    reg.register(
        make_def_with_activate("enable_tool", ToolCategory::System, true),
        e3,
    )
    .expect("register");

    let catalog = reg.lazy_tool_catalog();
    assert_eq!(catalog.len(), 1, "expected catalog.len() to equal 1");
    assert_eq!(
        catalog[0].0.as_str(),
        "web_search",
        "expected catalog[0].0.as_str() to equal \"web_search\""
    );
}
