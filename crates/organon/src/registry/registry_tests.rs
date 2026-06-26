use std::sync::{Arc, Mutex, RwLock};

use koina::id::{NousId, SessionId};

use super::*;
use crate::surface::ENABLE_TOOL;
use crate::types::{
    ApprovalRequirement, InputSchema, PropertyDef, PropertyType, Reversibility, ToolCallCapability,
    ToolCallCapabilityRule,
};

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
        turn_number: 0,
        workspace: std::path::PathBuf::from("/tmp/test"),
        allowed_roots: vec![std::path::PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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

fn capability(groups: Vec<ToolGroupId>, reversibility: Reversibility) -> ToolCallCapability {
    ToolCallCapability::new(groups, reversibility)
}

fn tool_input(name: &'static str, arguments: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::from_static(name),
        tool_use_id: format!("toolu_{name}"),
        arguments,
    }
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

#[tokio::test]
async fn execute_rejects_invalid_input_before_dispatch() {
    let mut reg = ToolRegistry::new();
    let (exec, calls) = mock_executor("hello");
    let def = ToolDef {
        name: ToolName::new("typed").expect("valid test tool name"),
        description: "Test tool with schema".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "count".to_owned(),
                PropertyDef {
                    property_type: PropertyType::Integer,
                    description: "Integer count".to_owned(),
                    ..Default::default()
                },
            )]),
            required: vec!["count".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
    };
    reg.register(def, exec).expect("register");

    let input = ToolInput {
        name: ToolName::from_static("typed"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"count": "five"}),
    };
    let err = reg.execute(&input, &mock_ctx()).await.expect_err("invalid input");
    assert!(
        err.to_string().contains("expected integer, got string"),
        "expected schema validation error, got: {err}"
    );
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let call_count = calls.lock().expect("lock poisoned").len();
    assert_eq!(call_count, 0, "expected executor not to be called");
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
                    ..Default::default(),
                },
            )]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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
        make_def_with_activate(ENABLE_TOOL, ToolCategory::System, true),
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
        names.contains(&ENABLE_TOOL),
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
        make_def_with_activate(ENABLE_TOOL, ToolCategory::System, false),
        e1,
    )
    .expect("register");

    let active = HashSet::new();
    let tools = reg.to_hermeneus_tools_filtered(&active);
    assert_eq!(tools.len(), 1, "expected tools.len() to equal 1");
    assert_eq!(
        tools[0].name, ENABLE_TOOL,
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
                    ..Default::default(),
                },
            )]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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
                    ..Default::default(),
                },
            )]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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
                    ..Default::default(),
                },
            )]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
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
            groups: vec![ToolGroupId::Read],
            tags: vec![],
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
        make_def_with_activate(ENABLE_TOOL, ToolCategory::System, true),
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

#[test]
fn definitions_for_groups_filters_by_intersection() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");

    let mut read_def = make_def("read", ToolCategory::Workspace);
    read_def.groups = vec![ToolGroupId::Read];
    reg.register(read_def, e1).expect("register");

    let mut edit_def = make_def("write", ToolCategory::Workspace);
    edit_def.groups = vec![ToolGroupId::Edit];
    reg.register(edit_def, e2).expect("register");

    let mut multi_def = make_def("exec", ToolCategory::Workspace);
    multi_def.groups = vec![ToolGroupId::Read, ToolGroupId::Command];
    reg.register(multi_def, e3).expect("register");

    let read_only = reg.definitions_for_policy(&ToolGroupPolicy::groups(vec![ToolGroupId::Read]));
    let names: Vec<&str> = read_only.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"read"), "read should be in read-only set");
    assert!(
        !names.contains(&"write"),
        "write should not be in read-only set"
    );
    assert!(
        names.contains(&"exec"),
        "exec should be in read-only set (has Read)"
    );

    let edit_cmd = reg.definitions_for_policy(&ToolGroupPolicy::groups(vec![
        ToolGroupId::Edit,
        ToolGroupId::Command,
    ]));
    let names: Vec<&str> = edit_cmd.iter().map(|d| d.name.as_str()).collect();
    assert!(
        !names.contains(&"read"),
        "read should not be in edit+command set"
    );
    assert!(
        names.contains(&"write"),
        "write should be in edit+command set"
    );
    assert!(
        names.contains(&"exec"),
        "exec should be in edit+command set (has Command)"
    );
}

#[test]
fn definitions_for_groups_empty_list_denies_all() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let mut def = make_def("read", ToolCategory::Workspace);
    def.groups = vec![ToolGroupId::Read];
    reg.register(def, e1).expect("register");

    let all = reg.definitions_for_groups(&[]);
    assert!(all.is_empty(), "empty allowed groups should deny all tools");
}

