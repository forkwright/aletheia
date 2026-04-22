//! MCP tool: `architecture_fact` — query structured architecture facts.
//!
//! Research agents call this tool **before** synthesising a plan to verify
//! architectural premises cheaply, instead of re-reading the codebase.
//!
//! # Operations
//!
//! | `op`     | Required args | Optional args | Effect                            |
//! |----------|---------------|---------------|-----------------------------------|
//! | `get`    | `id`          | —             | Return the fact with exact `id`.  |
//! | `put`    | `fact`        | —             | Write/overwrite a fact.           |
//! | `list`   | —             | `scope`       | List all facts (optionally filtered). |
//! | `search` | `query`       | —             | Substring search across id/scope/claim. |
//!
//! # Storage
//!
//! Delegates to [`eidos::knowledge::architecture_fact::FactStore`] backed by
//! flat JSON files under `ALETHEIA_FACTS_DIR` env var or
//! `~/aletheia/instance/facts/` by default.
//!
//! # Producer discipline
//!
//! When calling `put`, the caller must populate `updated_by` with the PR
//! number (`PR-<N>`) or session key for the current session.  This provides
//! the lightweight audit trail required by the standards.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use eidos::knowledge::architecture_fact::{ArchitectureFact, FactScope, FactStore};
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve the `FactStore` base directory from env or default.
fn fact_store() -> FactStore {
    match std::env::var("ALETHEIA_FACTS_DIR") {
        Ok(dir) if !dir.is_empty() => FactStore::new(dir),
        _ => FactStore::new(FactStore::default_path()),
    }
}

/// Parse a `scope` string into a [`FactScope`].
fn parse_scope(s: &str) -> Option<FactScope> {
    match s {
        "crate" => Some(FactScope::Crate),
        "module" => Some(FactScope::Module),
        "concept" => Some(FactScope::Concept),
        "boundary" => Some(FactScope::Boundary),
        _ => None,
    }
}

// ── Executor ─────────────────────────────────────────────────────────────────

struct ArchitectureFactExecutor;

impl ToolExecutor for ArchitectureFactExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(op) = input.arguments.get("op").and_then(|v| v.as_str()) else {
                return Ok(ToolResult::error("missing required field: op"));
            };

            match op {
                "get" => op_get(&input.arguments).await,
                "put" => op_put(&input.arguments).await,
                "list" => op_list(&input.arguments).await,
                "search" => op_search(&input.arguments).await,
                other => Ok(ToolResult::error(format!(
                    "unknown op '{other}': expected one of get, put, list, search"
                ))),
            }
        })
    }
}

// ── op: get ──────────────────────────────────────────────────────────────────

async fn op_get(args: &serde_json::Value) -> Result<ToolResult> {
    let Some(id) = args.get("id").and_then(|v| v.as_str()) else {
        return Ok(ToolResult::error("op=get requires 'id'"));
    };
    let store = fact_store();
    match store.get(id).await {
        Ok(Some(fact)) => {
            let json = match serde_json::to_string_pretty(&fact) {
                Ok(j) => j,
                Err(e) => return Ok(ToolResult::error(format!("serialise error: {e}"))),
            };
            Ok(ToolResult::text(json))
        }
        Ok(None) => Ok(ToolResult::text(format!("no fact found with id '{id}'"))),
        Err(e) => Ok(ToolResult::error(e.to_string())),
    }
}

// ── op: put ──────────────────────────────────────────────────────────────────

async fn op_put(args: &serde_json::Value) -> Result<ToolResult> {
    let Some(fact_val) = args.get("fact") else {
        return Ok(ToolResult::error("op=put requires 'fact'"));
    };
    let fact: ArchitectureFact = match serde_json::from_value(fact_val.clone()) {
        Ok(f) => f,
        Err(e) => return Ok(ToolResult::error(format!("invalid fact: {e}"))),
    };
    if fact.updated_by.is_empty() {
        return Ok(ToolResult::error(
            "fact.updated_by must be set to a PR number (PR-<N>) or session key",
        ));
    }
    let id = fact.id.clone();
    let store = fact_store();
    match store.put(fact).await {
        Ok(()) => Ok(ToolResult::text(format!("fact '{id}' written"))),
        Err(e) => Ok(ToolResult::error(e.to_string())),
    }
}

// ── op: list ─────────────────────────────────────────────────────────────────

