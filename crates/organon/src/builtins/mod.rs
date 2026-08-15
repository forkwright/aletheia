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
#[cfg(feature = "poiesis")]
pub mod diff_report;
/// Dynamic tool activation meta-tool.
pub mod enable_tool;
/// Energeia capability tools (dromeus, dokimasia, diorthosis, epitropos, parateresis,
/// mathesis, prographe, schedion, metron). Wired to real energeia subsystems.
#[cfg(feature = "energeia")]
pub mod energeia;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Shared protected-path policy for filesystem mutation tools.
pub(crate) mod filesystem_policy;
/// Filesystem mutation tools (mkdir, mv, cp, rm).
pub mod fs_ops;
/// Git read-only and non-destructive operations (status, log, diff, branch, checkout).
pub mod git_ops;
/// Generic HTTP client (POST/PUT/DELETE/PATCH with headers + body).
pub mod http_client;
/// Inspect report tool: extract text from documents.
#[cfg(feature = "poiesis")]
pub mod inspect_report;
/// Intake report tool: parse Slack-style text into a structured scaffold.
#[cfg(feature = "poiesis")]
pub mod intake_report;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// Parameter registry query tool (discover tunable parameters).
pub mod parameters;
/// Planning project management tools (create, status, execute, verify).
pub mod planning;
/// Poiesis report tools: generate_document, lint_report, verify_report,
/// render_typst_report, render_docx_report.
#[cfg(feature = "poiesis")]
pub mod poiesis;
/// DOCX report rendering tool (render_docx_report).
#[cfg(feature = "poiesis")]
pub mod render_docx_report;
/// Render a JSON eval report to PDF (render_eval_report).
#[cfg(feature = "poiesis")]
pub mod render_eval_report;
/// Render a JSON graph audit to PDF (render_graph_audit).
#[cfg(feature = "poiesis")]
pub mod render_graph_audit;
/// Render a JSON slide descriptor to PPTX.
#[cfg(feature = "poiesis")]
pub mod render_pptx_report;
/// JSON-first XLSX report tool (`render_xlsx_report`).
#[cfg(feature = "poiesis")]
pub mod render_xlsx_report;
/// Report runtime dependency doctor (Pandoc, LaTeX, Chromium, Typst).
#[cfg(feature = "poiesis")]
pub mod report_runtime_health;
/// Web research tools (web_fetch).
pub mod research;
/// Scaffold report tool: generates a new report project from embedded templates.
#[cfg(feature = "poiesis")]
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
use crate::sandbox::{SandboxConfig, SandboxConfigExt as _};

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
    // SECURITY(#5081, #5064, #5232, #4997): surface misleading sandbox
    // guarantees once at startup rather than leaving them discoverable only
    // by reading source or scattered per-invocation log lines. A guarantee
    // that is `broken_under_enforcing` (currently: `egress = "allowlist"`
    // with non-loopback entries) is not merely weaker than advertised --
    // under `enforcement = "enforcing"` it is not enforceable at all, so
    // registration is refused rather than starting up on a promise the
    // sandbox can never keep. Under `enforcement = "permissive"` the same
    // condition is logged only, matching every other guarantee's documented
    // "logged but not blocked" behavior.
    let enforcing = sandbox.enforcement == crate::sandbox::SandboxEnforcement::Enforcing;
    for issue in sandbox.validate() {
        if issue.broken_under_enforcing && enforcing {
            tracing::error!(
                message = %issue.message,
                "sandbox configuration guarantee is not enforceable; refusing to register tools"
            );
            return crate::error::SandboxConfigUnenforceableSnafu {
                message: issue.message,
            }
            .fail();
        }
        tracing::warn!(
            message = %issue.message,
            "sandbox configuration guarantee is weaker than it may appear"
        );
    }

    #[cfg(feature = "computer-use")]
    computer_use::register(registry, &sandbox)?;

    workspace::register(registry, sandbox.clone())?;
    memory::register(registry)?;
    communication::register(registry)?;
    filesystem::register_with_sandbox(registry, sandbox.clone())?;
    fs_ops::register(registry)?;
    http_client::register(registry, &sandbox)?;
    view_file::register(registry)?;
    agent::register(registry)?;
    enable_tool::register(registry)?;
    planning::register(registry)?;
    research::register(registry, &sandbox)?;
    architecture_fact::register(registry)?;
    code_graph_query::register(registry)?;
    #[cfg(feature = "z3")]
    z3_solver::register(registry)?;
    web_search::register(registry, &sandbox)?;
    // WHY here, and moved rather than cloned: every registrar above either borrows or takes
    // its own copy, so this is the final owner of `sandbox`. Consuming it is what makes the
    // by-value parameter honest — clippy::needless_pass_by_value fires on a value the body
    // never actually takes, and cloning into the last use is the shape that triggers it.
    // Registration order is immaterial: each call inserts under its own distinct tool name.
    git_ops::register_with_sandbox(registry, sandbox)?;
    triage::register(registry)?;
    parameters::register(registry)?;
    #[cfg(feature = "energeia")]
    // WHY: generic registration still supports service-less schemas for tests
    // and tools that do not own the runtime; Aletheia injects real services.
    // Tools requiring services return structured errors rather than panicking.
    energeia::register(registry, energeia_services)?;
    #[cfg(feature = "bookkeeper")]
    bookkeeper::register(registry)?;
    #[cfg(feature = "poiesis")]
    poiesis::register(registry)?;
    #[cfg(feature = "poiesis")]
    report_runtime_health::register(registry)?;
    #[cfg(feature = "poiesis")]
    intake_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    scaffold_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    render_docx_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    render_pptx_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    render_xlsx_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    render_eval_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    render_graph_audit::register(registry)?;
    skill_read::register(registry)?;
    working_checkpoint::register(registry)?;
    #[cfg(feature = "poiesis")]
    diff_report::register(registry)?;
    #[cfg(feature = "poiesis")]
    inspect_report::register(registry)?;
    Ok(())
}

