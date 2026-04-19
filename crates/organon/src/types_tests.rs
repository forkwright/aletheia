#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;

use super::*;

#[test]
fn tool_def_serde_roundtrip() {
    let def = ToolDef {
        name: ToolName::new("test_tool").expect("valid"),
        description: "A test tool".to_owned(),
        extended_description: Some("Detailed description".to_owned()),
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
    let json = serde_json::to_string(&def).expect("serialize");
    let back: ToolDef = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.name.as_str(),
        "test_tool",
        "expected back.name.as_str() to equal \"test_tool\""
    );
    assert_eq!(
        back.category,
        ToolCategory::Workspace,
        "expected back.category to equal ToolCategory::Workspace"
    );
}

#[test]
fn input_schema_to_json_schema() {
    let schema = InputSchema {
        properties: IndexMap::from([
            (
                "path".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "File path".to_owned(),
                    enum_values: None,
                    default: None,
                },
            ),
            (
                "max_lines".to_owned(),
                PropertyDef {
                    property_type: PropertyType::Number,
                    description: "Maximum lines".to_owned(),
                    enum_values: None,
                    default: Some(serde_json::json!(100)),
                },
            ),
        ]),
        required: vec!["path".to_owned()],
    };
    let json_schema = schema.to_json_schema();
    assert_eq!(
        json_schema["type"], "object",
        "expected json_schema[\"type\"] to equal \"object\""
    );
    assert_eq!(
        json_schema["properties"]["path"]["type"], "string",
        "expected json_schema[\"properties\"][\"path\"][\"ty... to equal \"string\""
    );
    assert_eq!(
        json_schema["properties"]["max_lines"]["default"], 100,
        "expected json_schema[\"properties\"][\"max_lines\"... to equal 100"
    );
    assert_eq!(
        json_schema["required"][0], "path",
        "expected json_schema[\"required\"][0] to equal \"path\""
    );
}

#[test]
fn property_type_serde() {
    let json = serde_json::to_string(&PropertyType::Integer).expect("serialize");
    assert_eq!(
        json, "\"integer\"",
        "expected json to equal \"\"integer\"\""
    );
    let back: PropertyType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        PropertyType::Integer,
        "expected back to equal PropertyType::Integer"
    );
}

#[test]
fn tool_category_display() {
    assert_eq!(
        ToolCategory::Workspace.to_string(),
        "workspace",
        "expected ToolCategory::Workspace.to_string() to equal \"workspace\""
    );
    assert_eq!(
        ToolCategory::Communication.to_string(),
        "communication",
        "expected ToolCategory::Communication.to_string() to equal \"communication\""
    );
    assert_eq!(
        ToolCategory::Research.to_string(),
        "research",
        "expected ToolCategory::Research.to_string() to equal \"research\""
    );
}

#[test]
fn tool_stats_record_accumulates() {
    let mut stats = ToolStats::default();
    stats.record("read", 10, false);
    stats.record("write", 20, false);
    stats.record("read", 15, true);
    assert_eq!(
        stats.total_calls, 3,
        "expected stats.total_calls to equal 3"
    );
    assert_eq!(
        stats.total_duration_ms, 45,
        "expected stats.total_duration_ms to equal 45"
    );
    assert_eq!(
        stats.error_count, 1,
        "expected stats.error_count to equal 1"
    );
    assert_eq!(
        stats.calls_by_tool["read"], 2,
        "expected stats.calls_by_tool[\"read\"] to equal 2"
    );
    assert_eq!(
        stats.calls_by_tool["write"], 1,
        "expected stats.calls_by_tool[\"write\"] to equal 1"
    );
}

#[test]
fn tool_stats_top_tools() {
    let mut stats = ToolStats::default();
    stats.record("a", 1, false);
    stats.record("b", 1, false);
    stats.record("b", 1, false);
    stats.record("c", 1, false);
    stats.record("c", 1, false);
    stats.record("c", 1, false);
    let top = stats.top_tools(2);
    assert_eq!(top.len(), 2, "expected top.len() to equal 2");
    assert_eq!(top[0], ("c", 3), "expected top[0] to equal (\"c\", 3)");
    assert_eq!(top[1], ("b", 2), "expected top[1] to equal (\"b\", 2)");
}

