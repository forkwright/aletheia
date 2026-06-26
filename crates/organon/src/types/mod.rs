//! Core types for tool definitions, input/output, and execution context.

mod context;
mod policy;
mod services;

pub use context::{ServerToolConfig, ToolContext, ToolHttpClients, ToolServices};
pub use policy::{
    ToolArgumentValueCapability, ToolCallCapability, ToolCallCapabilityRule, ToolGroupId,
    ToolGroupPolicy,
};
pub use services::{
    BlackboardEntry, BlackboardStore, CrossNousService, DatalogResult, FactSummary,
    KnowledgeSearchService, MemoryResult, MessageService, NoteEntry, NoteStore, PlanningPlanInput,
    PlanningService, SpawnContext, SpawnRequest, SpawnResult, SpawnService, WorkingCheckpoint,
    WorkingCheckpointStore,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use hermeneus::types::{DocumentSource, ImageSource, ToolResultBlock, ToolResultContent};
use koina::id::ToolName;

/// Operational-intent tag for cross-category tool lookup.
///
/// Categories ([`ToolCategory`]) are structural / navigational; tags describe
/// what the tool *does* at an operational level.  A single tool may carry
/// multiple tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolTag {
    /// Intel-gathering, discovery, read-only inspection.
    Recon,
    /// File or state mutation.
    Edit,
    /// Tests, lints, checks, validation.
    Verify,
    /// External data retrieval (HTTP, MCP, web search, etc.).
    Fetch,
    /// Sub-agent or task creation.
    Spawn,
    /// Planning, design-doc, strategy, roadmap.
    Plan,
    /// Shell, cargo, runtime commands, and communication dispatch.
    Execute,
    /// Document / report / slide / spreadsheet generation and output-shaping.
    Format,
}

impl std::fmt::Display for ToolTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recon => f.write_str("recon"),
            Self::Edit => f.write_str("edit"),
            Self::Verify => f.write_str("verify"),
            Self::Fetch => f.write_str("fetch"),
            Self::Spawn => f.write_str("spawn"),
            Self::Plan => f.write_str("plan"),
            Self::Execute => f.write_str("execute"),
            Self::Format => f.write_str("format"),
        }
    }
}

/// Tool definition: the rich metadata that organon tracks internally.
///
/// Converted to `hermeneus::types::ToolDefinition` (the lean LLM wire format)
/// via `ToolRegistry::to_hermeneus_tools`.
///
/// # Examples
///
/// ```no_run
/// use organon::types::{ToolDef, InputSchema, ToolCategory, Reversibility, ToolGroupId, ToolTag};
/// use koina::id::ToolName;
///
/// let def = ToolDef {
///     name: ToolName::new("read_file").expect("valid static tool name"),
///     description: "Read a file from disk.".into(),
///     extended_description: None,
///     input_schema: InputSchema {
///         properties: Default::default(),
///         required: vec![],
///     },
///     category: ToolCategory::Workspace,
///     reversibility: Reversibility::FullyReversible,
///     auto_activate: true,
///     groups: vec![ToolGroupId::Read],
///     tags: vec![ToolTag::Recon],
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Validated tool name.
    pub name: ToolName,
    /// Short description sent to the LLM (token-budget friendly).
    pub description: String,
    /// Detailed description for extended-thinking mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: InputSchema,
    /// Semantic category.
    pub category: ToolCategory,
    /// How reversible this tool's effects are.
    pub reversibility: Reversibility,
    /// Whether the tool activates automatically by domain without explicit config.
    pub auto_activate: bool,
    /// Tool groups this tool belongs to.  Used for role-based gating.
    #[serde(default)]
    pub groups: Vec<ToolGroupId>,
    /// Operational-intent tags for cross-category lookup.
    #[serde(default)]
    pub tags: Vec<ToolTag>,
}

/// JSON Schema for tool input parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSchema {
    /// Property definitions, insertion-ordered.
    pub properties: IndexMap<String, PropertyDef>,
    /// Names of required properties.
    pub required: Vec<String>,
}