#[test]
fn definitions_for_groups_denies_tools_with_empty_groups() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let mut def = make_def("legacy", ToolCategory::Workspace);
    def.groups = vec![];
    reg.register(def, e1).expect("register");

    let filtered = reg.definitions_for_policy(&ToolGroupPolicy::groups(vec![ToolGroupId::Read]));
    assert_eq!(
        filtered.len(),
        0,
        "tools with empty groups should be denied under group policies"
    );
}

#[tokio::test]
async fn execute_checked_allows_tool_in_group() {
    let mut reg = ToolRegistry::new();
    let (exec, calls) = mock_executor("hello");
    let mut def = make_def("read", ToolCategory::Workspace);
    def.groups = vec![ToolGroupId::Read];
    reg.register(def, exec).expect("register");

    let input = ToolInput {
        name: ToolName::from_static("read"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = reg
        .execute_checked(
            &input,
            &mock_ctx(),
            "coder",
            &ToolGroupPolicy::groups(vec![ToolGroupId::Read]),
        )
        .await
        .expect("execute_checked should succeed");
    assert_eq!(result.content.text_summary(), "hello");
    assert!(!result.is_error);
    let call_count = calls.lock().expect("lock poisoned").len();
    assert_eq!(call_count, 1);
}

#[tokio::test]
async fn execute_checked_denies_tool_outside_group() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("hello");
    let mut def = make_def("write", ToolCategory::Workspace);
    def.groups = vec![ToolGroupId::Edit];
    reg.register(def, exec).expect("register");

    let input = ToolInput {
        name: ToolName::from_static("write"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let err = reg
        .execute_checked(
            &input,
            &mock_ctx(),
            "explorer",
            &ToolGroupPolicy::groups(vec![ToolGroupId::Read]),
        )
        .await
        .expect_err("execute_checked should fail for out-of-group tool");
    assert!(
        err.to_string().contains("tool group violation"),
        "error should mention tool group violation: {err}"
    );
}

#[tokio::test]
async fn execute_checked_empty_groups_denies_all() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("hello");
    let mut def = make_def("write", ToolCategory::Workspace);
    def.groups = vec![ToolGroupId::Edit];
    reg.register(def, exec).expect("register");

    let input = ToolInput {
        name: ToolName::from_static("write"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let err = reg
        .execute_checked(&input, &mock_ctx(), "legacy", &ToolGroupPolicy::DenyAll)
        .await
        .expect_err("execute_checked should fail with deny-all policy");
    assert!(
        err.to_string().contains("tool group violation"),
        "error should mention tool group violation: {err}"
    );
}

#[tokio::test]
async fn execute_checked_allow_all_grants_tool() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("hello");
    let mut def = make_def("write", ToolCategory::Workspace);
    def.groups = vec![ToolGroupId::Edit];
    reg.register(def, exec).expect("register");

    let input = ToolInput {
        name: ToolName::from_static("write"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = reg
        .execute_checked(
            &input,
            &mock_ctx(),
            "admin",
            &ToolGroupPolicy::AllowAll {
                reason: "test admin".to_owned(),
            },
        )
        .await
        .expect("execute_checked should succeed with allow-all policy");
    assert_eq!(result.content.text_summary(), "hello");
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "test covers the mixed-tool call capability policy matrix"
)]
async fn execute_checked_enforces_call_capability_for_mixed_tools() {
    let mut reg = ToolRegistry::new();
    let (note_exec, note_calls) = mock_executor("note ok");
    let (blackboard_exec, blackboard_calls) = mock_executor("blackboard ok");
    let (fact_exec, fact_calls) = mock_executor("fact ok");

    let mut note_def = make_def("note", ToolCategory::Memory);
    note_def.groups = vec![ToolGroupId::Read, ToolGroupId::Edit];
    reg.register_with_call_capability(
        note_def,
        ToolCallCapabilityRule::argument_value(
            "action",
            [
                (
                    "add",
                    capability(vec![ToolGroupId::Edit], Reversibility::Reversible),
                ),
                (
                    "list",
                    capability(vec![ToolGroupId::Read], Reversibility::FullyReversible),
                ),
                (
                    "delete",
                    capability(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
                ),
            ],
        ),
        note_exec,
    )
    .expect("register note");

    let mut blackboard_def = make_def("blackboard", ToolCategory::Memory);
    blackboard_def.groups = vec![ToolGroupId::Read, ToolGroupId::Edit];
    reg.register_with_call_capability(
        blackboard_def,
        ToolCallCapabilityRule::argument_value(
            "action",
            [
                (
                    "write",
                    capability(vec![ToolGroupId::Edit], Reversibility::Reversible),
                ),
                (
                    "read",
                    capability(vec![ToolGroupId::Read], Reversibility::FullyReversible),
                ),
                (
                    "delete",
                    capability(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
                ),
            ],
        ),
        blackboard_exec,
    )
    .expect("register blackboard");

    let mut fact_def = make_def("architecture_fact", ToolCategory::Research);
    fact_def.groups = vec![ToolGroupId::Read, ToolGroupId::Edit, ToolGroupId::Plan];
    reg.register_with_call_capability(
        fact_def,
        ToolCallCapabilityRule::argument_value(
            "op",
            [
                (
                    "get",
                    capability(
                        vec![ToolGroupId::Read, ToolGroupId::Plan],
                        Reversibility::FullyReversible,
                    ),
                ),
                (
                    "put",
                    capability(
                        vec![ToolGroupId::Edit, ToolGroupId::Plan],
                        Reversibility::PartiallyReversible,
                    ),
                ),
            ],
        ),
        fact_exec,
    )
    .expect("register architecture_fact");

    let read_policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
    reg.execute_checked(
        &tool_input("note", serde_json::json!({"action": "list"})),
        &mock_ctx(),
        "reader",
        &read_policy,
    )
    .await
    .expect("read operation allowed");

    for input in [
        tool_input("note", serde_json::json!({"action": "add"})),
        tool_input("note", serde_json::json!({"action": "delete"})),
        tool_input("blackboard", serde_json::json!({"action": "write"})),
        tool_input("blackboard", serde_json::json!({"action": "delete"})),
        tool_input("architecture_fact", serde_json::json!({"op": "put"})),
    ] {
        let err = reg
            .execute_checked(&input, &mock_ctx(), "reader", &read_policy)
            .await
            .expect_err("write operation should be denied");
        assert!(
            err.to_string().contains("tool group violation"),
            "error should mention tool group violation: {err}"
        );
    }

    assert_eq!(note_calls.lock().expect("lock poisoned").len(), 1);
    assert_eq!(blackboard_calls.lock().expect("lock poisoned").len(), 0);
    assert_eq!(fact_calls.lock().expect("lock poisoned").len(), 0);
}

#[test]
fn call_capability_drives_approval_requirement() {
    let mut reg = ToolRegistry::new();
    let (exec, _) = mock_executor("ok");
    let mut def = make_def("note", ToolCategory::Memory);
    def.groups = vec![ToolGroupId::Read, ToolGroupId::Edit];
    reg.register_with_call_capability(
        def,
        ToolCallCapabilityRule::argument_value(
            "action",
            [
                (
                    "list",
                    capability(vec![ToolGroupId::Read], Reversibility::FullyReversible),
                ),
                (
                    "add",
                    capability(vec![ToolGroupId::Edit], Reversibility::Reversible),
                ),
                (
                    "delete",
                    capability(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
                ),
            ],
        ),
        exec,
    )
    .expect("register");

    assert_eq!(
        reg.approval_requirement_for_input(&tool_input(
            "note",
            serde_json::json!({"action": "list"})
        ))
        .expect("approval"),
        ApprovalRequirement::None
    );
    assert_eq!(
        reg.approval_requirement_for_input(&tool_input(
            "note",
            serde_json::json!({"action": "add"})
        ))
        .expect("approval"),
        ApprovalRequirement::Advisory
    );
    assert_eq!(
        reg.approval_requirement_for_input(&tool_input(
            "note",
            serde_json::json!({"action": "delete"})
        ))
        .expect("approval"),
        ApprovalRequirement::Required
    );
}

#[test]
fn presentation_matches_execution_policy() {
    let mut reg = ToolRegistry::new();
    let (read_exec, _) = mock_executor("read");
    let (edit_exec, _) = mock_executor("edit");
    let (empty_exec, _) = mock_executor("empty");

    let mut read_def = make_def("read", ToolCategory::Workspace);
    read_def.groups = vec![ToolGroupId::Read];
    reg.register(read_def, read_exec).expect("register read");

    let mut edit_def = make_def("edit", ToolCategory::Workspace);
    edit_def.groups = vec![ToolGroupId::Edit];
    reg.register(edit_def, edit_exec).expect("register edit");

    let mut empty_def = make_def("empty", ToolCategory::Workspace);
    empty_def.groups = vec![];
    reg.register(empty_def, empty_exec).expect("register empty");

    let policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
    let presented: Vec<_> = reg
        .to_hermeneus_tools_for_policy(&policy)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    let executable: Vec<_> = reg
        .definitions()
        .into_iter()
        .filter(|def| policy.permits(&def.groups))
        .map(|def| def.name.as_str().to_owned())
        .collect();

    assert_eq!(presented, executable);
    assert_eq!(presented, vec!["read"]);
}