#[test]
fn tool_input_serde_roundtrip() {
    let input = ToolInput {
        name: ToolName::new("read").expect("valid"),
        tool_use_id: "toolu_123".to_owned(),
        arguments: serde_json::json!({"path": "/tmp/test.txt"}),
    };
    let json = serde_json::to_string(&input).expect("serialize");
    let back: ToolInput = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.name.as_str(),
        "read",
        "expected back.name.as_str() to equal \"read\""
    );
    assert_eq!(
        back.tool_use_id, "toolu_123",
        "expected back.tool_use_id to equal \"toolu_123\""
    );
}

#[test]
fn tool_result_text_constructor() {
    let r = ToolResult::text("hello");
    assert_eq!(
        r.content.text_summary(),
        "hello",
        "expected r.content.text_summary() to equal \"hello\""
    );
    assert!(!r.is_error, "expected r.is_error to be false");
}

#[test]
fn tool_result_error_constructor() {
    let r = ToolResult::error("bad input");
    assert_eq!(
        r.content.text_summary(),
        "bad input",
        "expected r.content.text_summary() to equal \"bad input\""
    );
    assert!(r.is_error, "expected r.is_error to be true");
}

#[test]
fn tool_result_blocks_constructor() {
    let r = ToolResult::blocks(vec![ToolResultBlock::Text {
        text: "desc".to_owned(),
    }]);
    assert_eq!(
        r.content.text_summary(),
        "desc",
        "expected r.content.text_summary() to equal \"desc\""
    );
    assert!(!r.is_error, "expected r.is_error to be false");
}

#[test]
fn test_input_schema_enum_values_serialized_in_json_schema() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "type".to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: "Type".to_owned(),
                enum_values: Some(vec!["a".to_owned(), "b".to_owned()]),
                default: None,
            },
        )]),
        required: vec![],
    };
    let json = schema.to_json_schema();
    assert_eq!(
        json["properties"]["type"]["enum"][0], "a",
        "expected json[\"properties\"][\"type\"][\"enum\"][0] to equal \"a\""
    );
    assert_eq!(
        json["properties"]["type"]["enum"][1], "b",
        "expected json[\"properties\"][\"type\"][\"enum\"][1] to equal \"b\""
    );
}

#[test]
fn test_input_schema_with_no_required_fields_has_empty_required_array() {
    let schema = InputSchema {
        properties: IndexMap::new(),
        required: vec![],
    };
    let json = schema.to_json_schema();
    let required = json["required"].as_array().expect("array");
    assert!(
        required.is_empty(),
        "expected required.is_empty() to be true"
    );
}

#[test]
fn test_property_type_display_all_variants() {
    assert_eq!(
        PropertyType::String.to_string(),
        "string",
        "expected PropertyType::String.to_string() to equal \"string\""
    );
    assert_eq!(
        PropertyType::Number.to_string(),
        "number",
        "expected PropertyType::Number.to_string() to equal \"number\""
    );
    assert_eq!(
        PropertyType::Integer.to_string(),
        "integer",
        "expected PropertyType::Integer.to_string() to equal \"integer\""
    );
    assert_eq!(
        PropertyType::Boolean.to_string(),
        "boolean",
        "expected PropertyType::Boolean.to_string() to equal \"boolean\""
    );
    assert_eq!(
        PropertyType::Array.to_string(),
        "array",
        "expected PropertyType::Array.to_string() to equal \"array\""
    );
    assert_eq!(
        PropertyType::Object.to_string(),
        "object",
        "expected PropertyType::Object.to_string() to equal \"object\""
    );
}

