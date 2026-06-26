//! MCP tool: `code_graph_query` — symbol-level cross-crate queries.
//!
//! Agents call this tool to answer structural questions about the aletheia
//! workspace that grep and `architecture_fact` cannot answer: which crates use
//! a symbol, which types implement a trait, which crates re-export a name.
//!
//! # Relationship to `architecture_fact`
//!
//! | Layer                | Tier       | Kind                    | Use                            |
//! |----------------------|------------|-------------------------|--------------------------------|
//! | `architecture_fact`  | Verified   | Human-curated claims    | "What does the design say?"    |
//! | `code_graph_query`   | Inferred   | Machine-derived symbols | "What does the code actually do?" |
//!
//! These are complementary surfaces.  Do not conflate them.  A fact like
//! `aletheia.eidos.dependency-direction` can be *cross-checked* by querying
//! `crate_rdeps(eidos)` — if that returns internal aletheia crates as
//! dependents of eidos, something has changed.
//!
//! # Operations
//!
//! | `op`             | Required arg(s)       | Optional arg(s) | Effect                                   |
//! |------------------|-----------------------|-----------------|------------------------------------------|
//! | `symbol_rdeps`   | `symbol`              | `target_crate`  | Symbols that reference `symbol`.         |
//! | `impl_search`    | `trait_name`          | —               | Types that implement `trait_name`.        |
//! | `reexport_chain` | `symbol`              | —               | Crates that `pub use` `symbol`.          |
//! | `crate_deps`     | `crate_name`          | —               | Direct workspace deps of `crate_name`.   |
//! | `crate_rdeps`    | `crate_name`          | —               | Workspace crates that depend on `crate_name`. |
//! | `symbols_in`     | `crate_name`          | `kind`          | All symbols in `crate_name`.             |
//! | `rebuild`        | —                     | `workspace`     | Rebuild the index (incremental).         |
//!
//! # Cache
//!
//! The index lives at `~/.cache/aletheia/gnosis.fjall` by default.
//! Override with `GNOSIS_CACHE_PATH`.  Delete the directory to force a full rebuild.
//!
//! # Workspace path
//!
//! Defaults to the directory containing the running binary's workspace
//! (`GNOSIS_WORKSPACE_ROOT` env var, or the process `cwd`).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use indexmap::IndexMap;

use gnosis::CodeGraph;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve the workspace root from env or `cwd`.
fn workspace_root(override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("GNOSIS_WORKSPACE_ROOT")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Open (or re-open) a `CodeGraph` handle.
///
/// Returns `Err` as a `ToolResult::error` string on failure.
fn open_graph(workspace: Option<&str>) -> std::result::Result<CodeGraph, String> {
    let root = workspace_root(workspace);
    CodeGraph::open_default(&root).map_err(|e| format!("gnosis: failed to open index: {e}"))
}

// ── Executor ─────────────────────────────────────────────────────────────────

struct CodeGraphQueryExecutor;

impl ToolExecutor for CodeGraphQueryExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let args = &input.arguments;
            let Some(op) = args.get("op").and_then(|v| v.as_str()) else {
                return Ok(ToolResult::error("missing required field: op"));
            };

            Ok(match op {
                "symbol_rdeps" => op_symbol_rdeps(args),
                "impl_search" => op_impl_search(args),
                "reexport_chain" => op_reexport_chain(args),
                "crate_deps" => op_crate_deps(args),
                "crate_rdeps" => op_crate_rdeps(args),
                "symbols_in" => op_symbols_in(args),
                "rebuild" => op_rebuild(args),
                other => ToolResult::error(format!(
                    "unknown op '{other}': expected one of \
                     symbol_rdeps, impl_search, reexport_chain, \
                     crate_deps, crate_rdeps, symbols_in, rebuild"
                )),
            })
        })
    }
}

// ── op: symbol_rdeps ─────────────────────────────────────────────────────────

