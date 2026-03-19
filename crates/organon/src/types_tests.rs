#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]
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
        auto_activate: false,
    };
    let json = serde_json::to_string(&def).expect("serialize");
    let back: ToolDef = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.name.as_str(), "test_tool");
    assert_eq!(back.category, ToolCategory::Workspace);
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
    assert_eq!(json_schema["type"], "object");
    assert_eq!(json_schema["properties"]["path"]["type"], "string");
    assert_eq!(json_schema["properties"]["max_lines"]["default"], 100);
    assert_eq!(json_schema["required"][0], "path");
}

#[test]
fn property_type_serde() {
    let json = serde_json::to_string(&PropertyType::Integer).expect("serialize");
    assert_eq!(json, "\"integer\"");
    let back: PropertyType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, PropertyType::Integer);
}

#[test]
fn tool_category_display() {
    assert_eq!(ToolCategory::Workspace.to_string(), "workspace");
    assert_eq!(ToolCategory::Communication.to_string(), "communication");
    assert_eq!(ToolCategory::Research.to_string(), "research");
}

#[test]
fn tool_stats_record_accumulates() {
    let mut stats = ToolStats::default();
    stats.record("read", 10, false);
    stats.record("write", 20, false);
    stats.record("read", 15, true);
    assert_eq!(stats.total_calls, 3);
    assert_eq!(stats.total_duration_ms, 45);
    assert_eq!(stats.error_count, 1);
    assert_eq!(stats.calls_by_tool["read"], 2);
    assert_eq!(stats.calls_by_tool["write"], 1);
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
    assert_eq!(top.len(), 2);
    assert_eq!(top[0], ("c", 3));
    assert_eq!(top[1], ("b", 2));
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
    assert_eq!(back.name.as_str(), "read");
    assert_eq!(back.tool_use_id, "toolu_123");
}

#[test]
fn tool_result_text_constructor() {
    let r = ToolResult::text("hello");
    assert_eq!(r.content.text_summary(), "hello");
    assert!(!r.is_error);
}

#[test]
fn tool_result_error_constructor() {
    let r = ToolResult::error("bad input");
    assert_eq!(r.content.text_summary(), "bad input");
    assert!(r.is_error);
}

#[test]
fn tool_result_blocks_constructor() {
    let r = ToolResult::blocks(vec![ToolResultBlock::Text {
        text: "desc".to_owned(),
    }]);
    assert_eq!(r.content.text_summary(), "desc");
    assert!(!r.is_error);
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
    assert_eq!(json["properties"]["type"]["enum"][0], "a");
    assert_eq!(json["properties"]["type"]["enum"][1], "b");
}

#[test]
fn test_input_schema_with_no_required_fields_has_empty_required_array() {
    let schema = InputSchema {
        properties: IndexMap::new(),
        required: vec![],
    };
    let json = schema.to_json_schema();
    let required = json["required"].as_array().expect("array");
    assert!(required.is_empty());
}

#[test]
fn test_property_type_display_all_variants() {
    assert_eq!(PropertyType::String.to_string(), "string");
    assert_eq!(PropertyType::Number.to_string(), "number");
    assert_eq!(PropertyType::Integer.to_string(), "integer");
    assert_eq!(PropertyType::Boolean.to_string(), "boolean");
    assert_eq!(PropertyType::Array.to_string(), "array");
    assert_eq!(PropertyType::Object.to_string(), "object");
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
        assert_eq!(cat.to_string(), expected);
    }
}

#[test]
fn test_tool_stats_initial_state_all_zeros() {
    let stats = ToolStats::default();
    assert_eq!(stats.total_calls, 0);
    assert_eq!(stats.total_duration_ms, 0);
    assert_eq!(stats.error_count, 0);
    assert!(stats.calls_by_tool.is_empty());
}

#[test]
fn test_tool_stats_zero_errors_when_all_calls_succeed() {
    let mut stats = ToolStats::default();
    stats.record("read", 10, false);
    stats.record("write", 20, false);
    assert_eq!(stats.error_count, 0);
}

#[test]
fn test_tool_stats_top_tools_returns_empty_when_no_calls() {
    let stats = ToolStats::default();
    assert!(stats.top_tools(5).is_empty());
}

#[test]
fn test_tool_stats_top_tools_limited_to_n() {
    let mut stats = ToolStats::default();
    for name in ["a", "b", "c", "d", "e"] {
        stats.record(name, 1, false);
    }
    let top = stats.top_tools(3);
    assert_eq!(top.len(), 3);
}

#[test]
fn test_tool_result_text_is_not_error() {
    let r = ToolResult::text("ok");
    assert!(!r.is_error);
}

#[test]
fn test_tool_result_error_is_error() {
    let r = ToolResult::error("bad");
    assert!(r.is_error);
}

#[test]
fn test_tool_result_blocks_is_not_error() {
    let r = ToolResult::blocks(vec![]);
    assert!(!r.is_error);
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
        auto_activate: true,
    };
    assert!(def.auto_activate);
}

#[test]
fn test_input_schema_type_is_object_in_json_schema() {
    let schema = InputSchema {
        properties: IndexMap::new(),
        required: vec![],
    };
    let json = schema.to_json_schema();
    assert_eq!(json["type"], "object");
}

#[test]
fn server_tool_config_default_disables_all() {
    let config = ServerToolConfig::default();
    assert!(!config.web_search);
    assert!(!config.code_execution);
    assert!(config.web_search_max_uses.is_none());
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
    assert!(back.web_search);
    assert_eq!(back.web_search_max_uses, Some(5));
    assert!(back.code_execution);
}

#[test]
fn server_tool_config_catalog_entries_empty_when_disabled() {
    let config = ServerToolConfig::default();
    assert!(config.catalog_entries().is_empty());
}

#[test]
fn server_tool_config_catalog_entries_web_search() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: None,
        code_execution: false,
    };
    let entries = config.catalog_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0.as_str(), "web_search");
}

#[test]
fn server_tool_config_catalog_entries_both() {
    let config = ServerToolConfig {
        web_search: true,
        web_search_max_uses: Some(3),
        code_execution: true,
    };
    let entries = config.catalog_entries();
    assert_eq!(entries.len(), 2);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"code_execution"));
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
    assert!(defs.is_empty());
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
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].tool_type, "web_search_20250305");
    assert_eq!(defs[0].name, "web_search");
    assert_eq!(defs[0].max_uses, Some(5));
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
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].tool_type, "code_execution_20250522");
    assert_eq!(defs[0].name, "code_execution");
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
    assert!(defs.is_empty());
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
    assert_eq!(defs.len(), 2);
}

#[test]
fn server_tool_config_deserializes_from_partial_json() {
    let json = r#"{"web_search": true}"#;
    let config: ServerToolConfig = serde_json::from_str(json).expect("deserialize");
    assert!(config.web_search);
    assert!(!config.code_execution);
    assert!(config.web_search_max_uses.is_none());
}