#[test]
fn test_tool_category_all_categories_have_display() {
    let cases = [
        (ToolCategory::Workspace, "workspace"),
        (ToolCategory::Memory, "memory"),
        (ToolCategory::Communication, "communication"),
        (ToolCategory::Planning, "planning"),
        (ToolCategory::System, "system"),
        (ToolCategory::Agent, "agent"),
        (ToolCategory::Research, "research"),
        (ToolCategory::Domain, "domain"),
    ];
    for (cat, expected) in cases {
        assert_eq!(
            cat.to_string(),
            expected,
            "expected cat.to_string() to equal expected"
        );
    }
}

#[test]
fn test_tool_stats_initial_state_all_zeros() {
    let stats = ToolStats::default();
    assert_eq!(
        stats.total_calls, 0,
        "expected stats.total_calls to equal 0"
    );
    assert_eq!(
        stats.total_duration_ms, 0,
        "expected stats.total_duration_ms to equal 0"
    );
    assert_eq!(
        stats.error_count, 0,
        "expected stats.error_count to equal 0"
    );
    assert!(
        stats.calls_by_tool.is_empty(),
        "expected stats.calls_by_tool.is_empty() to be true"
    );
}

#[test]
fn test_tool_stats_zero_errors_when_all_calls_succeed() {
    let mut stats = ToolStats::default();
    stats.record("read", 10, false);
    stats.record("write", 20, false);
    assert_eq!(
        stats.error_count, 0,
        "expected stats.error_count to equal 0"
    );
}

#[test]
fn test_tool_stats_top_tools_returns_empty_when_no_calls() {
    let stats = ToolStats::default();
    assert!(
        stats.top_tools(5).is_empty(),
        "expected stats.top_tools(5).is_empty() to be true"
    );
}

#[test]
fn test_tool_stats_top_tools_limited_to_n() {
    let mut stats = ToolStats::default();
    for name in ["a", "b", "c", "d", "e"] {
        stats.record(name, 1, false);
    }
    let top = stats.top_tools(3);
    assert_eq!(top.len(), 3, "expected top.len() to equal 3");
}

#[test]
fn test_tool_result_text_is_not_error() {
    let r = ToolResult::text("ok");
    assert!(!r.is_error, "expected r.is_error to be false");
}

#[test]
fn test_tool_result_error_is_error() {
    let r = ToolResult::error("bad");
    assert!(r.is_error, "expected r.is_error to be true");
}

#[test]
fn test_tool_result_blocks_is_not_error() {
    let r = ToolResult::blocks(vec![]);
    assert!(!r.is_error, "expected r.is_error to be false");
}

#[test]
fn test_tool_def_auto_activate_stored_correctly() {
    let def = ToolDef {
        name: ToolName::new("t").expect("valid"),
        description: "d".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: true,
    };
    assert!(def.auto_activate, "expected def.auto_activate to be true");
}

#[test]
fn test_input_schema_type_is_object_in_json_schema() {
    let schema = InputSchema {
        properties: IndexMap::new(),
        required: vec![],
    };
    let json = schema.to_json_schema();
    assert_eq!(
        json["type"], "object",
        "expected json[\"type\"] to equal \"object\""
    );
}

#[test]
fn server_tool_config_default_disables_all() {
    let config = ServerToolConfig::default();
    assert!(!config.web_search, "expected config.web_search to be false");
    assert!(
        !config.code_execution,
        "expected config.code_execution to be false"
    );
    assert!(
        config.web_search_max_uses.is_none(),
        "expected config.web_search_max_uses.is_none() to be true"
    );
}

#[test]
fn server_tool_config_serde_roundtrip() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: Some(5),
        code_execution: true,
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let back: ServerToolConfig = serde_json::from_str(&json).expect("deserialize");
    assert!(back.web_search, "expected back.web_search to be true");
    assert_eq!(
        back.web_search_max_uses,
        Some(5),
        "expected back.web_search_max_uses to equal Some(5)"
    );
    assert!(
        back.code_execution,
        "expected back.code_execution to be true"
    );
}