impl InputSchema {
    /// Serialize to a JSON Schema `{"type": "object", ...}` value
    /// suitable for `hermeneus::types::ToolDefinition::input_schema`.
    #[must_use]
    pub fn to_json_schema(&self) -> serde_json::Value {
        // kanon:ignore RUST/pub-visibility
        let mut props = serde_json::Map::new();
        for (name, def) in &self.properties {
            props.insert(name.clone(), def.to_json_schema());
        }

        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": self.required,
        })
    }

    /// Validate tool arguments against this schema before dispatch.
    ///
    /// WHY: Catching schema violations before the executor runs gives callers
    /// a clear, machine-readable error and prevents ad hoc parsing failures
    /// deep in tool logic.
    pub(crate) fn validate(
        &self,
        name: &ToolName,
        arguments: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let args = arguments.as_object().ok_or_else(|| {
            crate::error::InvalidInputSnafu {
                name: name.clone(),
                reason: format!(
                    "arguments must be a JSON object, got {}",
                    json_value_type(arguments)
                ),
            }
            .build()
        })?;

        for required in &self.required {
            if !args.contains_key(required) {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("missing required argument: {required}"),
                }
                .build());
            }
        }

        for (key, value) in args {
            if let Some(def) = self.properties.get(key) {
                def.validate(name, key, value)?;
            }
        }

        Ok(())
    }
}

/// A single property in the input schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropertyDef {
    /// The JSON Schema type.
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    /// Human-readable description.
    pub description: String,
    /// Allowed enum values, if constrained.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Schema for array items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<PropertyDef>>,
    /// Schemas for known object properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, PropertyDef>>,
    /// Names of required properties when this property is an object schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Policy for object properties not declared in `properties`.
    /// `false` forbids extra keys; `true` allows any value; a schema object
    /// constrains the values of extra keys.
    #[serde(rename = "additionalProperties", skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<AdditionalProperties>,
    /// Minimum numeric value (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Maximum numeric value (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    /// Minimum string length (Unicode code points).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    /// Maximum string length (Unicode code points).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    /// Regular expression pattern for string values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Minimum number of array items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_items: Option<usize>,
    /// Maximum number of array items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<usize>,
}

/// Policy for object properties not declared in `properties`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    /// Extra properties are allowed or forbidden.
    Bool(bool),
    /// Extra properties must match this schema.
    Schema(Box<PropertyDef>),
}

/// JSON Schema primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PropertyType {
    /// JSON string type.
    #[default]
    String,
    /// JSON number type (float or integer).
    Number,
    /// JSON integer type.
    Integer,
    /// JSON boolean type.
    Boolean,
    /// JSON array type.
    Array,
    /// JSON object type.
    Object,
}

impl std::fmt::Display for PropertyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => f.write_str("string"),
            Self::Number => f.write_str("number"),
            Self::Integer => f.write_str("integer"),
            Self::Boolean => f.write_str("boolean"),
            Self::Array => f.write_str("array"),
            Self::Object => f.write_str("object"),
        }
    }
}

