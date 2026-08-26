---
scope: "crates/organon/"
defers_to: ["../../CLAUDE.md"]
tightens: ["tool registration, schema, tag, and sandbox guidance"]
---

# organon

## At a glance

Tool registry, executors, and sandbox for built-in tools. Depends on koina, hermeneus, taxis, eidos, gnosis, and the poiesis helper crates. Entry point: `src/lib.rs` (ToolRegistry, ToolExecutor).

## Depth

Tool registry, executors, and sandbox. 16K lines. 67 built-in tools.

## Read first

1. `src/registry.rs`: ToolRegistry, ToolExecutor trait (the core abstraction)
2. `src/types.rs`: ToolDef, ToolInput, ToolResult, ToolContext, service traits
3. `src/builtins/mod.rs`: register_all() and module organization
4. `src/sandbox/mod.rs`: Landlock + seccomp + network namespace config
5. `src/process_guard.rs`: RAII subprocess lifecycle (kill-on-drop)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `ToolExecutor` | `registry.rs` | Trait: `async execute(input, ctx) -> Result<ToolResult>` |
| `ToolRegistry` | `registry.rs` | Name-based dispatch with metrics and tracing |
| `ToolDef` | `types.rs` | Tool metadata: name, description, schema, category, tags |
| `ToolContext` | `types.rs` | Per-execution context: nous_id, session_id, workspace, services |
| `ToolServices` | `types.rs` | Service locator: messaging, planning, knowledge, spawn |
| `SandboxConfig` | `sandbox/mod.rs` | Landlock + seccomp + egress policy; a type alias to `taxis::config::SandboxSettings` (single-owned there), extended with organon-side behavior via the `SandboxConfigExt` trait |
| `ProcessGuard` | `process_guard.rs` | RAII child process wrapper, `pub(crate)` (prevents orphans/zombies) |
| `ToolCapabilityMetadata` | `types.rs` | Owner/stability/rollback/redaction governance for one tool, stored per-`ToolName` on `ToolRegistry` (not on `ToolDef`) via `declare_capability`/`capability_metadata` |

## Built-in tools (67)

| Category | Tools |
|----------|-------|
| Workspace | read, write, edit, exec |
| Filesystem (navigation) | grep, find, ls |
| Filesystem (mutation) | mkdir, mv, cp, rm |
| Git | git_status, git_log, git_diff, git_branch, git_checkout |
| View File | view_file |
| Memory | memory_search, memory_correct, memory_retract, memory_forget, memory_audit, note, blackboard, datalog_query |
| Communication | message, sessions_send, sessions_ask |
| Agent | sessions_spawn, sessions_dispatch |
| Enable Tool | enable_tool |
| Planning | plan_create, plan_research, plan_requirements, plan_roadmap, plan_discuss, plan_execute, plan_verify, plan_status, plan_step_complete, plan_step_fail, plan_verify_criteria |
| Research | web_fetch, http_request, web_search |
| Knowledge / metadata | architecture_fact, code_graph_query, parameters, skill_read, working_checkpoint |
| Poiesis reports | generate_document, lint_report, verify_report, render_typst_report, qa_gate, intake_report, scaffold_report, render_docx_report, render_pptx_report, render_xlsx_report, render_eval_report, render_graph_audit, diff_report, inspect_report |
| Triage | issue_scan, issue_triage, issue_approve |
| Computer Use | computer_use (feature-gated: `computer-use`) |

`web_search` requires `BRAVE_SEARCH_API_KEY` at runtime (Brave Search API). `http_request` and `web_search` are lazy (activate via `enable_tool`). Git operations are read-only or non-destructive by design - no commit, push, reset, rebase, or `--force` checkout; destructive Git work still goes through `exec` under operator review.

## Patterns

- **Registration**: `ToolDef` + `impl ToolExecutor` -> `registry.register(def, Box::new(executor))`
- **Activation**: `auto_activate: true` = always available. `false` = requires `enable_tool` to activate.
- **Sandbox**: Linux only. Landlock (filesystem), seccomp (syscalls), network namespace. Permissive default.
- **Path validation**: normalize -> check allowed_roots -> canonicalize -> re-check. Tilde expansion.
- **ProcessGuard**: `kill()` + `wait()` on drop. Call `detach()` if process should outlive guard.

## Recent substrate notes