#[test]
fn server_tool_config_catalog_entries_empty_when_disabled() {
    let config = ServerToolConfig::default();
    assert!(
        config.catalog_entries().is_empty(),
        "expected config.catalog_entries().is_empty() to be true"
    );
}

#[test]
fn server_tool_config_catalog_entries_web_search() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: None,
        code_execution: false,
    };
    let entries = config.catalog_entries();
    assert_eq!(entries.len(), 1, "expected entries.len() to equal 1");
    assert_eq!(
        entries[0].0.as_str(),
        "web_search",
        "expected entries[0].0.as_str() to equal \"web_search\""
    );
}

#[test]
fn server_tool_config_catalog_entries_both() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: Some(3),
        code_execution: true,
    };
    let entries = config.catalog_entries();
    assert_eq!(entries.len(), 2, "expected entries.len() to equal 2");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"web_search"),
        "expected names.contains(&\"web_search\") to be true"
    );
    assert!(
        names.contains(&"code_execution"),
        "expected names.contains(&\"code_execution\") to be true"
    );
}

#[test]
fn server_tool_config_active_definitions_empty_when_none_active() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: Some(5),
        code_execution: true,
    };
    let active = HashSet::new();
    let defs = config.active_definitions(&active);
    assert!(defs.is_empty(), "expected defs.is_empty() to be true");
}

#[test]
fn server_tool_config_active_definitions_web_search() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: Some(5),
        code_execution: false,
    };
    let mut active = HashSet::new();
    active.insert(ToolName::new("web_search").expect("valid"));
    let defs = config.active_definitions(&active);
    assert_eq!(defs.len(), 1, "expected defs.len() to equal 1");
    assert_eq!(
        defs[0].tool_type, "web_search_20250305",
        "expected defs[0].tool_type to equal \"web_search_20250305\""
    );
    assert_eq!(
        defs[0].name, "web_search",
        "expected defs[0].name to equal \"web_search\""
    );
    assert_eq!(
        defs[0].max_uses,
        Some(5),
        "expected defs[0].max_uses to equal Some(5)"
    );
}

#[test]
fn server_tool_config_active_definitions_code_execution() {
    let config = ServerToolConfig {
        web_search: false,
        web_search_max_uses: None,
        code_execution: true,
    };
    let mut active = HashSet::new();
    active.insert(ToolName::new("code_execution").expect("valid"));
    let defs = config.active_definitions(&active);
    assert_eq!(defs.len(), 1, "expected defs.len() to equal 1");
    assert_eq!(
        defs[0].tool_type, "code_execution_20250522",
        "expected defs[0].tool_type to equal \"code_execution_20250522\""
    );
    assert_eq!(
        defs[0].name, "code_execution",
        "expected defs[0].name to equal \"code_execution\""
    );
}

#[test]
fn server_tool_config_active_ignores_disabled_tools() {
    let config = ServerToolConfig {
        web_search: false,
        web_search_max_uses: None,
        code_execution: false,
    };
    let mut active = HashSet::new();
    active.insert(ToolName::new("web_search").expect("valid"));
    active.insert(ToolName::new("code_execution").expect("valid"));
    let defs = config.active_definitions(&active);
    assert!(defs.is_empty(), "expected defs.is_empty() to be true");
}

#[test]
fn server_tool_config_active_definitions_both() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: None,
        code_execution: true,
    };
    let mut active = HashSet::new();
    active.insert(ToolName::new("web_search").expect("valid"));
    active.insert(ToolName::new("code_execution").expect("valid"));
    let defs = config.active_definitions(&active);
    assert_eq!(defs.len(), 2, "expected defs.len() to equal 2");
}

#[test]
fn server_tool_config_deserializes_from_partial_json() {
    let json = r#"{"web_search": true}"#;
    let config: ServerToolConfig = serde_json::from_str(json).expect("deserialize");
    assert!(config.web_search, "expected config.web_search to be true");
    assert!(
        !config.code_execution,
        "expected config.code_execution to be false"
    );
    assert!(
        config.web_search_max_uses.is_none(),
        "expected config.web_search_max_uses.is_none() to be true"
    );
}