impl PropertyDef {
    /// Serialize this property to a JSON Schema fragment.
    #[must_use]
    pub(crate) fn to_json_schema(&self) -> serde_json::Value {
        let mut prop = serde_json::Map::new();
        prop.insert(
            String::from("type"),
            serde_json::to_value(self.property_type)
                .unwrap_or(serde_json::Value::String("string".to_owned())),
        );
        prop.insert(
            String::from("description"),
            serde_json::Value::String(self.description.clone()),
        );
        if let Some(ref enum_vals) = self.enum_values {
            prop.insert(
                String::from("enum"),
                serde_json::Value::Array(
                    enum_vals
                        .iter()
                        .map(|v| serde_json::Value::String(v.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(ref default) = self.default {
            prop.insert(String::from("default"), default.clone());
        }
        if let Some(ref items) = self.items {
            prop.insert(String::from("items"), items.to_json_schema());
        }
        if let Some(ref properties) = self.properties {
            let mut props = serde_json::Map::new();
            for (name, def) in properties {
                props.insert(name.clone(), def.to_json_schema());
            }
            prop.insert(String::from("properties"), serde_json::Value::Object(props));
        }
        if let Some(ref required) = self.required {
            prop.insert(String::from("required"), serde_json::json!(required));
        }
        if let Some(ref additional) = self.additional_properties {
            let value = match additional {
                AdditionalProperties::Bool(b) => serde_json::json!(b),
                AdditionalProperties::Schema(schema) => schema.to_json_schema(),
            };
            prop.insert(String::from("additionalProperties"), value);
        }
        if let Some(min) = self.minimum {
            prop.insert(String::from("minimum"), serde_json::json!(min));
        }
        if let Some(max) = self.maximum {
            prop.insert(String::from("maximum"), serde_json::json!(max));
        }
        if let Some(min_len) = self.min_length {
            prop.insert(String::from("minLength"), serde_json::json!(min_len));
        }
        if let Some(max_len) = self.max_length {
            prop.insert(String::from("maxLength"), serde_json::json!(max_len));
        }
        if let Some(ref pattern) = self.pattern {
            prop.insert(
                String::from("pattern"),
                serde_json::Value::String(pattern.clone()),
            );
        }
        if let Some(min_items) = self.min_items {
            prop.insert(String::from("minItems"), serde_json::json!(min_items));
        }
        if let Some(max_items) = self.max_items {
            prop.insert(String::from("maxItems"), serde_json::json!(max_items));
        }
        serde_json::Value::Object(prop)
    }

    /// Validate a JSON value against this property schema.
    pub(crate) fn validate(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        self.validate_type(name, path, value)?;
        self.validate_enum(name, path, value)?;
        self.validate_numeric(name, path, value)?;
        self.validate_string(name, path, value)?;
        self.validate_array(name, path, value)?;
        self.validate_object(name, path, value)?;
        Ok(())
    }

    fn validate_type(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let valid = match self.property_type {
            PropertyType::String => value.is_string(),
            PropertyType::Number => value.is_number(),
            PropertyType::Integer => value.is_i64() || value.is_u64(),
            PropertyType::Boolean => value.is_boolean(),
            PropertyType::Array => value.is_array(),
            PropertyType::Object => value.is_object(),
        };
        if !valid {
            return Err(crate::error::InvalidInputSnafu {
                name: name.clone(),
                reason: format!(
                    "{path}: expected {}, got {}",
                    self.property_type,
                    json_value_type(value)
                ),
            }
            .build());
        }
        Ok(())
    }

    fn validate_enum(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let Some(ref enum_vals) = self.enum_values else {
            return Ok(());
        };
        let s = value.as_str().ok_or_else(|| {
            crate::error::InvalidInputSnafu {
                name: name.clone(),
                reason: format!("{path}: enum constraint requires a string value"),
            }
            .build()
        })?;
        if !enum_vals.iter().any(|v| v == s) {
            return Err(crate::error::InvalidInputSnafu {
                name: name.clone(),
                reason: format!("{path}: value {s:?} not in enum {enum_vals:?}"),
            }
            .build());
        }
        Ok(())
    }

    fn validate_numeric(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let Some(num) = value.as_f64() else {
            return Ok(());
        };
        if let Some(min) = self.minimum {
            if num < min {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: value {num} is below minimum {min}"),
                }
                .build());
            }
        }
        if let Some(max) = self.maximum {
            if num > max {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: value {num} is above maximum {max}"),
                }
                .build());
            }
        }
        Ok(())
    }

    fn validate_string(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let Some(s) = value.as_str() else {
            return Ok(());
        };
        let len = s.chars().count();
        if let Some(min) = self.min_length {
            if len < min {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: string length {len} is below minimum {min}"),
                }
                .build());
            }
        }
        if let Some(max) = self.max_length {
            if len > max {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: string length {len} is above maximum {max}"),
                }
                .build());
            }
        }
        if let Some(ref pattern) = self.pattern {
            let re = regex::Regex::new(pattern).map_err(|e| {
                crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: invalid pattern {pattern:?}: {e}"),
                }
                .build()
            })?;
            if !re.is_match(s) {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: value {s:?} does not match pattern {pattern:?}"),
                }
                .build());
            }
        }
        Ok(())
    }

    fn validate_array(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let Some(arr) = value.as_array() else {
            return Ok(());
        };
        if let Some(min) = self.min_items {
            if arr.len() < min {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: array length {} is below minimum {min}", arr.len()),
                }
                .build());
            }
        }
        if let Some(max) = self.max_items {
            if arr.len() > max {
                return Err(crate::error::InvalidInputSnafu {
                    name: name.clone(),
                    reason: format!("{path}: array length {} is above maximum {max}", arr.len()),
                }
                .build());
            }
        }
        if let Some(ref items) = self.items {
            for (i, item) in arr.iter().enumerate() {
                items.validate(name, &format!("{path}[{i}]"), item)?;
            }
        }
        Ok(())
    }

    fn validate_object(
        &self,
        name: &ToolName,
        path: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let Some(obj) = value.as_object() else {
            return Ok(());
        };
        if let Some(ref required) = self.required {
            for key in required {
                if !obj.contains_key(key) {
                    return Err(crate::error::InvalidInputSnafu {
                        name: name.clone(),
                        reason: format!("{path}: missing required property {key:?}"),
                    }
                    .build());
                }
            }
        }
        if let Some(ref properties) = self.properties {
            for (key, val) in obj {
                if let Some(prop_def) = properties.get(key) {
                    prop_def.validate(name, &format!("{path}.{key}"), val)?;
                } else if let Some(ref additional) = self.additional_properties {
                    match additional {
                        AdditionalProperties::Bool(false) => {
                            return Err(crate::error::InvalidInputSnafu {
                                name: name.clone(),
                                reason: format!("{path}: extra property {key:?} is not allowed"),
                            }
                            .build());
                        }
                        AdditionalProperties::Bool(true) => {}
                        AdditionalProperties::Schema(schema) => {
                            schema.validate(name, &format!("{path}.{key}"), val)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn json_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// How reversible a tool's effects are.
///
/// Used by the approval system to determine which tool calls need confirmation
/// and whether counterfactual dry-run simulation is meaningful.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reversibility {
    /// Read-only operations with no side effects (ls, read, search).
    FullyReversible,
    /// Can be undone (write file with backup, git commit can revert).
    Reversible,
    /// Partial undo possible (delete file can restore from backup if exists).
    PartiallyReversible,
    /// Cannot be undone (exec with external side effects, API calls, messages).
    #[default]
    Irreversible,
}

impl std::fmt::Display for Reversibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullyReversible => f.write_str("fully_reversible"),
            Self::Reversible => f.write_str("reversible"),
            Self::PartiallyReversible => f.write_str("partially_reversible"),
            Self::Irreversible => f.write_str("irreversible"),
        }
    }
}

#[cfg(test)]
impl Reversibility {
    /// Whether this tool's effects can be simulated in a dry run.
    #[must_use]
    pub(crate) fn supports_dry_run(self) -> bool {
        matches!(self, Self::FullyReversible | Self::Reversible)
    }
}

/// What level of approval a tool call requires before execution.
///
/// Derived from [`Reversibility`] but can be overridden per-tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalRequirement {
    /// No approval needed (read-only, fully reversible).
    None,
    /// Approval recommended but not enforced.
    Advisory,
    /// Approval required before execution.
    Required,
    /// Approval required with explicit confirmation prompt.
    Mandatory,
}

impl std::fmt::Display for ApprovalRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Advisory => f.write_str("advisory"),
            Self::Required => f.write_str("required"),
            Self::Mandatory => f.write_str("mandatory"),
        }
    }
}

