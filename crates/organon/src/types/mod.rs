//! Core types for tool definitions, input/output, and execution context.

mod context;
mod services;

pub use context::{ServerToolConfig, ToolContext, ToolServices};
pub use services::{
    BlackboardEntry, BlackboardStore, CrossNousService, DatalogResult, FactSummary,
    KnowledgeSearchService, MemoryResult, MessageService, NoteEntry, NoteStore, PlanningService,
    SpawnRequest, SpawnResult, SpawnService,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use hermeneus::types::{DocumentSource, ImageSource, ToolResultBlock, ToolResultContent};
use koina::id::ToolName;

/// Machine-checkable tool groups for role-based gating.
///
/// Each tool declares which groups it belongs to; roles declare which groups
/// they are allowed to use.  The registry filters the tool surface presented
/// to the LLM and rejects out-of-group calls at dispatch time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolGroupId {
    /// File/code reading tools (`read`, `grep`, `find`, `ls`, `view_file`, ...).
    Read,
    /// File/code mutation tools (`write`, `edit`, `mkdir`, `mv`, `cp`, `rm`, ...).
    Edit,
    /// Shell/cargo execution (`exec`, `git_checkout`, `computer_use`, ...).
    Command,
    /// MCP tool invocation and external API calls (`web_fetch`, `http_request`, ...).
    Mcp,
    /// Spawning sub-agents (`sessions_spawn`, `sessions_dispatch`, ...).
    SpawnSubtask,
    /// Planning and design tools (`plan_create`, `plan_roadmap`, ...).
    Plan,
    /// Tests, lint, fmt, and verification tools (`lint_report`, `verify_report`, ...).
    Verify,
}

impl std::fmt::Display for ToolGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => f.write_str("read"),
            Self::Edit => f.write_str("edit"),
            Self::Command => f.write_str("command"),
            Self::Mcp => f.write_str("mcp"),
            Self::SpawnSubtask => f.write_str("spawn_subtask"),
            Self::Plan => f.write_str("plan"),
            Self::Verify => f.write_str("verify"),
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
/// use organon::types::{ToolDef, InputSchema, ToolCategory, Reversibility, ToolGroupId};
/// use koina::id::ToolName;
///
/// let def = ToolDef {
///     name: ToolName::new("read_file").unwrap(),
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
            let mut prop = serde_json::Map::new();
            prop.insert(
                String::from("type"),
                serde_json::to_value(def.property_type)
                    .unwrap_or(serde_json::Value::String("string".to_owned())),
            );
            prop.insert(
                String::from("description"),
                serde_json::Value::String(def.description.clone()),
            );
            if let Some(ref enum_vals) = def.enum_values {
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
            if let Some(ref default) = def.default {
                prop.insert(String::from("default"), default.clone());
            }
            props.insert(name.clone(), serde_json::Value::Object(prop));
        }

        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": self.required,
        })
    }
}

/// A single property in the input schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// JSON Schema primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PropertyType {
    /// JSON string type.
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

impl Reversibility {
    /// Whether this tool's effects can be simulated in a dry run.
    #[must_use]
    pub fn supports_dry_run(self) -> bool {
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

/// Metadata recorded per tool call for session audit trails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMetadata {
    /// The tool's reversibility classification at call time.
    pub reversibility: Reversibility,
    /// The approval requirement that was applied.
    pub approval: ApprovalRequirement,
    /// Whether the call was a dry-run simulation.
    pub dry_run: bool,
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
#[derive(Debug, Clone, Default, Serialize)]
#[expect(
    missing_docs,
    reason = "stats struct fields are self-documenting by name"
)]
pub struct ToolStats {
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

impl ToolStats {
    /// Record a tool execution (binary-outcome variant).
    ///
    /// Retained for call sites that only know `is_error`; prefer
    /// [`ToolStats::record_outcome`] to capture partial-success
    /// state explicitly.
    pub fn record(&mut self, name: &str, duration_ms: u64, is_error: bool) {
        // kanon:ignore RUST/pub-visibility
        let outcome = if is_error {
            ToolOutcome::failure(String::new())
        } else {
            ToolOutcome::Success
        };
        self.record_outcome(name, duration_ms, &outcome);
    }

    /// Record a tool execution with full outcome detail.
    pub fn record_outcome(&mut self, name: &str, duration_ms: u64, outcome: &ToolOutcome) {
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
    pub fn top_tools(&self, n: usize) -> Vec<(&str, u32)> {
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