// ── Reversibility tests ──────────────────────────────────────────────

#[test]
fn reversibility_display_all_variants() {
    assert_eq!(
        Reversibility::FullyReversible.to_string(),
        "fully_reversible",
        "FullyReversible display"
    );
    assert_eq!(
        Reversibility::Reversible.to_string(),
        "reversible",
        "Reversible display"
    );
    assert_eq!(
        Reversibility::PartiallyReversible.to_string(),
        "partially_reversible",
        "PartiallyReversible display"
    );
    assert_eq!(
        Reversibility::Irreversible.to_string(),
        "irreversible",
        "Irreversible display"
    );
}

#[test]
fn reversibility_default_is_irreversible() {
    assert_eq!(
        Reversibility::default(),
        Reversibility::Irreversible,
        "default reversibility should be Irreversible"
    );
}

#[test]
fn reversibility_supports_dry_run() {
    assert!(
        Reversibility::FullyReversible.supports_dry_run(),
        "FullyReversible should support dry run"
    );
    assert!(
        Reversibility::Reversible.supports_dry_run(),
        "Reversible should support dry run"
    );
    assert!(
        !Reversibility::PartiallyReversible.supports_dry_run(),
        "PartiallyReversible should not support dry run"
    );
    assert!(
        !Reversibility::Irreversible.supports_dry_run(),
        "Irreversible should not support dry run"
    );
}

#[test]
fn reversibility_serde_roundtrip() {
    for rev in [
        Reversibility::FullyReversible,
        Reversibility::Reversible,
        Reversibility::PartiallyReversible,
        Reversibility::Irreversible,
    ] {
        let json = serde_json::to_string(&rev).expect("serialize");
        let back: Reversibility = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rev, back, "roundtrip for {rev}");
    }
}

#[test]
fn approval_requirement_from_reversibility() {
    assert_eq!(
        ApprovalRequirement::from(Reversibility::FullyReversible),
        ApprovalRequirement::None,
        "FullyReversible -> None"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::Reversible),
        ApprovalRequirement::Advisory,
        "Reversible -> Advisory"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::PartiallyReversible),
        ApprovalRequirement::Required,
        "PartiallyReversible -> Required"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::Irreversible),
        ApprovalRequirement::Mandatory,
        "Irreversible -> Mandatory"
    );
}

#[test]
fn approval_requirement_display() {
    assert_eq!(
        ApprovalRequirement::None.to_string(),
        "none",
        "None display"
    );
    assert_eq!(
        ApprovalRequirement::Advisory.to_string(),
        "advisory",
        "Advisory display"
    );
    assert_eq!(
        ApprovalRequirement::Required.to_string(),
        "required",
        "Required display"
    );
    assert_eq!(
        ApprovalRequirement::Mandatory.to_string(),
        "mandatory",
        "Mandatory display"
    );
}

#[test]
fn tool_call_metadata_serde_roundtrip() {
    let meta = ToolCallMetadata {
        reversibility: Reversibility::PartiallyReversible,
        approval: ApprovalRequirement::Required,
        dry_run: true,
    };
    let json = serde_json::to_string(&meta).expect("serialize");
    let back: ToolCallMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.reversibility,
        Reversibility::PartiallyReversible,
        "reversibility roundtrip"
    );
    assert_eq!(
        back.approval,
        ApprovalRequirement::Required,
        "approval roundtrip"
    );
    assert!(back.dry_run, "dry_run roundtrip");
}

#[test]
fn tool_def_includes_reversibility_in_serde() {
    let def = ToolDef {
        name: ToolName::new("test").expect("valid"),
        description: "test".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
    };
    let json = serde_json::to_string(&def).expect("serialize");
    let back: ToolDef = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.reversibility,
        Reversibility::Reversible,
        "reversibility should survive serde roundtrip"
    );
}

