//! MCP meta-tool: `tool_schema` — fetch the full input schema for a named tool.
//!
//! When the `deferred-schemas` feature is enabled, LLM requests carry only tool
//! names and one-line descriptions rather than full JSON schemas.  Agents call
//! `tool_schema` **before** invoking any tool whose schema they have not yet
//! seen.  The returned JSON object is the complete `input_schema` that would
//! normally appear in the system-prompt tool block.
//!
//! Usage (deferred-schemas mode): the system prompt lists tools by name and
//! description only; agents call `tool_schema {"tool_name": …}` for the full
//! `input_schema` before constructing the real call.
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
use std::sync::{Arc, RwLock};

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::surface::{SurfaceAvailability, SurfaceEntryKind, SurfaceLookup};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

// ── Executor ─────────────────────────────────────────────────────────────────

/// Shared map of `tool_name → serialized JSON schema string`.
///
/// WHY: The executor captures a snapshot of schemas at registration time rather
/// than holding an `Arc<ToolRegistry>` (which would create a self-referential
/// ownership cycle — the registry owns the executor that would own the registry).
/// The snapshot is wrapped in a lock so the registry can publish a finalized
/// snapshot after domain packs and external tools are registered, without
/// breaking the ownership boundary between the registry and its executors.
type ToolSchemaSnapshot = crate::registry::ToolSchemaSnapshot;

struct ToolSchemaExecutor {
    schemas: ToolSchemaSnapshot,
}

impl ToolExecutor for ToolSchemaExecutor {
    #[tracing::instrument(skip(self, input, ctx), fields(queried_tool = ?input.arguments.get("tool_name").and_then(|v| v.as_str())))]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(tool_name_str) = input.arguments.get("tool_name").and_then(|v| v.as_str())
            else {
                return Ok(ToolResult::error("missing required field: tool_name"));
            };

            if let Some(surface) = ctx.effective_surface() {
                let Ok(tool_name) = ToolName::new(tool_name_str.to_owned()) else {
                    return Ok(ToolResult::text(unavailable_schema_response(
                        tool_name_str,
                        "unknown_tool",
                    )));
                };
                return match surface.lookup(&tool_name) {
                    SurfaceLookup::Callable(entry) if entry.kind == SurfaceEntryKind::Registry => {
                        match entry.input_schema.as_ref() {
                            Some(schema) => Ok(ToolResult::text(format_json(schema))),
                            None => Ok(ToolResult::text(unavailable_schema_response(
                                tool_name_str,
                                "schema_unavailable",
                            ))),
                        }
                    }
                    SurfaceLookup::Callable(entry) if entry.kind == SurfaceEntryKind::Server => Ok(
                        ToolResult::text(available_server_tool_response(entry.name.as_str())),
                    ),
                    SurfaceLookup::Callable(_) => Ok(ToolResult::text(
                        unavailable_schema_response(tool_name_str, "schema_unavailable"),
                    )),
                    SurfaceLookup::Inactive(entry) | SurfaceLookup::Denied(entry) => Ok(
                        ToolResult::text(unavailable_schema_response_with_availability(
                            entry.name.as_str(),
                            &entry.availability,
                        )),
                    ),
                    SurfaceLookup::Unknown => Ok(ToolResult::text(unavailable_schema_response(
                        tool_name_str,
                        "unknown_tool",
                    ))),
                };
            }

            let Ok(schemas) = self.schemas.read() else {
                return Ok(ToolResult::error(
                    "tool_schema snapshot lock is poisoned".to_owned(),
                ));
            };