async fn op_list(args: &serde_json::Value) -> Result<ToolResult> {
    let scope = args
        .get("scope")
        .and_then(|v| v.as_str())
        .and_then(parse_scope);

    if let Some(raw) = args.get("scope").and_then(|v| v.as_str())
        && scope.is_none()
    {
        return Ok(ToolResult::error(format!(
            "unknown scope '{raw}': expected one of crate, module, concept, boundary",
        )));
    }

    let store = fact_store();
    match store.list(scope).await {
        Ok(facts) => {
            if facts.is_empty() {
                return Ok(ToolResult::text("no facts found".to_owned()));
            }
            let lines: Vec<String> = facts
                .iter()
                .map(|f| format!("[{}] {} — {}", f.scope, f.id, f.claim))
                .collect();
            Ok(ToolResult::text(format!(
                "{} fact(s):\n\n{}",
                facts.len(),
                lines.join("\n")
            )))
        }
        Err(e) => Ok(ToolResult::error(e.to_string())),
    }
}

// ── op: search ───────────────────────────────────────────────────────────────

async fn op_search(args: &serde_json::Value) -> Result<ToolResult> {
    let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
        return Ok(ToolResult::error("op=search requires 'query'"));
    };
    let store = fact_store();
    match store.search(query).await {
        Ok(facts) => {
            if facts.is_empty() {
                return Ok(ToolResult::text(format!("no facts match '{query}'")));
            }
            let lines: Vec<String> = facts
                .iter()
                .map(|f| format!("[{}] {} — {}", f.scope, f.id, f.claim))
                .collect();
            Ok(ToolResult::text(format!(
                "{} fact(s) matching '{query}':\n\n{}",
                facts.len(),
                lines.join("\n")
            )))
        }
        Err(e) => Ok(ToolResult::error(e.to_string())),
    }
}

// ── ToolDef ──────────────────────────────────────────────────────────────────