// ── ToolDiagnostics tests ────────────────────────────────────────────

#[test]
fn tool_diagnostics_to_llm_text_includes_all_fields() {
    let diag = ToolDiagnostics {
        exit_code: Some(127),
        stderr: Some("command not found".to_owned()),
        sandbox_violations: vec!["read /etc/shadow".to_owned()],
        duration_ms: 42,
    };
    let text = diag.to_llm_text();
    assert!(
        text.contains("exit_code=127"),
        "should include exit_code: {text}"
    );
    assert!(
        text.contains("stderr=command not found"),
        "should include stderr: {text}"
    );
    assert!(
        text.contains("sandbox_violations=read /etc/shadow"),
        "should include sandbox violations: {text}"
    );
    assert!(
        text.contains("duration_ms=42"),
        "should include duration: {text}"
    );
}

#[test]
fn tool_diagnostics_to_llm_text_omits_empty_fields() {
    let diag = ToolDiagnostics {
        exit_code: None,
        stderr: None,
        sandbox_violations: Vec::new(),
        duration_ms: 100,
    };
    let text = diag.to_llm_text();
    assert!(
        !text.contains("exit_code"),
        "should omit exit_code when None: {text}"
    );
    assert!(
        !text.contains("stderr"),
        "should omit stderr when None: {text}"
    );
    assert!(
        !text.contains("sandbox_violations"),
        "should omit sandbox_violations when empty: {text}"
    );
    assert!(
        text.contains("duration_ms=100"),
        "should always include duration: {text}"
    );
}

#[test]
fn tool_diagnostics_bounds_long_stderr() {
    let long_stderr = "e".repeat(600);
    let diag = ToolDiagnostics {
        exit_code: Some(1),
        stderr: Some(long_stderr.clone()),
        sandbox_violations: Vec::new(),
        duration_ms: 0,
    };
    let text = diag.to_llm_text();
    assert!(
        text.contains("stderr="),
        "should include stderr prefix: {text}"
    );
    assert!(
        text.contains("…[truncated]"),
        "should truncate long stderr: {text}"
    );
    assert!(
        text.len() < long_stderr.len() + 100,
        "diagnostic text should be bounded: {text}"
    );
}

#[test]
fn tool_result_with_diagnostics_builder() {
    let result = ToolResult::text("hello").with_diagnostics(ToolDiagnostics {
        exit_code: Some(0),
        stderr: None,
        sandbox_violations: Vec::new(),
        duration_ms: 5,
    });
    assert!(!result.is_error, "should not be error");
    assert_eq!(
        result
            .diagnostics
            .as_ref()
            .expect("diagnostics present")
            .exit_code,
        Some(0),
        "should carry diagnostics"
    );
}

#[test]
fn tool_result_serde_roundtrip_preserves_diagnostics() {
    let result = ToolResult::error("fail").with_diagnostics(ToolDiagnostics {
        exit_code: Some(1),
        stderr: Some("stderr output".to_owned()),
        sandbox_violations: vec!["violation".to_owned()],
        duration_ms: 10,
    });
    let json = serde_json::to_string(&result).expect("serialize");
    let back: ToolResult = serde_json::from_str(&json).expect("deserialize");
    assert!(back.is_error, "should preserve is_error");
    let diag = back.diagnostics.expect("should preserve diagnostics");
    assert_eq!(diag.exit_code, Some(1));
    assert_eq!(diag.stderr.as_deref(), Some("stderr output"));
    assert_eq!(diag.sandbox_violations, vec!["violation".to_owned()]);
    assert_eq!(diag.duration_ms, 10);
}

#[test]
fn tool_result_serde_backward_compat_missing_diagnostics() {
    // WHY: ToolResultContent is an untagged enum; Text(String) serializes as
    // a plain JSON string, not an object.
    let json = r#"{"content":"legacy","is_error":false}"#;
    let back: ToolResult = serde_json::from_str(json).expect("deserialize legacy");
    assert_eq!(back.content.text_summary(), "legacy");
    assert!(!back.is_error);
    assert!(
        back.diagnostics.is_none(),
        "missing diagnostics should default to None"
    );
}