impl From<Reversibility> for ApprovalRequirement {
    fn from(rev: Reversibility) -> Self {
        match rev {
            Reversibility::FullyReversible => Self::None,
            Reversibility::Reversible => Self::Advisory,
            Reversibility::PartiallyReversible => Self::Required,
            Reversibility::Irreversible => Self::Mandatory,
        }
    }
}

/// Origin metadata for an externally-provided tool.
///
/// WHY: MCP and other external tool planes expose tools whose public local
/// name is namespaced to avoid collisions. Keeping the original server and
/// remote name alongside the local name makes origin unambiguous in
/// diagnostics, approval prompts, and audit trails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOrigin {
    /// Local name under which the tool is registered.
    pub local_name: String,
    /// External server or service that provides the tool.
    pub server_name: String,
    /// Tool name as exposed by the external server.
    pub remote_name: String,
}

/// Metadata recorded per tool call for session audit trails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMetadata {
    /// The tool's reversibility classification at call time.
    pub reversibility: Reversibility,
    /// The approval requirement that was applied.
    pub approval: ApprovalRequirement,
    /// Whether the call was a dry-run simulation.
    pub dry_run: bool,
    /// Origin metadata when the tool is externally provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ToolOrigin>,
}

/// Semantic tool category: classifies tool purpose.
///
/// This is a semantic classification, not a loading strategy.
/// The loading strategy (essential vs available) is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCategory {
    /// Filesystem operations.
    Workspace,
    /// Memory operations.
    Memory,
    /// Messaging and cross-agent communication.
    Communication,
    /// Planning and deliberation.
    Planning,
    /// System and configuration.
    System,
    /// Agent coordination and spawning.
    Agent,
    /// Web research and information retrieval.
    Research,
    /// External domain pack tools.
    Domain,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace => f.write_str("workspace"),
            Self::Memory => f.write_str("memory"),
            Self::Communication => f.write_str("communication"),
            Self::Planning => f.write_str("planning"),
            Self::System => f.write_str("system"),
            Self::Agent => f.write_str("agent"),
            Self::Research => f.write_str("research"),
            Self::Domain => f.write_str("domain"),
        }
    }
}

