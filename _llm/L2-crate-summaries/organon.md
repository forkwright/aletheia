# organon

**Purpose:** Tool registry, 49 built-in tool executors, and Landlock/seccomp sandbox for agent tool execution.

## Key types

| Type | Purpose |
|------|---------|
| `ToolExecutor` | Trait: `async execute(input, ctx) -> Result<ToolResult>` |
| `ToolRegistry` | Name-based dispatch with metrics and tracing |
| `ToolDef` | Tool metadata: name, description, JSON schema, category |
| `ToolContext` | Per-execution context: nous_id, session_id, workspace, services |
| `SandboxConfig` | Landlock + seccomp + egress policy |

## Public API surface

- `organon::registry` - `ToolExecutor` trait, `ToolRegistry`; call `register_all()` to load builtins
- `organon::types` - `ToolDef`, `ToolInput`, `ToolResult`, `ToolContext`, `ToolServices`
- `organon::sandbox` - `SandboxConfig` for process-level isolation

## When to look here

- When adding a new built-in tool (implement `ToolExecutor`, register in `register_all()`)
- When configuring or extending sandbox policy for tool execution