// ---------------------------------------------------------------------------
// ToolOutcome / partial success (#3633)
// ---------------------------------------------------------------------------

#[test]
fn tool_result_partial_success_constructor_carries_reasons() {
    // Scenario: 3-way sub-agent dispatch where 2 succeeded and 1 failed.
    // Old is_error bool would collapse this into a binary; new outcome
    // surfaces it as PartialSuccess with one reason per failed sub-op.
    let r = ToolResult::partial_success(
        r#"[{"ok":1},{"ok":2},{"err":"boom"}]"#,
        vec!["sub-agent #3: boom".to_owned()],
    );
    assert!(
        !r.is_error,
        "partial_success should map to is_error=false — tool delivered usable output"
    );
    assert!(r.outcome.is_partial(), "outcome should be PartialSuccess");
    assert_eq!(
        r.outcome.partial_reasons(),
        &["sub-agent #3: boom".to_owned()],
        "reasons should propagate through the outcome, not be collapsed"
    );

    // Old consumers still reading is_error get the back-compat view.
    match &r.outcome {
        ToolOutcome::PartialSuccess(info) => {
            assert_eq!(info.reasons.len(), 1, "one reason per failed sub-operation");
        }
        other => panic!("expected PartialSuccess, got {other:?}"),
    }
}

#[test]
fn tool_outcome_is_error_collapse_matches_legacy_bool() {
    assert!(!ToolOutcome::Success.is_error());
    assert!(!ToolOutcome::partial(Vec::<String>::new()).is_error());
    assert!(ToolOutcome::failure("x").is_error());
}

#[test]
fn tool_result_outcome_serde_roundtrip_partial() {
    let r = ToolResult::partial_success("payload", vec!["warn".to_owned()]);
    let json = serde_json::to_string(&r).expect("serialize");
    let back: ToolResult = serde_json::from_str(&json).expect("deserialize");
    assert!(back.outcome.is_partial(), "outcome should survive serde");
    assert_eq!(back.outcome.partial_reasons(), &["warn".to_owned()]);
    assert!(!back.is_error);
}

#[test]
fn tool_result_serde_legacy_payload_defaults_to_success_then_normalizes() {
    // Legacy payloads predate the `outcome` field; #[serde(default)]
    // populates it as Success. For legacy error records, normalize()
    // promotes the outcome to Failure to match is_error=true.
    let success_legacy = r#"{"content":"ok","is_error":false}"#;
    let back: ToolResult = serde_json::from_str(success_legacy).expect("deserialize success");
    assert!(
        back.outcome.is_success(),
        "legacy success defaults correctly"
    );

    let error_legacy = r#"{"content":"bad","is_error":true}"#;
    let back: ToolResult = serde_json::from_str::<ToolResult>(error_legacy)
        .expect("deserialize error")
        .normalize();
    assert!(
        back.outcome.is_error(),
        "normalize() promotes legacy is_error=true records to Failure"
    );
    assert!(back.is_error);
}

#[test]
fn tool_stats_record_outcome_separates_partial_from_error() {
    let mut stats = ToolStats::default();
    stats.record_outcome("dispatch", 10, &ToolOutcome::Success);
    stats.record_outcome(
        "dispatch",
        20,
        &ToolOutcome::partial(vec!["one failed".to_owned()]),
    );
    stats.record_outcome("dispatch", 30, &ToolOutcome::failure("crash"));
    assert_eq!(stats.total_calls, 3);
    assert_eq!(
        stats.error_count, 1,
        "only true failures count as errors, not partials"
    );
    assert_eq!(
        stats.partial_count, 1,
        "partial successes get their own counter (#3633)"
    );
    assert_eq!(stats.total_duration_ms, 60);
}