impl ToolCategory {
    /// Unicode icon for TUI display.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Workspace => "≡",
            Self::Memory => "⊙",
            Self::Communication => "↔",
            Self::Planning => "◈",
            Self::System => "⚙",
            Self::Agent => "⊛",
            Self::Research => "⊕",
            Self::Domain => "○",
        }
    }

    /// Human-readable display name.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Memory => "Memory",
            Self::Communication => "Communication",
            Self::Planning => "Planning",
            Self::System => "System",
            Self::Agent => "Agent",
            Self::Research => "Research",
            Self::Domain => "Domain",
        }
    }

    /// Whether this category is read-only (non-destructive).
    #[must_use]
    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Research | Self::Planning)
    }

    /// Whether this category performs destructive or irreversible operations.
    #[must_use]
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::System | Self::Agent | Self::Communication)
    }
}

/// Diagnostic metadata preserved from tool execution.
///
/// Populated by tools that run in a rich environment (subprocesses,
/// sandboxes, etc.) so the LLM can distinguish failure modes such as
/// "command not found" vs "permission denied".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDiagnostics {
    /// Subprocess exit code, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Captured stderr output, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Sandbox policy violations that occurred during execution.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sandbox_violations: Vec<String>,
    /// Wall-clock execution duration in milliseconds.
    pub duration_ms: u64,
}

impl ToolDiagnostics {
    /// Format diagnostics for LLM consumption.
    ///
    /// Each field is bounded to avoid consuming excessive token budget.
    /// The total output is designed to fit in ~256 tokens.
    #[must_use]
    pub fn to_llm_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(code) = self.exit_code {
            parts.push(format!("exit_code={code}"));
        }
        if let Some(stderr) = &self.stderr {
            let trimmed = stderr.trim();
            if !trimmed.is_empty() {
                let max = 512;
                let bounded = if trimmed.len() > max {
                    let end = trimmed.floor_char_boundary(max);
                    let prefix = trimmed.get(..end).unwrap_or(trimmed);
                    format!("{prefix}…[truncated]")
                } else {
                    trimmed.to_owned()
                };
                parts.push(format!("stderr={bounded}"));
            }
        }
        if !self.sandbox_violations.is_empty() {
            let joined = self.sandbox_violations.join(", ");
            let max = 256;
            let bounded = if joined.len() > max {
                let end = joined.floor_char_boundary(max);
                let prefix = joined.get(..end).unwrap_or(&joined);
                format!("{prefix}…[truncated]")
            } else {
                joined
            };
            parts.push(format!("sandbox_violations={bounded}"));
        }
        parts.push(format!("duration_ms={}", self.duration_ms));
        format!("[diagnostics: {}]", parts.join(", "))
    }
}

