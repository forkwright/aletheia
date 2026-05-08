# organon

## At a glance

Tool registry, executors, and sandbox for built-in tools. Depends on koina, hermeneus, and taxis. Entry point: `src/lib.rs` (ToolRegistry, ToolExecutor).

## Depth

Tool registry, executors, and sandbox. 16K lines. 49 built-in tools.

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
| `SandboxConfig` | `sandbox/mod.rs` | Landlock + seccomp + egress policy |
| `ProcessGuard` | `process_guard.rs` | RAII child process wrapper, `pub(crate)` (prevents orphans/zombies) |

## Built-in tools (49)

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
| Triage | issue_scan, issue_triage, issue_approve |
| Computer Use | computer_use (feature-gated: `computer-use`) |

`web_search` requires `BRAVE_SEARCH_API_KEY` at runtime (Brave Search API). `http_request` and `web_search` are lazy (activate via `enable_tool`). Git operations are read-only or non-destructive by design - no commit, push, reset, rebase, or `--force` checkout; destructive Git work still goes through `exec` under operator review.

## Patterns

- **Registration**: `ToolDef` + `impl ToolExecutor` -> `registry.register(def, Box::new(executor))`
- **Activation**: `auto_activate: true` = always available. `false` = requires `enable_tool` to activate.
- **Sandbox**: Linux only. Landlock (filesystem), seccomp (syscalls), network namespace. Permissive default.
- **Path validation**: normalize -> check allowed_roots -> canonicalize -> re-check. Tilde expansion.
- **ProcessGuard**: `kill()` + `wait()` on drop. Call `detach()` if process should outlive guard.

## Common tasks

| Task | Where |
|------|-------|
| Add built-in tool | New file in `src/builtins/`, implement ToolExecutor, register in `builtins/mod.rs` |
| Modify sandbox | `src/sandbox/mod.rs` (SandboxConfig) + `aletheia.toml` [sandbox] section |
| Add service trait | `src/types.rs` (new trait) + binary crate provides implementation |
| Add tool category | `src/types.rs` (ToolCategory enum) |
| Add tool tag | `src/types.rs` (ToolTag enum) |
| Tag a tool | Add `tags: vec![ToolTag::...]` to the tool's `_def()` function |

## Query axes

There are two ways to query the registry for tools:

| Axis | Method | Semantics | When to use |
|------|--------|-----------|-------------|
| **Category** | `definitions_for_category` | Structural / navigational. Groups tools by domain (Workspace, Memory, Planning, etc.). | Browsing the tool surface by domain. |
| **Tags** | `definitions_for_tags` | Operational / semantic. Returns tools whose tags intersect the query set (union semantics). | "What tools help me look things up?" — cuts across categories. |

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

Uses: koina, hermeneus, tokio, serde, snafu, tracing, landlock, seccompiler
Used by: nous, pylon, thesauros, aletheia (binary)
