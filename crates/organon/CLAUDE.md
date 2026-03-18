# organon

Tool registry, executors, and sandbox. 12K lines. 33 built-in tools.

## Read first

1. `src/registry.rs`: ToolRegistry, ToolExecutor trait (the core abstraction)
2. `src/types.rs`: ToolDef, ToolInput, ToolResult, ToolContext, service traits
3. `src/builtins/mod.rs`: register_all() and module organization
4. `src/sandbox.rs`: Landlock + seccomp + network namespace config
5. `src/process_guard.rs`: RAII subprocess lifecycle (kill-on-drop)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `ToolExecutor` | `registry.rs` | Trait: `async execute(input, ctx) -> Result<ToolResult>` |
| `ToolRegistry` | `registry.rs` | Name-based dispatch with metrics and tracing |
| `ToolDef` | `types.rs` | Tool metadata: name, description, schema, category |
| `ToolContext` | `types.rs` | Per-execution context: nous_id, session_id, workspace, services |
| `ToolServices` | `types.rs` | Service locator: messaging, planning, knowledge, spawn |
| `SandboxConfig` | `sandbox.rs` | Landlock + seccomp + egress policy |
| `ProcessGuard` | `process_guard.rs` | RAII child process wrapper (prevents orphans/zombies) |

## Built-in tools (33)

| Category | Tools |
|----------|-------|
| Workspace | read, write, edit, exec, view_file, grep, find, ls |
| Memory | memory_search, memory_correct, memory_retract, memory_forget, memory_audit, note, blackboard, datalog_query |
| Communication | message, sessions_send, sessions_ask |
| Agent | sessions_spawn, sessions_dispatch, enable_tool |
| Planning | plan_create, plan_research, plan_requirements, plan_roadmap, plan_discuss, plan_execute, plan_verify, plan_status, plan_step_complete, plan_step_fail |
| Research | web_fetch |

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
| Modify sandbox | `src/sandbox.rs` (SandboxConfig) + `aletheia.toml` [sandbox] section |
| Add service trait | `src/types.rs` (new trait) + binary crate provides implementation |
| Add tool category | `src/types.rs` (ToolCategory enum) |

## Dependencies

Uses: koina, hermeneus, tokio, serde, snafu, tracing, landlock, seccompiler
Used by: nous, pylon, thesauros, aletheia (binary)