/// Rich outcome classification for a tool invocation.
///
/// Replaces the collapsed `is_error: bool` view with three cases that
/// expose partial-success states — e.g. `sessions_dispatch` running
/// several sub-agents where some succeed and some fail (#3633).
///
/// Legacy callers read [`ToolResult::is_error`], which remains derived
/// from the outcome for backward compatibility: `PartialSuccess` maps
/// to `is_error = false` because the tool itself delivered usable
/// output; the LLM sees caveats inside the content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolOutcome {
    /// All sub-operations succeeded.
    #[default]
    Success,
    /// Tool returned usable output, but some sub-operations failed or
    /// emitted warnings. Payload is boxed to keep `ToolOutcome`
    /// (and therefore `ToolResult`) small so that
    /// `Result<_, ToolResult>` helpers stay under the
    /// `clippy::result_large_err` threshold.
    PartialSuccess(Box<PartialSuccessInfo>),
    /// Tool failed; no usable output.
    Failure(Box<FailureInfo>),
}

/// Payload for [`ToolOutcome::PartialSuccess`]. Boxed in the outcome
/// to keep the enum pointer-sized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialSuccessInfo {
    /// One reason per degraded sub-operation.
    pub reasons: Vec<String>,
}

/// Payload for [`ToolOutcome::Failure`]. Boxed in the outcome to keep
/// the enum pointer-sized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureInfo {
    /// Single human-readable failure reason.
    pub reason: String,
}

impl ToolOutcome {
    /// Construct a `PartialSuccess` outcome from an iterator of reasons.
    #[must_use]
    pub fn partial(reasons: impl IntoIterator<Item = String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self::PartialSuccess(Box::new(PartialSuccessInfo {
            reasons: reasons.into_iter().collect(),
        }))
    }

    /// Construct a `Failure` outcome from a single reason.
    #[must_use]
    pub fn failure(reason: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self::Failure(Box::new(FailureInfo {
            reason: reason.into(),
        }))
    }

    /// Whether this outcome represents a pure error state.
    ///
    /// `PartialSuccess` returns `false` because the tool produced
    /// usable output even if some sub-operations were degraded — the
    /// caveats travel in the content payload, not the error channel.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failure(_))
    }

    /// Whether this outcome represents a fully-successful run.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Whether this outcome represents a partial success.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        matches!(self, Self::PartialSuccess(_))
    }

    /// Reasons recorded for `PartialSuccess`, empty otherwise.
    #[must_use]
    pub fn partial_reasons(&self) -> &[String] {
        match self {
            Self::PartialSuccess(info) => info.reasons.as_slice(),
            _ => &[],
        }
    }

    /// Reason recorded for `Failure`, empty string otherwise.
    #[must_use]
    pub fn failure_reason(&self) -> &str {
        match self {
            Self::Failure(info) => info.reason.as_str(),
            _ => "",
        }
    }
}

/// What the tool executor returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Result content: text or rich content blocks.
    pub content: ToolResultContent,
    /// Whether this result represents an error.
    ///
    /// Retained for backward compatibility and wire-format stability
    /// (serialized LLM-facing envelopes and persisted sessions).
    /// New code should inspect [`ToolResult::outcome`] for the
    /// partial-success distinction (#3633); `is_error` stays
    /// synchronized with it via the constructors and remains a
    /// faithful binary collapse: `Success` or `PartialSuccess`
    /// → `false`, `Failure` → `true`.
    pub is_error: bool,
    /// Rich outcome classification. Defaults to `Success` when a
    /// legacy payload without this field is deserialized; the
    /// `is_error` flag is not consulted during deserialization
    /// because `#[serde(default)]` runs before field population.
    /// For legacy inputs, call [`ToolResult::normalize`] to reconcile.
    #[serde(default)]
    pub outcome: ToolOutcome,
    /// Optional diagnostic metadata from the execution environment.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diagnostics: Option<ToolDiagnostics>,
    /// HMAC-SHA256 receipt for hallucination-resistant attestation.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub receipt: Option<String>,
}