#[cfg(all(test, feature = "energeia", not(feature = "bookkeeper")))]
#[expect(clippy::expect_used, reason = "test assertions")]
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

    #[test]
    fn enforcing_sandbox_rejects_unenforceable_egress_allowlist() {
        // SECURITY(#4997): regression test. Before this fix, an `egress =
        // "allowlist"` config with non-loopback entries under
        // `enforcement = "enforcing"` was only logged (tracing::error!) and
        // registration continued — every subsequent subprocess spawn then
        // ran under a policy that silently behaved as `egress = "deny"`
        // while still being configured and reported as "allowlist". This
        // must now be refused outright rather than start up on a guarantee
        // the sandbox can never keep.
        let mut registry = ToolRegistry::new();
        let sandbox = SandboxConfig {
            enforcement: crate::sandbox::SandboxEnforcement::Enforcing,
            egress: crate::sandbox::EgressPolicy::Allowlist,
            egress_allowlist: vec!["93.184.216.34".to_owned()],
            ..SandboxConfig::default()
        };
        let err = register_domain_tools(&mut registry, sandbox, None)
            .expect_err("an unenforceable allowlist under enforcement=enforcing must be refused");
        assert!(
            matches!(err, crate::error::Error::SandboxConfigUnenforceable { .. }),
            "must fail with the dedicated sandbox-config error variant: {err:?}"
        );
        assert!(
            err.to_string().contains("allowlist"),
            "error should name the unenforceable control: {err}"
        );
    }

    #[test]
    fn permissive_sandbox_still_registers_with_unenforceable_egress_allowlist() {
        // WHY: enforcement=permissive's documented contract across every
        // other guarantee (Landlock, seccomp, allowed_root) is "logged but
        // not blocked"; the allowlist-specific rejection above must not
        // widen that contract into a hard failure under permissive.
        let mut registry = ToolRegistry::new();
        let sandbox = SandboxConfig {
            enforcement: crate::sandbox::SandboxEnforcement::Permissive,
            egress: crate::sandbox::EgressPolicy::Allowlist,
            egress_allowlist: vec!["93.184.216.34".to_owned()],
            ..SandboxConfig::default()
        };
        register_domain_tools(&mut registry, sandbox, None)
            .expect("permissive enforcement must still register tools, only warn");
    }

    #[test]
    fn enforcing_sandbox_registers_with_loopback_only_egress_allowlist() {
        // WHY: a loopback-only allowlist IS within what the child-process
        // network-namespace mechanism can provide (see
        // `allowlist_is_loopback_only`), so it must not be rejected.
        let mut registry = ToolRegistry::new();
        let sandbox = SandboxConfig {
            enforcement: crate::sandbox::SandboxEnforcement::Enforcing,
            egress: crate::sandbox::EgressPolicy::Allowlist,
            egress_allowlist: vec!["127.0.0.1".to_owned()],
            ..SandboxConfig::default()
        };
        register_domain_tools(&mut registry, sandbox, None)
            .expect("a loopback-only allowlist is enforceable and must not be refused");
    }
}