fn architecture_fact_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("architecture_fact"), // kanon:ignore RUST/expect
        description:
            "Query or write structured architecture facts about the aletheia codebase. \
             Call this before synthesising a plan to verify premises without reading source. \
             ops: get (by id), put (write/overwrite), list (optionally filtered by scope), \
             search (substring across id/scope/claim)."
                .to_owned(),
        extended_description: Some(
            "Architecture facts are short, cited, versioned claims about architectural \
             seams: spawn model, storage invariants, hook taxonomy, lifecycle boundaries, \
             crate ownership. Each fact carries evidence (file paths) and a producer \
             (PR number or session key). Fact ids use dot-separated hierarchical names \
             such as `aletheia.spawn.model`."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "op".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Operation: get | put | list | search".to_owned(),
                        enum_values: Some(vec![
                            "get".to_owned(),
                            "put".to_owned(),
                            "list".to_owned(),
                            "search".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Fact id for op=get (dot-separated, e.g. aletheia.spawn.model)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Filter by scope for op=list: crate | module | concept | boundary"
                                .to_owned(),
                        enum_values: Some(vec![
                            "crate".to_owned(),
                            "module".to_owned(),
                            "concept".to_owned(),
                            "boundary".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "query".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Substring query for op=search (case-insensitive, matches id/scope/claim)"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "fact".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Object,
                        description: "ArchitectureFact object for op=put. \
                             Required fields: id, scope, claim, evidence, updated_by. \
                             updated_by must be PR-<N> or session key."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["op".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::Reversible,
        auto_activate: true,
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `architecture_fact` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(architecture_fact_def(), Box::new(ArchitectureFactExecutor))?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use crate::testing::install_crypto_provider;
    use crate::types::{ServerToolConfig, ToolContext, ToolInput, ToolServices};

    use super::*;

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

    /// Point the store at a fresh temp dir for each test via env var.
    fn setup_temp_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        // WHY: The executor reads ALETHEIA_FACTS_DIR at call time, so we
        // set it before each test invocation.  Tests are not run in parallel
        // within this module so the env var is safe to mutate.
        #[expect(unsafe_code, reason = "test-only env mutation; tests run sequentially")]
        // SAFETY: single-threaded tokio test context; no concurrent env readers.
        unsafe {
            std::env::set_var("ALETHEIA_FACTS_DIR", dir.path());
        };
        dir
    }

    #[tokio::test]
    async fn op_put_then_get_roundtrip() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        // Put
        let put_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_put".to_owned(),
            arguments: serde_json::json!({
                "op": "put",
                "fact": {
                    "id": "test.mcp.put",
                    "scope": "concept",
                    "claim": "Test claim via MCP.",
                    "evidence": ["src/lib.rs:1"],
                    "updated_at": "2026-04-22T00:00:00Z",
                    "updated_by": "PR-3789"
                }
            }),
        };
        let result = executor
            .execute(&put_input, &ctx)
            .await
            .expect("execute put");
        assert!(!result.is_error, "put should succeed");
        assert!(result.content.text_summary().contains("written"));

        // Get
        let get_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_get".to_owned(),
            arguments: serde_json::json!({ "op": "get", "id": "test.mcp.put" }),
        };
        let result = executor
            .execute(&get_input, &ctx)
            .await
            .expect("execute get");
        assert!(!result.is_error, "get should succeed");
        assert!(
            result
                .content
                .text_summary()
                .contains("Test claim via MCP.")
        );
    }

    #[tokio::test]
    async fn op_list_all() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        // Write two facts
        for i in 0..2u32 {
            let put_input = ToolInput {
                name: ToolName::from_static("architecture_fact"),
                tool_use_id: format!("toolu_put_{i}"),
                arguments: serde_json::json!({
                    "op": "put",
                    "fact": {
                        "id": format!("test.list.{i}"),
                        "scope": "crate",
                        "claim": format!("Claim {i}."),
                        "evidence": [],
                        "updated_at": "2026-04-22T00:00:00Z",
                        "updated_by": "PR-3789"
                    }
                }),
            };
            executor.execute(&put_input, &ctx).await.expect("put");
        }

        let list_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_list".to_owned(),
            arguments: serde_json::json!({ "op": "list" }),
        };
        let result = executor
            .execute(&list_input, &ctx)
            .await
            .expect("execute list");
        assert!(!result.is_error, "list should succeed");
        let text = result.content.text_summary();
        assert!(text.contains("2 fact(s)"), "expected 2 facts, got: {text}");
    }

    #[tokio::test]
    async fn op_search_returns_matching() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        let put_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_put_search".to_owned(),
            arguments: serde_json::json!({
                "op": "put",
                "fact": {
                    "id": "test.search.mcp",
                    "scope": "concept",
                    "claim": "Agents use Tokio runtime.",
                    "evidence": [],
                    "updated_at": "2026-04-22T00:00:00Z",
                    "updated_by": "PR-3789"
                }
            }),
        };
        executor.execute(&put_input, &ctx).await.expect("put");

        let search_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_search".to_owned(),
            arguments: serde_json::json!({ "op": "search", "query": "tokio" }),
        };
        let result = executor
            .execute(&search_input, &ctx)
            .await
            .expect("execute search");
        assert!(!result.is_error, "search should succeed");
        assert!(
            result
                .content
                .text_summary()
                .contains("Agents use Tokio runtime."),
            "expected search to return matching fact"
        );
    }

    #[tokio::test]
    async fn op_get_missing_returns_not_found_message() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        let get_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_get_miss".to_owned(),
            arguments: serde_json::json!({ "op": "get", "id": "does.not.exist" }),
        };
        let result = executor
            .execute(&get_input, &ctx)
            .await
            .expect("execute get");
        assert!(!result.is_error, "missing get should be non-error");
        assert!(
            result.content.text_summary().contains("no fact found"),
            "expected not-found message"
        );
    }

    #[tokio::test]
    async fn op_put_missing_updated_by_is_error() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        let put_input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_put_nobby".to_owned(),
            arguments: serde_json::json!({
                "op": "put",
                "fact": {
                    "id": "test.no-updated-by",
                    "scope": "crate",
                    "claim": "Some claim.",
                    "evidence": [],
                    "updated_at": "2026-04-22T00:00:00Z",
                    "updated_by": ""
                }
            }),
        };
        let result = executor
            .execute(&put_input, &ctx)
            .await
            .expect("execute put");
        assert!(result.is_error, "empty updated_by should be an error");
    }

    #[tokio::test]
    async fn op_unknown_is_error() {
        let _dir = setup_temp_dir();
        let ctx = mock_ctx();
        let executor = ArchitectureFactExecutor;

        let input = ToolInput {
            name: ToolName::from_static("architecture_fact"),
            tool_use_id: "toolu_bad_op".to_owned(),
            arguments: serde_json::json!({ "op": "frobnicate" }),
        };
        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error, "unknown op should be an error");
    }

    #[test]
    fn tool_def_is_auto_activate_and_research_category() {
        let def = architecture_fact_def();
        assert!(def.auto_activate, "expected auto_activate = true");
        assert_eq!(def.category, ToolCategory::Research);
    }
}
