//! Registry-wide contract tests for real Organon builtins.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "contract tests should fail with direct assertion context"
)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use indexmap::IndexMap;
use koina::id::{NousId, SessionId, ToolName};
use serde_json::{Map, Value};

use organon::builtins;
use organon::registry::ToolRegistry;
use organon::sandbox::SandboxConfig;
use organon::types::{
    AdditionalProperties, ApprovalRequirement, InputSchema, PropertyDef, PropertyType,
    Reversibility, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolTag,
};

fn builtin_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    builtins::register_all_with_sandbox(&mut registry, SandboxConfig::default())
        .expect("builtins should register without collision");
    registry
}

fn validation_context() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("alice").expect("valid nous id"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: PathBuf::from("/tmp"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

#[test]
fn builtin_schemas_are_structurally_valid() {
    let registry = builtin_registry();
    let defs = registry.definitions();
    assert!(
        defs.len() > 20,
        "expected registry-wide coverage, got {} builtin(s)",
        defs.len()
    );

    for def in defs {
        assert!(
            !def.description.trim().is_empty(),
            "{} must have a description",
            def.name
        );

        let json_schema = def.input_schema.to_json_schema();
        assert_eq!(
            json_schema.get("type").and_then(Value::as_str),
            Some("object"),
            "{} schema must serialize as an object",
            def.name
        );
        assert!(
            json_schema.get("properties").is_some_and(Value::is_object),
            "{} schema must serialize properties",
            def.name
        );

        assert_input_schema(def.name.as_str(), &def.input_schema);
    }
}

#[tokio::test]
async fn builtin_required_fields_are_rejected_before_execution() {
    let registry = builtin_registry();
    let ctx = validation_context();

    for def in registry.definitions() {
        for missing in &def.input_schema.required {
            let arguments = required_arguments_except(&def.input_schema, missing);
            let input = ToolInput {
                name: def.name.clone(),
                tool_use_id: format!("contract_{}_{}", def.name, missing),
                arguments,
            };

            let err = registry
                .execute(&input, &ctx)
                .await
                .expect_err("missing required field must fail before executor");
            let message = err.to_string();
            assert!(
                message.contains(missing) && message.contains("missing"),
                "{} missing required field {missing:?} should be named, got: {message}",
                def.name
            );
        }
    }
}

#[test]
fn builtin_metadata_is_complete_and_approval_consistent() {
    let registry = builtin_registry();

    for def in registry.definitions() {
        assert!(
            !def.groups.is_empty(),
            "{} must declare at least one tool group",
            def.name
        );
        assert!(
            !def.tags.is_empty(),
            "{} must declare at least one operational tag",
            def.name
        );
        assert_no_duplicate_groups(def);
        assert_no_duplicate_tags(def);
        assert_category_group_match(def);

        assert_eq!(
            registry.approval_requirement(&def.name),
            Some(ApprovalRequirement::from(def.reversibility)),
            "{} approval must derive from reversibility",
            def.name
        );
        let metadata = registry
            .call_metadata(&def.name, false)
            .expect("registered tool should have call metadata");
        assert_eq!(
            metadata.reversibility, def.reversibility,
            "{} metadata reversibility drifted from definition",
            def.name
        );
        assert_eq!(
            metadata.approval,
            ApprovalRequirement::from(def.reversibility),
            "{} metadata approval drifted from definition",
            def.name
        );

        for tag in &def.tags {
            let tagged = registry.definitions_for_tags(&[*tag]);
            assert!(
                tagged.iter().any(|candidate| candidate.name == def.name),
                "{} must be discoverable by tag {tag}",
                def.name
            );
        }
    }
}

#[test]
fn lazy_activation_surface_matches_builtin_metadata() {
    let registry = builtin_registry();
    let inactive_surface = registry.to_hermeneus_tools_filtered(&HashSet::new());
    let inactive_names: HashSet<&str> = inactive_surface
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();

    assert!(
        inactive_names.contains("enable_tool"),
        "enable_tool must remain available to activate lazy tools"
    );

    let lazy_catalog_names: HashSet<String> = registry
        .lazy_tool_catalog()
        .into_iter()
        .map(|(name, _)| name.as_str().to_owned())
        .collect();

    for def in registry.definitions() {
        if def.auto_activate || def.name.as_str() == "enable_tool" {
            assert!(
                inactive_names.contains(def.name.as_str()),
                "{} must be present in inactive surface",
                def.name
            );
            assert!(
                !lazy_catalog_names.contains(def.name.as_str()),
                "{} must not appear in lazy catalog",
                def.name
            );
        } else {
            assert!(
                !inactive_names.contains(def.name.as_str()),
                "{} must stay hidden until activated",
                def.name
            );
            assert!(
                lazy_catalog_names.contains(def.name.as_str()),
                "{} must appear in lazy catalog",
                def.name
            );
        }
    }

    let activated = HashSet::from([ToolName::from_static("web_search")]);
    let active_names: HashSet<String> = registry
        .to_hermeneus_tools_filtered(&activated)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(
        active_names.contains("web_search"),
        "activated lazy tool must enter the callable surface"
    );
}

#[test]
fn subprocess_backed_builtins_have_explicit_safety_metadata() {
    let registry = builtin_registry();

    let expected = vec![
        ToolFixture::new(
            "grep",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "find",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "ls",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "exec",
            ToolCategory::Workspace,
            Reversibility::Irreversible,
            true,
            &[ToolGroupId::Command],
            &[ToolTag::Execute],
        ),
        ToolFixture::new(
            "git_status",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "git_log",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "git_diff",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "git_branch",
            ToolCategory::Workspace,
            Reversibility::FullyReversible,
            true,
            &[ToolGroupId::Read],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "git_checkout",
            ToolCategory::Workspace,
            Reversibility::Reversible,
            true,
            &[ToolGroupId::Command],
            &[ToolTag::Edit],
        ),
        #[cfg(feature = "computer-use")]
        ToolFixture::new(
            "computer_use",
            ToolCategory::System,
            Reversibility::Irreversible,
            false,
            &[ToolGroupId::Command, ToolGroupId::Edit],
            &[ToolTag::Execute],
        ),
        #[cfg(feature = "bookkeeper")]
        ToolFixture::new(
            "katharos",
            ToolCategory::System,
            Reversibility::Irreversible,
            false,
            &[ToolGroupId::Read, ToolGroupId::Edit, ToolGroupId::Command],
            &[ToolTag::Edit, ToolTag::Execute],
        ),
    ];

    for fixture in expected {
        fixture.assert_matches(&registry);
    }
}

#[test]
fn external_service_builtins_are_lazy_and_not_daemon_safe() {
    let registry = builtin_registry();

    for fixture in [
        ToolFixture::new(
            "web_fetch",
            ToolCategory::Research,
            Reversibility::Irreversible,
            false,
            &[ToolGroupId::Read, ToolGroupId::Mcp],
            &[ToolTag::Fetch],
        ),
        ToolFixture::new(
            "http_request",
            ToolCategory::Research,
            Reversibility::Irreversible,
            false,
            &[ToolGroupId::Mcp],
            &[ToolTag::Fetch],
        ),
        ToolFixture::new(
            "web_search",
            ToolCategory::Research,
            Reversibility::Reversible,
            false,
            &[ToolGroupId::Read, ToolGroupId::Mcp],
            &[ToolTag::Fetch, ToolTag::Recon],
        ),
        ToolFixture::new(
            "issue_scan",
            ToolCategory::Planning,
            Reversibility::Reversible,
            false,
            &[ToolGroupId::Read, ToolGroupId::Plan, ToolGroupId::Mcp],
            &[ToolTag::Recon],
        ),
        ToolFixture::new(
            "issue_triage",
            ToolCategory::Planning,
            Reversibility::PartiallyReversible,
            false,
            &[ToolGroupId::Edit, ToolGroupId::Mcp],
            &[ToolTag::Plan],
        ),
    ] {
        fixture.assert_matches(&registry);
        let def = registry
            .get_def(&ToolName::from_static(fixture.name))
            .expect("fixture tool should be registered");
        assert!(
            def.groups.contains(&ToolGroupId::Mcp),
            "{} must be marked as an external API/MCP surface",
            def.name
        );
        assert!(
            def.reversibility != Reversibility::FullyReversible,
            "{} must not be daemon-safe/read-only classified",
            def.name
        );
        assert!(
            !def.auto_activate,
            "{} external service tool must require explicit activation",
            def.name
        );
    }
}

fn assert_input_schema(tool_name: &str, schema: &InputSchema) {
    assert_required_declared(tool_name, "$", &schema.properties, &schema.required);
    for (property_name, property) in &schema.properties {
        assert_property_schema(tool_name, property_name, property);
    }
}

fn assert_property_schema(tool_name: &str, path: &str, property: &PropertyDef) {
    assert!(
        !property.description.trim().is_empty(),
        "{tool_name}.{path} must describe the property"
    );
    if let Some(enum_values) = &property.enum_values {
        assert!(
            !enum_values.is_empty(),
            "{tool_name}.{path} enum must not be empty"
        );
        for value in enum_values {
            assert!(
                !value.trim().is_empty(),
                "{tool_name}.{path} enum value must not be empty"
            );
        }
    }
    assert_min_max(
        tool_name,
        path,
        "numeric",
        property.minimum,
        property.maximum,
    );
    assert_min_max_usize(
        tool_name,
        path,
        "string length",
        property.min_length,
        property.max_length,
    );
    assert_min_max_usize(
        tool_name,
        path,
        "array length",
        property.min_items,
        property.max_items,
    );

    if property.property_type == PropertyType::Array {
        assert!(
            property.items.is_some(),
            "{tool_name}.{path} array schema must declare items"
        );
    }
    if let Some(items) = &property.items {
        assert_property_schema(tool_name, &format!("{path}[]"), items);
    }

    if let Some(properties) = &property.properties {
        let required = property.required.as_deref().unwrap_or(&[]);
        assert_required_declared(tool_name, path, properties, required);
        for (name, nested) in properties {
            assert_property_schema(tool_name, &format!("{path}.{name}"), nested);
        }
    } else {
        assert!(
            property.required.as_ref().is_none_or(Vec::is_empty),
            "{tool_name}.{path} declares required properties without object properties"
        );
    }

    if let Some(AdditionalProperties::Schema(schema)) = &property.additional_properties {
        assert_property_schema(tool_name, &format!("{path}.*"), schema);
    }
}

fn assert_required_declared(
    tool_name: &str,
    path: &str,
    properties: &IndexMap<String, PropertyDef>,
    required: &[String],
) {
    for required_name in required {
        assert!(
            properties.contains_key(required_name),
            "{tool_name}.{path} requires undeclared property {required_name:?}"
        );
    }
}

fn assert_min_max(tool_name: &str, path: &str, label: &str, min: Option<f64>, max: Option<f64>) {
    if let (Some(min), Some(max)) = (min, max) {
        assert!(
            min <= max,
            "{tool_name}.{path} {label} minimum {min} exceeds maximum {max}"
        );
    }
}

fn assert_min_max_usize(
    tool_name: &str,
    path: &str,
    label: &str,
    min: Option<usize>,
    max: Option<usize>,
) {
    if let (Some(min), Some(max)) = (min, max) {
        assert!(
            min <= max,
            "{tool_name}.{path} {label} minimum {min} exceeds maximum {max}"
        );
    }
}

fn required_arguments_except(schema: &InputSchema, missing: &str) -> Value {
    let mut args = Map::new();
    for required in &schema.required {
        if required == missing {
            continue;
        }
        let property = schema
            .properties
            .get(required)
            .expect("required property should be declared");
        args.insert(required.clone(), sample_value(property));
    }
    Value::Object(args)
}

fn sample_value(property: &PropertyDef) -> Value {
    match property.property_type {
        PropertyType::String => property.enum_values.as_ref().map_or_else(
            || Value::String("value".to_owned()),
            |values| Value::String(values[0].clone()),
        ),
        PropertyType::Number => Value::from(property.minimum.unwrap_or(1.0)),
        PropertyType::Integer => Value::from(sample_integer(property)),
        PropertyType::Boolean => Value::Bool(true),
        PropertyType::Array => {
            let count = property.min_items.unwrap_or(1);
            let item = property.items.as_deref().map_or(Value::Null, sample_value);
            Value::Array(vec![item; count])
        }
        PropertyType::Object => {
            let mut object = Map::new();
            if let Some(properties) = &property.properties {
                for required in property.required.as_deref().unwrap_or(&[]) {
                    let nested = properties
                        .get(required)
                        .expect("required nested property should be declared");
                    object.insert(required.clone(), sample_value(nested));
                }
            }
            Value::Object(object)
        }
        _ => Value::Null,
    }
}

fn sample_integer(_property: &PropertyDef) -> i64 {
    1
}

fn assert_no_duplicate_groups(def: &ToolDef) {
    let mut seen = HashSet::new();
    for group in &def.groups {
        assert!(seen.insert(*group), "{} repeats group {group}", def.name);
    }
}

fn assert_no_duplicate_tags(def: &ToolDef) {
    let mut seen = HashSet::new();
    for tag in &def.tags {
        assert!(seen.insert(*tag), "{} repeats tag {tag}", def.name);
    }
}

fn assert_category_group_match(def: &ToolDef) {
    let valid = match def.category {
        ToolCategory::Workspace => has_any_group(
            def,
            &[
                ToolGroupId::Read,
                ToolGroupId::Edit,
                ToolGroupId::Command,
                ToolGroupId::Verify,
            ],
        ),
        ToolCategory::Memory => has_any_group(
            def,
            &[ToolGroupId::Read, ToolGroupId::Edit, ToolGroupId::Verify],
        ),
        ToolCategory::Communication => has_any_group(def, &[ToolGroupId::Mcp]),
        ToolCategory::Planning => has_any_group(
            def,
            &[
                ToolGroupId::Plan,
                ToolGroupId::Read,
                ToolGroupId::Edit,
                ToolGroupId::Verify,
                ToolGroupId::Mcp,
            ],
        ),
        ToolCategory::System => has_any_group(
            def,
            &[
                ToolGroupId::Read,
                ToolGroupId::Command,
                ToolGroupId::Edit,
                ToolGroupId::Verify,
            ],
        ),
        ToolCategory::Agent => has_any_group(
            def,
            &[
                ToolGroupId::SpawnSubtask,
                ToolGroupId::Verify,
                ToolGroupId::Plan,
                ToolGroupId::Mcp,
            ],
        ),
        ToolCategory::Research => has_any_group(
            def,
            &[
                ToolGroupId::Read,
                ToolGroupId::Mcp,
                ToolGroupId::Verify,
                ToolGroupId::Edit,
            ],
        ),
        _ => !def.groups.is_empty(),
    };
    assert!(
        valid,
        "{} category {:?} does not match groups {:?}",
        def.name, def.category, def.groups
    );
}

fn has_any_group(def: &ToolDef, groups: &[ToolGroupId]) -> bool {
    def.groups.iter().any(|group| groups.contains(group))
}

struct ToolFixture {
    name: &'static str,
    category: ToolCategory,
    reversibility: Reversibility,
    auto_activate: bool,
    groups: &'static [ToolGroupId],
    tags: &'static [ToolTag],
}

impl ToolFixture {
    const fn new(
        name: &'static str,
        category: ToolCategory,
        reversibility: Reversibility,
        auto_activate: bool,
        groups: &'static [ToolGroupId],
        tags: &'static [ToolTag],
    ) -> Self {
        Self {
            name,
            category,
            reversibility,
            auto_activate,
            groups,
            tags,
        }
    }

    fn assert_matches(&self, registry: &ToolRegistry) {
        let def = registry
            .get_def(&ToolName::from_static(self.name))
            .expect("fixture tool should be registered");
        assert_eq!(
            def.category, self.category,
            "{} category drifted",
            self.name
        );
        assert_eq!(
            def.reversibility, self.reversibility,
            "{} reversibility drifted",
            self.name
        );
        assert_eq!(
            def.auto_activate, self.auto_activate,
            "{} activation drifted",
            self.name
        );
        assert_eq!(def.groups, self.groups, "{} groups drifted", self.name);
        assert_eq!(def.tags, self.tags, "{} tags drifted", self.name);
    }
}