impl ToolResult {
    /// Create a text-only success result.
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
            outcome: ToolOutcome::Success,
            diagnostics: None,
            receipt: None,
        }
    }

    /// Create a text-only error result.
    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        let reason = content.into();
        Self {
            content: ToolResultContent::Text(reason.clone()),
            is_error: true,
            outcome: ToolOutcome::failure(reason),
            diagnostics: None,
            receipt: None,
        }
    }

    /// Create a result with rich content blocks.
    #[must_use]
    pub fn blocks(blocks: Vec<ToolResultBlock>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Blocks(blocks),
            is_error: false,
            outcome: ToolOutcome::Success,
            diagnostics: None,
            receipt: None,
        }
    }

    /// Create a partial-success result: the tool produced usable
    /// output, but some sub-operations failed or emitted warnings.
    ///
    /// `reasons` should carry one human-readable note per degraded
    /// sub-operation. `is_error` is set to `false` because the tool
    /// surface delivered output; caveats travel in-band.
    ///
    /// See #3633 — `sessions_dispatch` is the canonical producer.
    #[must_use]
    pub fn partial_success(
        content: impl Into<String>,
        reasons: impl IntoIterator<Item = String>,
    ) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
            outcome: ToolOutcome::partial(reasons),
            diagnostics: None,
            receipt: None,
        }
    }

    /// Reconcile `outcome` against `is_error` after deserialization.
    ///
    /// Legacy payloads omit the `outcome` field, so it defaults to
    /// `Success`; call this to promote `is_error = true` legacy
    /// records to `Failure` so downstream observers see the error
    /// through the new API.
    #[must_use]
    pub fn normalize(mut self) -> Self {
        // kanon:ignore RUST/pub-visibility
        if self.is_error && matches!(self.outcome, ToolOutcome::Success) {
            // text_summary() already returns an owned String.
            let reason = self.content.text_summary();
            self.outcome = ToolOutcome::failure(reason);
        }
        self
    }

    /// Attach diagnostic metadata to this result.
    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: ToolDiagnostics) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Attach a receipt to this result.
    #[must_use]
    pub fn with_receipt(mut self, receipt: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.receipt = Some(receipt.into());
        self
    }
}

/// Input to a tool execution (from the LLM's `tool_use` block).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    /// Which tool to invoke.
    pub name: ToolName,
    /// The `tool_use` block ID from the LLM response.
    pub tool_use_id: String,
    /// The arguments the LLM provided.
    pub arguments: serde_json::Value,
}

/// Cumulative tool execution statistics for a pipeline turn.
#[cfg(test)]
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ToolStats {
    pub total_calls: u32,
    pub total_duration_ms: u64,
    pub error_count: u32,
    /// Count of calls that produced `ToolOutcome::PartialSuccess`
    /// (see #3633). Separate from `error_count` because partial
    /// successes deliver usable output even with degraded
    /// sub-operations.
    pub partial_count: u32,
    pub calls_by_tool: IndexMap<String, u32>,
}

#[cfg(test)]
impl ToolStats {
    /// Record a tool execution with full outcome detail.
    pub(crate) fn record_outcome(&mut self, name: &str, duration_ms: u64, outcome: &ToolOutcome) {
        // kanon:ignore RUST/pub-visibility
        self.total_calls += 1;
        self.total_duration_ms += duration_ms;
        match outcome {
            ToolOutcome::Success => {}
            ToolOutcome::PartialSuccess(_) => self.partial_count += 1,
            ToolOutcome::Failure(_) => self.error_count += 1,
        }
        *self.calls_by_tool.entry(name.to_owned()).or_insert(0) += 1;
    }

    /// Top N tools by call count.
    #[must_use]
    pub(crate) fn top_tools(&self, n: usize) -> Vec<(&str, u32)> {
        // kanon:ignore RUST/pub-visibility
        let mut sorted: Vec<_> = self
            .calls_by_tool
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
        sorted.truncate(n);
        sorted
    }
}

#[cfg(test)]
#[path = "../types_tests/mod.rs"]
mod tests;
