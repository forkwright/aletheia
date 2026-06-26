//! Built-in tool executors and stubs.

/// Agent coordination tools (spawn, dispatch).
pub mod agent;
/// Architecture-fact query/write tool (architecture_fact).
pub mod architecture_fact;
/// Bookkeeper tools (prompt archival and worktree cleanup).
#[cfg(feature = "bookkeeper")]
pub mod bookkeeper;
/// Machine-derived code-graph symbol-level queries (code_graph_query).
pub mod code_graph_query;
/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Computer use: screen capture, action dispatch, sandboxed execution.
#[cfg(feature = "computer-use")]
pub mod computer_use;
/// Diff report tool: compare documents and report changes.
pub mod diff_report;
/// Dynamic tool activation meta-tool.
pub mod enable_tool;
/// Energeia capability tools (dromeus, dokimasia, diorthosis, epitropos, parateresis,
/// mathesis, prographe, schedion, metron). Wired to real energeia subsystems.
#[cfg(feature = "energeia")]
pub mod energeia;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Filesystem mutation tools (mkdir, mv, cp, rm).
pub mod fs_ops;
/// Git read-only and non-destructive operations (status, log, diff, branch, checkout).
pub mod git_ops;
/// Generic HTTP client (POST/PUT/DELETE/PATCH with headers + body).
pub mod http_client;
/// Inspect report tool: extract text from documents.
pub mod inspect_report;
/// Intake report tool: parse Slack-style text into a structured scaffold.
pub mod intake_report;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// Parameter registry query tool (discover tunable parameters).
pub mod parameters;
/// Planning project management tools (create, status, execute, verify).
pub mod planning;
/// Poiesis report tools: generate_document, lint_report, verify_report,
/// render_typst_report, render_docx_report.
pub mod poiesis;
/// DOCX report rendering tool (render_docx_report).
pub mod render_docx_report;
/// Render a JSON eval report to PDF (render_eval_report).
pub mod render_eval_report;
/// Render a JSON graph audit to PDF (render_graph_audit).
pub mod render_graph_audit;
/// Render a JSON slide descriptor to PPTX.
pub mod render_pptx_report;
/// JSON-first XLSX report tool (`render_xlsx_report`).
pub mod render_xlsx_report;
/// Report runtime dependency doctor (Pandoc, LaTeX, Chromium, Typst).
pub mod report_runtime_health;
/// Web research tools (web_fetch).
pub mod research;
/// Scaffold report tool: generates a new report project from embedded templates.
pub mod scaffold_report;
/// Read a lazy-loaded skill by name from the knowledge store (skill_read).
pub mod skill_read;
/// `tool_schema` meta-tool: fetch full JSON schema for any named tool on demand.
///
/// Always compiled (not feature-gated) so the tool is available even when
/// `deferred-schemas` is off.  The `deferred-schemas` feature controls whether
/// callers serialize full schemas or summaries into LLM requests; this tool
/// provides the on-demand schema retrieval path for the deferred case.
pub mod tool_schema;
/// Issue triage tools (scan, score, stage, approve).
pub mod triage;
/// File viewing with multimodal support (images, PDFs, text).
pub mod view_file;
/// Web search via Brave Search API (requires BRAVE_SEARCH_API_KEY).
pub mod web_search;
/// Agent-curated working-memory checkpoint tool (update_working_checkpoint).
pub mod working_checkpoint;
/// File and shell workspace tools (read, write, edit, exec).
pub mod workspace;
/// Z3 SMT solver tool (z3_solver).
#[cfg(feature = "z3")]
pub mod z3_solver;

use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::sandbox::SandboxConfig;

/// Register all built-in tool executors with default sandbox config.
///
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
pub fn register_all(registry: &mut ToolRegistry) -> Result<()> {
    register_all_with_sandbox(registry, SandboxConfig::default())
}

/// Register all built-in tool executors with custom sandbox config.
///
/// Registration is two-phase:
///
/// 1. All domain tools are registered first.
/// 2. `tool_schema` is registered last, capturing a schema snapshot of every
///    tool registered in phase 1.  This avoids a self-referential ownership
///    cycle (the registry owns the `tool_schema` executor, which cannot safely
///    hold a back-reference to the same registry).
///
/// Callers that register additional tools after this function (for example
/// domain packs or external HTTP/MCP tools) should call
/// [`ToolRegistry::finalize_tool_schema`] to refresh the snapshot with the
/// complete tool set.
///
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
pub fn register_all_with_sandbox(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
) -> Result<()> {
    register_all_with_sandbox_inner(
        registry,
        sandbox,
        #[cfg(feature = "energeia")]
        None,
    )
}

/// Register all built-in tool executors with custom sandbox config and
/// service-backed Energeia tools.
///
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
#[cfg(feature = "energeia")]
pub fn register_all_with_sandbox_and_energeia_services(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
    services: &energeia::EnergeiaServices,
) -> Result<()> {
    register_all_with_sandbox_inner(registry, sandbox, Some(services))
}

