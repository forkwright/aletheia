//! MCP meta-tool: `tool_schema` — fetch the full input schema for a named tool.
//!
//! When the `deferred-schemas` feature is enabled, LLM requests carry only tool
//! names and one-line descriptions rather than full JSON schemas.  Agents call
//! `tool_schema` **before** invoking any tool whose schema they have not yet
//! seen.  The returned JSON object is the complete `input_schema` that would
//! normally appear in the system-prompt tool block.
//!
//! # Usage pattern (deferred-schemas mode)
//!
//! 1. The system prompt lists all tools with `name` + `description` only.
//! 2. Agent decides it wants to call, say, `plan_create`.
//! 3. Agent calls `tool_schema { "tool_name": "plan_create" }`.
//! 4. Response contains the full `input_schema` JSON object.
//! 5. Agent constructs its `plan_create` call with the correct parameters.
//!
//! # Feature flag
//!
//! This tool is registered regardless of the `deferred-schemas` flag so that
//! it is always available as a discovery aid.  The flag controls whether
//! *callers* (e.g. `nous`) switch from full-schema to summary serialization.
//! That way the tool is never absent if an operator enables the flag later in
//! a running deployment.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

// ── Executor ─────────────────────────────────────────────────────────────────

/// Pre-computed map of `tool_name → serialized JSON schema string`.
///
/// WHY: The executor captures a snapshot of schemas at registration time rather
/// than holding an `Arc<ToolRegistry>` (which would create a self-referential
/// ownership cycle — the registry owns the executor that would own the registry).
/// Schemas are static for the session lifetime, so this snapshot is valid and
/// avoids the cycle entirely.
struct ToolSchemaExecutor {
    schemas: HashMap<String, String>,
}

impl ToolExecutor for ToolSchemaExecutor {
    #[tracing::instrument(skip(self, input, _ctx), fields(queried_tool = ?input.arguments.get("tool_name").and_then(|v| v.as_str())))]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(tool_name_str) = input.arguments.get("tool_name").and_then(|v| v.as_str())
            else {
                return Ok(ToolResult::error("missing required field: tool_name"));
            };

            match self.schemas.get(tool_name_str) {
                Some(schema_json) => {
                    tracing::debug!(
                        tool.name = tool_name_str,
                        "tool_schema: serving full schema"
                    );
                    Ok(ToolResult::text(schema_json.clone()))
                }
                None => Ok(ToolResult::error(format!(
                    "no tool named '{tool_name_str}' is registered; \
                     check the tool list in the system prompt for available tool names"
                ))),
            }
        })
    }
}

// ── ToolDef ──────────────────────────────────────────────────────────────────