// WHY(#4559): 13 poiesis-* crates were unconditional dependencies with
// unconditional registration; this pins that a plain build (poiesis off,
// organon's default) excludes the whole document/report-tool family, and
// still registers everything else.
#[cfg(all(test, not(feature = "poiesis")))]
mod poiesis_default_off_tests {
    use super::*;

    #[test]
    fn default_registry_excludes_poiesis_family_tools() -> Result<()> {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry)?;
        let names: Vec<&str> = registry
            .definitions()
            .iter()
            .map(|def| def.name.as_str())
            .collect();

        let poiesis_tool_names = [
            "generate_document",
            "lint_report",
            "verify_report",
            "render_typst_report",
            "qa_gate",
            "render_docx_report",
            "render_pptx_report",
            "render_xlsx_report",
            "render_eval_report",
            "render_graph_audit",
            "report_runtime_health",
            "scaffold_report",
            "intake_report",
            "diff_report",
            "inspect_report",
        ];
        for tool_name in poiesis_tool_names {
            assert!(
                !names.contains(&tool_name),
                "poiesis-family tool {tool_name} must not be exposed when the poiesis feature is off"
            );
        }
        assert!(
            names.contains(&"grep") || names.contains(&"read_file"),
            "non-poiesis tools must still register when the poiesis feature is off"
        );
        Ok(())
    }
}

/// ARCHITECTURE(#4543): the gate the issue's acceptance criteria ask for --
/// "add a check or review rule that new public tools must declare
/// capability/governance metadata." Unconditionally compiled (unlike the
/// two feature-narrow test modules above) so it runs under whatever
/// feature combination is active, always covering at minimum the always-on
/// tools and, when the relevant `--features` flags are set, the
/// feature-gated ones too.
///
/// Scoped to `Reversibility::Irreversible` tools rather than every
/// registered tool: that is the side-effect class where "was rollback ever
/// reviewed" matters most, and it is the bounded, currently-classified set
/// (see the `declare_capability` calls alongside each of these tools'
/// `register()` call). A tool newly marked `Irreversible` without also
/// calling `declare_capability` fails this test with its name, not a
/// silent gap.
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod capability_governance_tests {
    use super::*;
    use crate::types::{Reversibility, UNASSIGNED_TOOL_OWNER};

    #[test]
    fn all_irreversible_tools_declare_capability_metadata() -> Result<()> {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry)?;

        let undeclared: Vec<&str> = registry
            .definitions()
            .iter()
            .filter(|def| def.reversibility == Reversibility::Irreversible)
            .map(|def| def.name.as_str())
            .filter(|name| {
                let name = koina::id::ToolName::new(name).expect("registered name is valid");
                registry.capability_metadata(&name).owner == UNASSIGNED_TOOL_OWNER
            })
            .collect();

        assert!(
            undeclared.is_empty(),
            "every Irreversible tool must call ToolRegistry::declare_capability \
             (see the `register()` function for each tool's module); missing: {undeclared:?}"
        );
        Ok(())
    }
}
