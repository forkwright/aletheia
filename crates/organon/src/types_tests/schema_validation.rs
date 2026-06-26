//! Schema validation tests for rich PropertyDef and InputSchema.

#![expect(clippy::expect_used, reason = "test assertions")]

use indexmap::IndexMap;

use super::super::*;

fn make_registry_with_builtins() -> crate::registry::ToolRegistry {
    let mut registry = crate::registry::ToolRegistry::new();
    crate::builtins::register_domain_tools(
        &mut registry,
        crate::sandbox::SandboxConfig::default(),
        #[cfg(feature = "energeia")]
        None,
    )
    .expect("register builtins");
    registry
}

#[test]
fn validates_required_field_missing() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "path".to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: "File path".to_owned(),
                ..Default::default()
            },
        )]),
        required: vec!["path".to_owned()],
    };
    let name = ToolName::new("test").expect("valid");
    let err = schema
        .validate(&name, &serde_json::json!({}))
        .expect_err("missing required field");
    assert!(
        err.to_string().contains("missing required argument: path"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_type_mismatch() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "count".to_owned(),
            PropertyDef {
                property_type: PropertyType::Integer,
                description: "Count".to_owned(),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    let err = schema
        .validate(&name, &serde_json::json!({"count": "five"}))
        .expect_err("type mismatch");
    assert!(
        err.to_string().contains("expected integer, got string"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_enum_value() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "color".to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: "Color".to_owned(),
                enum_values: Some(vec!["red".to_owned(), "blue".to_owned()]),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(&name, &serde_json::json!({"color": "red"}))
        .expect("valid enum value");
    let err = schema
        .validate(&name, &serde_json::json!({"color": "green"}))
        .expect_err("invalid enum value");
    assert!(
        err.to_string().contains("value \"green\" not in enum"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_numeric_bounds() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "port".to_owned(),
            PropertyDef {
                property_type: PropertyType::Integer,
                description: "Port".to_owned(),
                minimum: Some(1.0),
                maximum: Some(65535.0),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(&name, &serde_json::json!({"port": 443}))
        .expect("in-range port");
    let err = schema
        .validate(&name, &serde_json::json!({"port": 0}))
        .expect_err("port below minimum");
    assert!(
        err.to_string().contains("below minimum 1"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_string_length() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "label".to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: "Label".to_owned(),
                min_length: Some(2),
                max_length: Some(8),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(&name, &serde_json::json!({"label": "ok"}))
        .expect("valid length");
    let err = schema
        .validate(&name, &serde_json::json!({"label": "x"}))
        .expect_err("too short");
    assert!(
        err.to_string().contains("below minimum 2"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_array_items() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "tags".to_owned(),
            PropertyDef {
                property_type: PropertyType::Array,
                description: "Tags".to_owned(),
                items: Some(Box::new(PropertyDef {
                    property_type: PropertyType::String,
                    description: "Tag".to_owned(),
                    ..Default::default()
                })),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(&name, &serde_json::json!({"tags": ["a", "b"]}))
        .expect("valid array items");
    let err = schema
        .validate(&name, &serde_json::json!({"tags": ["a", 2]}))
        .expect_err("invalid array item");
    assert!(
        err.to_string().contains("tags[1]: expected string, got number"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_array_length_bounds() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "items".to_owned(),
            PropertyDef {
                property_type: PropertyType::Array,
                description: "Items".to_owned(),
                min_items: Some(1),
                max_items: Some(3),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    let err = schema
        .validate(&name, &serde_json::json!({"items": []}))
        .expect_err("array too short");
    assert!(
        err.to_string().contains("below minimum 1"),
        "unexpected error: {err}"
    );
    let err = schema
        .validate(&name, &serde_json::json!({"items": [1, 2, 3, 4]}))
        .expect_err("array too long");
    assert!(
        err.to_string().contains("above maximum 3"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_object_properties_and_rejects_extra() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "config".to_owned(),
            PropertyDef {
                property_type: PropertyType::Object,
                description: "Config".to_owned(),
                properties: Some(IndexMap::from([(
                    "enabled".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Enabled".to_owned(),
                        ..Default::default()
                    },
                )])),
                required: Some(vec!["enabled".to_owned()]),
                additional_properties: Some(AdditionalProperties::Bool(false)),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(
            &name,
            &serde_json::json!({"config": {"enabled": true}}),
        )
        .expect("valid object");
    let err = schema
        .validate(
            &name,
            &serde_json::json!({"config": {"enabled": true, "extra": 1}}),
        )
        .expect_err("extra property");
    assert!(
        err.to_string().contains("extra property \"extra\" is not allowed"),
        "unexpected error: {err}"
    );
}

#[test]
fn validates_object_additional_properties_schema() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "headers".to_owned(),
            PropertyDef {
                property_type: PropertyType::Object,
                description: "Headers".to_owned(),
                additional_properties: Some(AdditionalProperties::Schema(Box::new(
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Header value".to_owned(),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let name = ToolName::new("test").expect("valid");
    schema
        .validate(
            &name,
            &serde_json::json!({"headers": {"Content-Type": "application/json"}}),
        )
        .expect("valid header value");
    let err = schema
        .validate(
            &name,
            &serde_json::json!({"headers": {"Content-Length": 42}}),
        )
        .expect_err("non-string header value");
    assert!(
        err.to_string().contains("headers.Content-Length: expected string, got number"),
        "unexpected error: {err}"
    );
}

#[test]
fn json_schema_emits_nested_properties() {
    let schema = InputSchema {
        properties: IndexMap::from([(
            "task".to_owned(),
            PropertyDef {
                property_type: PropertyType::Object,
                description: "Task".to_owned(),
                properties: Some(IndexMap::from([(
                    "role".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Role".to_owned(),
                        enum_values: Some(vec!["coder".to_owned()]),
                        ..Default::default()
                    },
                )])),
                required: Some(vec!["role".to_owned()]),
                additional_properties: Some(AdditionalProperties::Bool(false)),
                ..Default::default()
            },
        )]),
        required: vec![],
    };
    let json = schema.to_json_schema();
    assert_eq!(
        json["properties"]["task"]["properties"]["role"]["type"],
        "string"
    );
    assert_eq!(
        json["properties"]["task"]["required"][0],
        "role"
    );
    assert_eq!(
        json["properties"]["task"]["additionalProperties"],
        false
    );
}

#[test]
fn http_request_headers_schema_rejects_non_string_values() {
    let registry = make_registry_with_builtins();
    let def = registry
        .get_def(&ToolName::from_static("http_request"))
        .expect("http_request tool registered");
    let schema = &def.input_schema;
    let json_schema = schema.to_json_schema();
    assert_eq!(
        json_schema["properties"]["headers"]["additionalProperties"]["type"],
        "string"
    );
    let name = ToolName::from_static("http_request");

    schema
        .validate(
            &name,
            &serde_json::json!({
                "url": "https://example.com/api",
                "headers": {"Content-Type": "application/json"}
            }),
        )
        .expect("valid headers");

    let err = schema
        .validate(
            &name,
            &serde_json::json!({
                "url": "https://example.com/api",
                "headers": {"Content-Length": 42}
            }),
        )
        .expect_err("non-string header value");
    assert!(
        err.to_string().contains("headers.Content-Length: expected string, got number"),
        "unexpected error: {err}"
    );
}

#[test]
fn sessions_dispatch_tasks_schema_requires_role_and_task() {
    let registry = make_registry_with_builtins();
    let def = registry
        .get_def(&ToolName::from_static("sessions_dispatch"))
        .expect("sessions_dispatch tool registered");
    let schema = &def.input_schema;
    let json_schema = schema.to_json_schema();
    let items = &json_schema["properties"]["tasks"]["items"];
    assert_eq!(items["additionalProperties"], false);
    let required: Vec<String> = serde_json::from_value(items["required"].clone()).expect("array");
    assert!(required.contains(&"role".to_owned()));
    assert!(required.contains(&"task".to_owned()));
    let name = ToolName::from_static("sessions_dispatch");

    schema
        .validate(
            &name,
            &serde_json::json!({
                "tasks": [
                    {"role": "coder", "task": "write a test"},
                    {"role": "reviewer", "task": "review it", "model": "haiku"}
                ]
            }),
        )
        .expect("valid tasks");

    let err = schema
        .validate(
            &name,
            &serde_json::json!({"tasks": [{"task": "missing role"}]}),
        )
        .expect_err("missing role");
    assert!(
        err.to_string().contains("tasks[0]: missing required property \"role\""),
        "unexpected error: {err}"
    );

    let err = schema
        .validate(
            &name,
            &serde_json::json!({"tasks": [{"role": "coder", "task": "ok", "extra": 1}]}),
        )
        .expect_err("extra property");
    assert!(
        err.to_string().contains("tasks[0]: extra property \"extra\" is not allowed"),
        "unexpected error: {err}"
    );
}

#[test]
fn datalog_query_params_schema_accepts_arbitrary_params() {
    let registry = make_registry_with_builtins();
    let def = registry
        .get_def(&ToolName::from_static("datalog_query"))
        .expect("datalog_query tool registered");
    let schema = &def.input_schema;
    let json_schema = schema.to_json_schema();
    assert_eq!(json_schema["properties"]["params"]["additionalProperties"], true);
    let name = ToolName::from_static("datalog_query");

    schema
        .validate(
            &name,
            &serde_json::json!({
                "query": "[:find ?e :where [?e :foo/bar _]]",
                "params": {"nous_id": "syn", "limit": 10}
            }),
        )
        .expect("arbitrary params accepted");
}
