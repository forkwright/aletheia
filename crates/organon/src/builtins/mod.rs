//! Built-in tool executors and stubs.

/// Agent coordination tools (spawn, dispatch).
pub mod agent;
/// Architecture-fact query/write tool (architecture_fact).
pub mod architecture_fact;
/// Bookkeeper tools (prompt archival and worktree cleanup).
#[cfg(feature = "energeia")]
pub mod bookkeeper;
/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Computer use: screen capture, action dispatch, sandboxed execution.
#[cfg(feature = "computer-use")]
pub mod computer_use;
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
/// Scaffold report tool: generates a new report project from embedded templates.
pub mod scaffold_report;
/// DOCX report rendering tool (render_docx_report).
pub mod render_docx_report;
/// Render a JSON slide descriptor to PPTX.
pub mod render_pptx_report;
/// Web research tools (web_fetch).
pub mod research;
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
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
pub fn register_all_with_sandbox(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
) -> Result<()> {
    // ── Phase 1: register all domain tools ───────────────────────────────────
    register_domain_tools(registry, sandbox)?;

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
) -> Result<()> {
    #[cfg(feature = "computer-use")]
    computer_use::register(registry, &sandbox)?;

    workspace::register(registry, sandbox)?;
    memory::register(registry)?;
    communication::register(registry)?;
    filesystem::register(registry)?;
    fs_ops::register(registry)?;
    git_ops::register(registry)?;
    http_client::register(registry)?;
    view_file::register(registry)?;
    agent::register(registry)?;
    enable_tool::register(registry)?;
    planning::register(registry)?;
    research::register(registry)?;
    architecture_fact::register(registry)?;
    #[cfg(feature = "z3")]
    z3_solver::register(registry)?;
    web_search::register(registry)?;
    triage::register(registry)?;
    parameters::register(registry)?;
    #[cfg(feature = "energeia")]
    // WHY: No EnergeiaServices provided at this registration level — callers that
    // have services configured should use energeia::register(registry, Some(services)).
    // Tools requiring services return structured errors rather than panicking.
    energeia::register(registry, None)?;
    #[cfg(feature = "energeia")]
    bookkeeper::register(registry)?;
    poiesis::register(registry)?;
    intake_report::register(registry)?;
    scaffold_report::register(registry)?;
    render_docx_report::register(registry)?;
    render_pptx_report::register(registry)?;
    Ok(())
}