            match schemas.get(tool_name_str) {
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
                    ..Default::default()
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
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
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
    let schemas: ToolSchemaSnapshot = Arc::new(RwLock::new(
        pairs.into_iter().collect::<HashMap<String, String>>(),
    ));

    tracing::info!(
        schema_count,
        "tool_schema: registered with {schema_count} pre-computed schemas"
    );

    registry.register(
        tool_schema_def(),
        Box::new(ToolSchemaExecutor {
            schemas: Arc::clone(&schemas),
        }),
    )?;
    registry.set_tool_schema_snapshot(Some(schemas));
    Ok(())
}

fn format_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn unavailable_schema_response(tool_name: &str, reason: &str) -> String {
    format_json(&serde_json::json!({
        "tool_name": tool_name,
        "available": false,
        "reason": reason,
    }))
}

fn unavailable_schema_response_with_availability(
    tool_name: &str,
    availability: &SurfaceAvailability,
) -> String {
    let reason = match availability {
        SurfaceAvailability::Inactive => "inactive",
        SurfaceAvailability::Denied(reason) => reason.as_str(),
        SurfaceAvailability::Callable => "schema_unavailable",
    };
    unavailable_schema_response(tool_name, reason)
}

fn available_server_tool_response(tool_name: &str) -> String {
    format_json(&serde_json::json!({
        "tool_name": tool_name,
        "available": true,
        "kind": "server",
        "reason": "server_tool_has_no_local_schema",
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, RwLock};

    use indexmap::IndexMap;
    use koina::id::{NousId, SessionId, ToolName};

    use crate::builtins::register_all_with_sandbox;
    use crate::registry::{ToolExecutor, ToolRegistry};
    use crate::sandbox::SandboxConfig;
    use crate::surface::SurfaceInputs;
    use crate::testing::install_crypto_provider;
    use crate::types::{
        InputSchema, PropertyDef, PropertyType, Reversibility, ServerToolConfig, ToolCategory,
        ToolContext, ToolDef, ToolGroupId, ToolGroupPolicy, ToolHttpClients, ToolInput, ToolResult,
        ToolServices, ToolTag,
    };

    fn mock_ctx() -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_clients: ToolHttpClients::for_tests(),
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
        register_domain_tools(
            &mut reg,
            SandboxConfig::default(),
            #[cfg(feature = "energeia")]
            None,
        )
        .expect("register_domain_tools");

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

    #[tokio::test]
    async fn tool_schema_uses_bound_effective_surface_for_denials() {
        let reg = build_registry();
        let ctx = mock_ctx();
        let active = HashSet::new();
        let allowlist = vec!["tool_schema".to_owned()];
        let policy = ToolGroupPolicy::AllowAll {
            reason: "test".to_owned(),
        };
        let surface = Arc::new(reg.effective_surface(SurfaceInputs {
            policy: &policy,
            allowlist: Some(&allowlist),
            active: &active,
            server_tools: &[],
            server_tool_config: None,
        }));
        let _binding = ctx.bind_effective_surface(surface);

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_3".to_owned(),
            arguments: serde_json::json!({ "tool_name": "read" }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");

        assert!(
            !result.is_error,
            "structured unavailable response should not be a tool execution error"
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&result.content.text_summary()).expect("valid JSON");
        assert_eq!(parsed.get("available"), Some(&serde_json::json!(false)));
        assert_eq!(parsed.get("reason"), Some(&serde_json::json!("allowlist")));
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

    // ── late registration ────────────────────────────────────────────────────

    struct LateTool;

    impl ToolExecutor for LateTool {
        fn execute<'a>(
            &'a self,
            _input: &'a ToolInput,
            _ctx: &'a ToolContext,
        ) -> Pin<Box<dyn Future<Output = crate::error::Result<ToolResult>> + Send + 'a>> {
            Box::pin(async { Ok(ToolResult::text("late tool executed")) })
        }
    }

    fn late_tool_def(name: &str, property: &str) -> ToolDef {
        let mut properties = IndexMap::new();
        properties.insert(
            property.to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: format!("Example property for {name}"),
                enum_values: None,
                default: None,
                ..Default::default()
            },
        );

        ToolDef {
            name: ToolName::new(name).expect("valid late tool name"),
            description: format!("Late-registered tool: {name}"),
            extended_description: None,
            input_schema: InputSchema {
                properties,
                required: vec![property.to_owned()],
            },
            category: ToolCategory::Research,
            reversibility: Reversibility::FullyReversible,
            auto_activate: false,
            groups: vec![ToolGroupId::Read],
            tags: vec![ToolTag::Fetch],
        }
    }

    #[tokio::test]
    async fn tool_schema_returns_error_for_late_tool_before_finalize() {
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register builtins");
        reg.register(late_tool_def("late_tool", "value"), Box::new(LateTool))
            .expect("register late tool");

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_late_before".to_owned(),
            arguments: serde_json::json!({ "tool_name": "late_tool" }),
        };
        let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
        assert!(
            result.is_error,
            "late-registered tool must not be visible before finalize"
        );
    }

    #[tokio::test]
    async fn tool_schema_returns_schema_for_late_tool_after_finalize() {
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register builtins");
        reg.register(late_tool_def("late_pack_tool", "query"), Box::new(LateTool))
            .expect("register late tool");
        reg.finalize_tool_schema()
            .expect("finalize tool_schema after late registration");

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_late_after".to_owned(),
            arguments: serde_json::json!({ "tool_name": "late_pack_tool" }),
        };
        let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
        assert!(
            !result.is_error,
            "expected success for late tool after finalize"
        );

        let text = result.content.text_summary();
        assert!(
            text.contains("query"),
            "expected schema to contain 'query' property, got: {text}"
        );

        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(
            parsed.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "schema must have type=object at root"
        );
    }

    #[tokio::test]
    async fn tool_schema_still_resolves_builtin_after_finalize() {
        let mut reg = ToolRegistry::new();
        register_all_with_sandbox(&mut reg, SandboxConfig::default()).expect("register builtins");
        reg.register(
            late_tool_def("late_http_tool", "endpoint"),
            Box::new(LateTool),
        )
        .expect("register late tool");
        reg.finalize_tool_schema()
            .expect("finalize tool_schema after late registration");

        let input = ToolInput {
            name: ToolName::from_static("tool_schema"),
            tool_use_id: "toolu_builtin_after".to_owned(),
            arguments: serde_json::json!({ "tool_name": "read" }),
        };
        let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
        assert!(!result.is_error, "built-in tool must still resolve");
        assert!(
            result.content.text_summary().contains("path"),
            "expected read schema to contain 'path'"
        );
    }
}