fn op_symbol_rdeps(args: &serde_json::Value) -> ToolResult {
    let Some(symbol) = args.get("symbol").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=symbol_rdeps requires 'symbol'");
    };
    let target_crate = args.get("target_crate").and_then(|v| v.as_str());
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.symbol_rdeps(symbol, target_crate) {
        Ok(rows) => rows_to_result(&rows, &format!("symbol_rdeps({symbol})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: impl_search ──────────────────────────────────────────────────────────

fn op_impl_search(args: &serde_json::Value) -> ToolResult {
    let Some(trait_name) = args.get("trait_name").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=impl_search requires 'trait_name'");
    };
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.impl_search(trait_name) {
        Ok(rows) => rows_to_result(&rows, &format!("impl_search({trait_name})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: reexport_chain ───────────────────────────────────────────────────────

fn op_reexport_chain(args: &serde_json::Value) -> ToolResult {
    let Some(symbol) = args.get("symbol").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=reexport_chain requires 'symbol'");
    };
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.reexport_chain(symbol) {
        Ok(rows) => rows_to_result(&rows, &format!("reexport_chain({symbol})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: crate_deps ───────────────────────────────────────────────────────────

fn op_crate_deps(args: &serde_json::Value) -> ToolResult {
    let Some(crate_name) = args.get("crate_name").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=crate_deps requires 'crate_name'");
    };
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.crate_deps(crate_name) {
        Ok(rows) => rows_to_result(&rows, &format!("crate_deps({crate_name})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: crate_rdeps ──────────────────────────────────────────────────────────

fn op_crate_rdeps(args: &serde_json::Value) -> ToolResult {
    let Some(crate_name) = args.get("crate_name").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=crate_rdeps requires 'crate_name'");
    };
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.crate_rdeps(crate_name) {
        Ok(rows) => rows_to_result(&rows, &format!("crate_rdeps({crate_name})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: symbols_in ───────────────────────────────────────────────────────────

fn op_symbols_in(args: &serde_json::Value) -> ToolResult {
    let Some(crate_name) = args.get("crate_name").and_then(|v| v.as_str()) else {
        return ToolResult::error("op=symbols_in requires 'crate_name'");
    };
    let kind = args.get("kind").and_then(|v| v.as_str());
    let workspace = args.get("workspace").and_then(|v| v.as_str());

    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.symbols_in(crate_name, kind) {
        Ok(rows) => rows_to_result(&rows, &format!("symbols_in({crate_name})")),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── op: rebuild ──────────────────────────────────────────────────────────────

fn op_rebuild(args: &serde_json::Value) -> ToolResult {
    let workspace = args.get("workspace").and_then(|v| v.as_str());
    let graph = match open_graph(workspace) {
        Ok(g) => g,
        Err(e) => return ToolResult::error(e),
    };
    match graph.rebuild() {
        Ok(()) => ToolResult::text("gnosis: index rebuild complete".to_owned()),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

// ── Shared result serialiser ──────────────────────────────────────────────────

fn rows_to_result(rows: &[gnosis::query::QueryRow], label: &str) -> ToolResult {
    if rows.is_empty() {
        return ToolResult::text(format!(
            "{label}: 0 results (index may need rebuild — run op=rebuild first)"
        ));
    }
    match serde_json::to_string_pretty(rows) {
        Ok(json) => ToolResult::text(format!("{label}: {} result(s)\n\n{json}", rows.len())),
        Err(e) => ToolResult::error(format!("serialise error: {e}")),
    }
}

// ── ToolDef ──────────────────────────────────────────────────────────────────

/// Build the `InputSchema` for `code_graph_query`.
fn input_schema() -> InputSchema {
    let str_prop = |desc: &str, enum_values: Option<Vec<String>>| PropertyDef {
        property_type: PropertyType::String,
        description: desc.to_owned(),
        enum_values,
        default: None,
        ..Default::default()
    };
    let ops = vec![
        "symbol_rdeps".to_owned(),
        "impl_search".to_owned(),
        "reexport_chain".to_owned(),
        "crate_deps".to_owned(),
        "crate_rdeps".to_owned(),
        "symbols_in".to_owned(),
        "rebuild".to_owned(),
    ];
    let kinds = vec![
        "fn".to_owned(),
        "struct".to_owned(),
        "enum".to_owned(),
        "trait".to_owned(),
        "type".to_owned(),
        "const".to_owned(),
        "impl".to_owned(),
        "reexport".to_owned(),
    ];
    InputSchema {
        properties: IndexMap::from([
            (
                "op".to_owned(),
                str_prop(
                    "Operation: symbol_rdeps | impl_search | reexport_chain | \
                 crate_deps | crate_rdeps | symbols_in | rebuild",
                    Some(ops),
                ),
            ),
            (
                "symbol".to_owned(),
                str_prop(
                    "Symbol name for symbol_rdeps / reexport_chain (e.g. 'Message', 'dispatch')",
                    None,
                ),
            ),
            (
                "trait_name".to_owned(),
                str_prop(
                    "Trait name for impl_search (e.g. 'Stamped', 'Display')",
                    None,
                ),
            ),
            (
                "crate_name".to_owned(),
                str_prop(
                    "Crate name for crate_deps / crate_rdeps / symbols_in (e.g. 'eidos', 'nous')",
                    None,
                ),
            ),
            (
                "target_crate".to_owned(),
                str_prop(
                    "Optional filter for symbol_rdeps: only return refs where to_crate matches",
                    None,
                ),
            ),
            (
                "kind".to_owned(),
                str_prop(
                    "Optional kind filter for symbols_in: \
                 fn | struct | enum | trait | type | const | impl | reexport",
                    Some(kinds),
                ),
            ),
            (
                "workspace".to_owned(),
                str_prop(
                    "Optional workspace root path override (default: GNOSIS_WORKSPACE_ROOT env or cwd)",
                    None,
                ),
            ),
        ]),
        required: vec!["op".to_owned()],
    }
}

fn code_graph_query_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("code_graph_query"), // kanon:ignore RUST/expect
        description:
            "Machine-derived symbol-level queries over the aletheia workspace code graph. \
             Answers: which crates use a symbol, which types implement a trait, \
             which crates re-export a name. Complements architecture_fact (human-curated). \
             ops: symbol_rdeps, impl_search, reexport_chain, crate_deps, crate_rdeps, \
             symbols_in, rebuild."
                .to_owned(),
        extended_description: Some(
            "Results carry EpistemicTier::Inferred confidence — machine-derived from AST. \
             Every row has a 'source' field ('gnosis@<schema_version>') for provenance. \
             Index is built from cargo metadata + syn AST walk. Run op=rebuild before \
             querying if the index is stale. Cache: ~/.cache/aletheia/gnosis.fjall \
             (override: GNOSIS_CACHE_PATH). Workspace: GNOSIS_WORKSPACE_ROOT or cwd."
                .to_owned(),
        ),
        input_schema: input_schema(),
        category: ToolCategory::Research,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `code_graph_query` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(code_graph_query_def(), Box::new(CodeGraphQueryExecutor))?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn tool_def_is_research_and_auto_activate() {
        let def = code_graph_query_def();
        assert!(def.auto_activate, "expected auto_activate = true");
        assert_eq!(def.category, ToolCategory::Research);
    }

    #[test]
    fn tool_def_required_field_is_op() {
        let def = code_graph_query_def();
        assert_eq!(def.input_schema.required, vec!["op".to_owned()]);
    }

    #[test]
    fn tool_def_reversibility_is_readonly() {
        let def = code_graph_query_def();
        assert_eq!(def.reversibility, Reversibility::FullyReversible);
    }

    #[test]
    fn tool_def_has_all_ops_in_enum_values() {
        let def = code_graph_query_def();
        let op_prop = def.input_schema.properties.get("op").expect("op property");
        let vals = op_prop.enum_values.as_ref().expect("enum_values");
        for op in &[
            "symbol_rdeps",
            "impl_search",
            "reexport_chain",
            "crate_deps",
            "crate_rdeps",
            "symbols_in",
            "rebuild",
        ] {
            assert!(vals.contains(&(*op).to_owned()), "missing op: {op}");
        }
    }

    #[test]
    fn tool_def_documents_fjall_cache_path() {
        let def = code_graph_query_def();
        let desc = def
            .extended_description
            .as_deref()
            .expect("extended description");
        assert!(desc.contains("gnosis.fjall"));
        assert!(!desc.contains("gnosis.sqlite"));
    }
}