fn tool_schema_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("tool_schema"), // kanon:ignore RUST/expect
        description: "Fetch the full input schema (JSON Schema) for a named tool. \
             Call this before invoking any tool whose parameters you have not yet seen. \
             Returns the complete input_schema object so you can construct a valid call."
            .to_owned(),
        extended_description: Some(
            "In deferred-schemas mode the system prompt lists only tool names and one-line \
             descriptions. Use tool_schema to retrieve the full parameter specification for \
             any tool before calling it. Example: tool_schema({\"tool_name\": \"plan_create\"}) \
             returns the JSON Schema for plan_create's input parameters."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "tool_name".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Name of the tool whose schema you want to retrieve \
                                  (e.g. \"plan_create\", \"exec\", \"memory_search\")."
                        .to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec!["tool_name".to_owned()],
        },
        category: ToolCategory::Research,
        // WHY: FullyReversible — read-only lookup of static registry data, no
        // side effects, no I/O.
        reversibility: Reversibility::FullyReversible,
        // WHY: auto_activate = true so `tool_schema` is always present in the
        // tool list even before any enable_tool call.  Agents need it for
        // bootstrap: the first unknown tool they want to call requires schema
        // retrieval, so it must be available unconditionally.
        auto_activate: true,
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `tool_schema` meta-tool into `registry` using pre-computed
/// `(tool_name, schema_json)` pairs.
///
/// # Why pairs instead of `&ToolRegistry`
///
/// Passing `&ToolRegistry` alongside `&mut ToolRegistry` in the same call is
/// rejected by the Rust borrow checker (overlapping borrows on the same value).
/// The caller resolves this by extracting `definitions()` into a `Vec` while
/// it holds an immutable borrow, then calling this function with a `&mut`
/// borrow.  This function consumes the pre-built pairs directly.
///
/// # Errors
///
/// Returns an error if `tool_schema` is already registered (duplicate name).
pub(crate) fn register_with_pairs(
    registry: &mut ToolRegistry,
    pairs: Vec<(String, String)>,
) -> Result<()> {
    let schema_count = pairs.len();
    let schemas: HashMap<String, String> = pairs.into_iter().collect();

    tracing::info!(
        schema_count,
        "tool_schema: registered with {schema_count} pre-computed schemas"
    );

    registry.register(tool_schema_def(), Box::new(ToolSchemaExecutor { schemas }))?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use crate::builtins::register_all_with_sandbox;
    use crate::registry::ToolRegistry;
    use crate::sandbox::SandboxConfig;
    use crate::testing::install_crypto_provider;
    use crate::types::{ServerToolConfig, ToolContext, ToolInput, ToolServices};

    fn mock_ctx() -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_client: reqwest::Client::new(),
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    /// Build a registry with all builtins registered, including `tool_schema`.
    ///
    /// Mirrors the two-phase pattern in `register_all_with_sandbox`: first
    /// register domain tools, then extract schema pairs, then register
    /// `tool_schema` with those pairs.
    fn build_registry() -> ToolRegistry {
        use crate::builtins::register_domain_tools;

        // Phase 1: register domain tools.
        let mut reg = ToolRegistry::new();
        register_domain_tools(&mut reg, SandboxConfig::default()).expect("register_domain_tools");

        // Phase 2: extract schema pairs while registry is immutably borrowed.
        let pairs: Vec<(String, String)> = reg
            .definitions()
            .into_iter()
            .filter_map(|def| {
                let schema = def.input_schema.to_json_schema();
                serde_json::to_string_pretty(&schema)
                    .ok()
                    .map(|json| (def.name.as_str().to_owned(), json))
            })
            .collect();

        // Phase 3: register tool_schema with the pairs.
        super::register_with_pairs(&mut reg, pairs).expect("register tool_schema");
        reg
    }

    // ── happy path ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn tool_schema_returns_full_schema_for_known_tool() {
        let reg = build_registry();
        let ctx = mock_ctx();

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({ "tool_name": "read" }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected success for known tool");

        let text = result.content.text_summary();
        // The schema for `read` must contain its `path` property.
        assert!(
            text.contains("path"),
            "expected schema to contain 'path' property, got: {text}"
        );
        // Must be valid JSON.
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("result must be valid JSON");
        let schema_type = parsed
            .get("type")
            .and_then(|v| v.as_str())
            .expect("schema must have a 'type' field");
        assert_eq!(
            schema_type, "object",
            "schema must have type=object at root"
        );
    }

    // ── error variant ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn tool_schema_returns_error_for_unknown_tool() {
        let reg = build_registry();
        let ctx = mock_ctx();

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_2".to_owned(),
            arguments: serde_json::json!({ "tool_name": "does_not_exist" }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error, "expected error for unknown tool");
        assert!(
            result.content.text_summary().contains("does_not_exist"),
            "error message should mention the unknown tool name"
        );
    }

    // ── eager-load regression guard ──────────────────────────────────────────

    #[test]
    fn eager_load_still_works_with_flag_off() {
        // Without the deferred-schemas feature, to_hermeneus_tools must still
        // return full schemas (input_schema has real content, not empty stubs).
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register_all");

        let tools = reg.to_hermeneus_tools();
        assert!(
            !tools.is_empty(),
            "eager-load must return at least one tool definition"
        );

        // Every tool must have a non-trivial input_schema (type=object).
        for t in &tools {
            let schema_type = t
                .input_schema
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("<missing>");
            assert_eq!(
                schema_type, "object",
                "tool '{}' must have a valid input_schema with type=object",
                t.name
            );
        }
    }

    // ── deferred-load schema content check ───────────────────────────────────

    #[test]
    #[cfg(feature = "deferred-schemas")]
    fn deferred_load_serializes_only_names_and_summaries() {
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register_all");

        let summaries = reg.to_hermeneus_tools_summaries();
        assert!(!summaries.is_empty(), "must return at least one entry");

        for t in &summaries {
            // Name and description must be non-empty.
            assert!(!t.name.is_empty(), "name must not be empty");
            assert!(!t.description.is_empty(), "description must not be empty");
            // input_schema must be the stub (empty properties, no real params).
            let props = t
                .input_schema
                .get("properties")
                .expect("input_schema must have 'properties'");
            assert_eq!(
                props,
                &serde_json::json!({}),
                "deferred summaries must carry empty properties stub for tool '{}'",
                t.name
            );
        }
    }

    // ── measurable byte-size reduction ───────────────────────────────────────

    #[test]
    #[cfg(feature = "deferred-schemas")]
    fn deferred_load_shrinks_request_size_measurably() {
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register_all");

        let (summary_bytes, schema_bytes) = reg.schema_byte_sizes();

        assert!(
            schema_bytes > 0,
            "full-schema payload must be non-empty (got 0 bytes)"
        );
        assert!(
            summary_bytes > 0,
            "summary payload must be non-empty (got 0 bytes)"
        );
        assert!(
            summary_bytes < schema_bytes,
            "deferred payload ({summary_bytes}B) must be smaller than eager payload ({schema_bytes}B)"
        );

        // Require at least 50% reduction (issue requirement).
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "byte-count sizes are small enough that usize->f64 is exact for ratio computation"
        )]
        let ratio = summary_bytes as f64 / schema_bytes as f64; // kanon:ignore RUST/as-cast
        assert!(
            ratio <= 0.50,
            "expected ≥50% size reduction: summary={summary_bytes}B schema={schema_bytes}B ratio={ratio:.2}"
        );
    }
}