fn register_all_with_sandbox_inner(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
    #[cfg(feature = "energeia")] energeia_services: Option<&energeia::EnergeiaServices>,
) -> Result<()> {
    // ── Phase 1: register all domain tools ───────────────────────────────────
    register_domain_tools(
        registry,
        sandbox,
        #[cfg(feature = "energeia")]
        energeia_services,
    )?;

    // ── Phase 2: register tool_schema with a snapshot of phase-1 definitions ──
    // WHY: tool_schema must see the complete tool set to serve schemas for
    // every domain tool.  We pass `registry` itself as the "snapshot" source:
    // `tool_schema::register` reads `definitions()` from it (all domain tools
    // are present at this point), pre-serialises the schemas, and stores them
    // inside the executor.  The executor never holds a back-reference to the
    // registry, so there is no ownership cycle.  See `tool_schema::register`
    // for the full rationale.
    //
    // SAFETY of the borrow: `register` takes `(&mut ToolRegistry, &ToolRegistry)`.
    // Rust disallows overlapping `&mut` and `&` on the same value in a single call.
    // We avoid this by first building a `Vec` of `(name, schema_json)` pairs from
    // the immutable view, then passing that Vec to registration.
    let schema_pairs: Vec<(String, String)> = registry
        .definitions()
        .into_iter()
        .filter_map(|def| {
            let schema = def.input_schema.to_json_schema();
            match serde_json::to_string_pretty(&schema) {
                Ok(json) => Some((def.name.as_str().to_owned(), json)),
                Err(e) => {
                    tracing::warn!(
                        tool.name = def.name.as_str(),
                        error = %e,
                        "tool_schema: failed to pre-serialize schema; tool will be unavailable via tool_schema"
                    );
                    None
                }
            }
        })
        .collect();
    tool_schema::register_with_pairs(registry, schema_pairs)?;

    Ok(())
}

/// Register all domain tools into `registry` (no `tool_schema` meta-tool).
///
/// Called by [`register_all_with_sandbox`] as phase 1.  Exposed so callers
/// that need to build a schema snapshot can register domain tools first,
/// snapshot, then add `tool_schema` themselves if needed.
///
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
pub(crate) fn register_domain_tools(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
    #[cfg(feature = "energeia")] energeia_services: Option<&energeia::EnergeiaServices>,
) -> Result<()> {
    #[cfg(feature = "computer-use")]
    computer_use::register(registry, &sandbox)?;

    workspace::register(registry, sandbox.clone())?;
    memory::register(registry)?;
    communication::register(registry)?;
    filesystem::register_with_sandbox(registry, sandbox.clone())?;
    fs_ops::register(registry)?;
    git_ops::register_with_sandbox(registry, sandbox)?;
    http_client::register(registry)?;
    view_file::register(registry)?;
    agent::register(registry)?;
    enable_tool::register(registry)?;
    planning::register(registry)?;
    research::register(registry)?;
    architecture_fact::register(registry)?;
    code_graph_query::register(registry)?;
    #[cfg(feature = "z3")]
    z3_solver::register(registry)?;
    web_search::register(registry)?;
    triage::register(registry)?;
    parameters::register(registry)?;
    #[cfg(feature = "energeia")]
    // WHY: generic registration still supports service-less schemas for tests
    // and tools that do not own the runtime; Aletheia injects real services.
    // Tools requiring services return structured errors rather than panicking.
    energeia::register(registry, energeia_services)?;
    #[cfg(feature = "bookkeeper")]
    bookkeeper::register(registry)?;
    poiesis::register(registry)?;
    report_runtime_health::register(registry)?;
    intake_report::register(registry)?;
    scaffold_report::register(registry)?;
    render_docx_report::register(registry)?;
    render_pptx_report::register(registry)?;
    render_xlsx_report::register(registry)?;
    render_eval_report::register(registry)?;
    render_graph_audit::register(registry)?;
    skill_read::register(registry)?;
    working_checkpoint::register(registry)?;
    diff_report::register(registry)?;
    inspect_report::register(registry)?;
    Ok(())
}

#[cfg(all(test, feature = "energeia", not(feature = "bookkeeper")))]
mod tests {
    use super::*;

    #[test]
    fn default_energeia_registry_excludes_feature_gated_bookkeeper_tools() -> Result<()> {
        let mut registry = ToolRegistry::new();
        register_domain_tools(&mut registry, SandboxConfig::default(), None)?;
        let names: Vec<&str> = registry
            .definitions()
            .iter()
            .map(|def| def.name.as_str())
            .collect();

        assert!(
            names.contains(&"parateresis"),
            "implemented energeia tools should still register"
        );
        assert!(
            !names.contains(&"tamias") && !names.contains(&"katharos"),
            "feature-gated bookkeeper tools must not be exposed by default"
        );
        Ok(())
    }
}