- `ToolDef` includes typed `tags` and `groups`; query by tags with `definitions_for_tags` for operational selection.
- Tool receipts are HMAC-SHA256 over tool-call/result tuples with a per-session ephemeral key and in-memory ledger.
- `working_checkpoint` is the agent-curated continuity tool; its store trait is part of `ToolServices`.
- File-reference interpolation happens inside tool execution helpers; keep user-facing tool schemas honest about it.
- Tool governance (owner/stability/rollback/redaction) is declared via `ToolRegistry::declare_capability`, called from each tool module's `register()` alongside `registry.register(def, executor)` -- never as fields on `ToolDef` itself, so the ~90 existing `_def()` functions never need a mechanical edit to add a new governance field. Undeclared tools read as the honest default (`owner: "unassigned"`, `RollbackSupport::Unsupported{reason: "undeclared"}`) rather than a fabricated "safe" value. `builtins::capability_governance_tests::all_registered_tools_declare_capability_metadata` gates that every tool `register_all` registers has an explicit declaration, in whatever feature combination the test runs under -- a newly-registered built-in tool without a declaration fails the test with its name; there is no allowlist.
- `Reversibility::supports_dry_run()` is public runtime API (not `#[cfg(test)]`-only); `ToolCallMetadata::dry_run_capable` surfaces it per recorded call, distinct from `dry_run` (whether THIS call was a simulation).
- `RedactionPolicy` on `ToolCapabilityMetadata` is enforced at the nous dispatch boundary (`nous::execute::dispatch`): the policy is applied to the placeholder-form arguments before they reach any surface that leaves the executor loop -- `ToolStart`/`ToolApprovalRequired` stream events, the persisted `ToolCall` record (which session-history persistence, skill evidence, and background extraction all read), and the receipt ledger. `Full` additionally redacts the recorded/streamed result text; the in-turn LLM-facing result block is deliberately not redacted (the model mid-turn needs the real output). Precedence with the secret vault: `{{secret:name}}` substitution (#3569) resolves only the executor-bound copy, then the declared policy applies to the trace copies -- `None` passes the placeholder form through, `Fields`/`Full` redact placeholders like any other value. A `Fields` name that is not in the tool's input schema fails `capability_governance_tests::declared_redaction_fields_exist_in_the_tool_input_schema`; `declare_capability` warns on the same condition for external registrations.
- Capability metadata is deliberately NOT in the agent-facing tool manifest (#6808): the provider wire format (`hermeneus::types::ToolDefinition`) is name/description/input_schema, so governance fields could only ride in `description` at per-turn token cost on every tool, and advertising which tools hide their arguments from the trace would tell the model where operator visibility is weakest. Decision-relevant governance belongs to the operator-facing approval/audit surfaces; an on-demand agent surface (e.g. a `tool_schema` response extension) is the named follow-up if that ever changes (#7004).

## Common tasks

| Task | Where |
|------|-------|
| Add built-in tool | New file in `src/builtins/`, implement ToolExecutor, register in `builtins/mod.rs` |
| Modify sandbox | `src/sandbox/mod.rs` (SandboxConfig) + `aletheia.toml` [sandbox] section |
| Add service trait | `src/types.rs` (new trait) + binary crate provides implementation |
| Add tool category | `src/types.rs` (ToolCategory enum) |
| Add tool tag | `src/types.rs` (ToolTag enum) |
| Tag a tool | Add `tags: vec![ToolTag::...]` to the tool's `_def()` function |
| Declare tool governance | `registry.declare_capability(name, ToolCapabilityMetadata { owner, stability, rollback, .. })` in the tool's `register()`, right after `registry.register(def, executor)`. Required for every built-in tool: `builtins::capability_governance_tests::all_registered_tools_declare_capability_metadata` fails (naming the tool) when any registered tool has no declaration. `owner` = the tool's module path (e.g. `organon::builtins::workspace`); `stability` = `Experimental` when the module is behind a cargo feature gate, otherwise `Stable` (`Planned` for a registered-but-unimplemented stub); `rollback` = read the executor body and state what its side effects actually allow -- `Supported` for pure reads/computations, otherwise `PartialSupport{reason}`/`Unsupported{reason}` citing the specific effect that cannot be undone (the reason field is mandatory, never a bare "no"). `redaction` defaults to `None` (trace surfaces carry the placeholder-form payload unchanged); set `Full`/`Fields` only when ordinary arguments legitimately carry secrets -- `Fields` names must be properties of the tool's input schema or the sibling gate test fails naming the field |

## Query axes

Query the registry for tools in two ways:

| Axis | Method | Semantics | When to use |
|------|--------|-----------|-------------|
| **Category** | `definitions_for_category` | Structural / navigational. Groups tools by domain (Workspace, Memory, Planning, etc.). | Browsing the tool surface by domain. |
| **Tags** | `definitions_for_tags` | Operational / semantic. Returns tools whose tags intersect the query set (union semantics). | "What tools help me look things up?" - cuts across categories. |

Tags are explicit, typed, and declared at registration time. Empty tag list returns an empty Vec (not "all tools").

### Tag variants

| Tag | Meaning | Example tools |
|-----|---------|---------------|
| `Recon` | Read-only inspection, discovery, search | `read`, `grep`, `find`, `ls`, `git_status`, `memory_search` |
| `Edit` | File or state mutation | `write`, `edit`, `mkdir`, `mv`, `cp`, `rm`, `note` |
| `Verify` | Tests, lints, checks, validation | `lint_report`, `verify_report`, `plan_verify`, `z3_solver` |
| `Fetch` | External data retrieval (HTTP, web) | `web_fetch`, `http_request`, `web_search` |
| `Spawn` | Sub-agent or task creation | `sessions_spawn`, `sessions_dispatch` |
| `Plan` | Planning, design-doc, strategy | `plan_create`, `plan_roadmap`, `plan_discuss` |
| `Execute` | Shell, cargo, runtime commands | `exec`, `computer_use`, `message` |
| `Format` | Document/report generation, output-shaping | `generate_document`, `render_*_report` |

Most tools carry 1–2 tags; a few carry 3.

## Dependencies

Uses: koina, hermeneus, taxis, eidos, gnosis, poiesis report crates, reqwest, tokio, serde, snafu, tracing, landlock, seccompiler
Used by: agora, aletheia, diaporeia, dokimion, integration-tests, nous, pylon, thesauros
