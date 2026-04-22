# organon

Tool registry, definitions, and built-in tool executors for aletheia.

Organon (ὄργανον): "instrument." The formal instruments through which agent capability expresses.

## Overview

`organon` is the single source of truth for what tools an aletheia agent can use. It provides:

- **`ToolRegistry`** — the runtime registry; tools are registered at startup and looked up by name during execution.
- **`ToolDef`** / **`InputSchema`** — rich metadata for each tool (name, description, JSON Schema, reversibility, category).
- **Built-in executors** — implementations for all platform tools (filesystem, workspace, memory, planning, communication, research, agent coordination, and more).
- **`to_hermeneus_tools`** / **`to_hermeneus_tools_filtered`** — serialization to the `hermeneus::types::ToolDefinition` wire format for LLM requests.

## Feature flags

| Flag | Default | Description |
|---|---|---|
| `deferred-schemas` | OFF | Deferred tool-schema path (see below). |
| `energeia` | OFF | Energeia capability tools (dromeus, dokimasia, etc.). |
| `computer-use` | OFF | Screen capture and action dispatch (Linux kernel ≥5.13). |
| `z3` | OFF | Z3 SMT solver tool (bundles libz3). |
| `test-support` | OFF | Mock executors and component spec validation helpers. |
| `test-core` | OFF | Core test infrastructure (no external service mocks). |

## Deferred tool schemas (`deferred-schemas`)

### Problem

By default, every LLM request includes the full JSON Schema for every registered tool in the `tools` array. With 49+ built-in tools this is a substantial static token cost paid before the first user message — even for tools the agent never uses.

### Solution

The `deferred-schemas` feature switches tool-declaration serialization to **name + one-line description only**. Agents retrieve the full schema for a specific tool on demand by calling the `tool_schema` meta-tool before invoking it.

### Wire-format comparison

**Eager (flag off, default):**

```json
[
  {
    "name": "plan_create",
    "description": "Create a new planning project with phases and plans",
    "input_schema": {
      "type": "object",
      "properties": {
        "name": { "type": "string", "description": "Project name" },
        "description": { "type": "string", "description": "What this project aims to accomplish" },
        "mode": { "type": "string", "enum": ["full", "quick", "background"], "default": "full" }
      },
      "required": ["name", "description"]
    }
  }
]
```

**Deferred (flag on):**

```json
[
  {
    "name": "plan_create",
    "description": "Create a new planning project with phases and plans",
    "input_schema": { "type": "object", "properties": {}, "required": [] }
  }
]
```

### `tool_schema` meta-tool

Always registered (regardless of the feature flag). Agents call it when they need the full schema for a tool they haven't seen yet.

```json
// Request
{ "tool_name": "plan_create" }

// Response — full input_schema JSON object
{
  "type": "object",
  "properties": {
    "name": { "type": "string", "description": "Project name" },
    ...
  },
  "required": ["name", "description"]
}
```

### Enabling the flag

The flag is **default OFF** in v1. Operators flip it per-deployment after the flag has soaked in a test environment. To enable in a Cargo workspace:

```toml
# Cargo.toml (workspace or crate)
[dependencies]
organon = { path = "...", features = ["deferred-schemas"] }
```

Callers that build `CompletionRequest` must also switch from `to_hermeneus_tools` / `to_hermeneus_tools_filtered` to the corresponding `_summaries` variants when the flag is on. That wiring lives in `nous`; see the follow-up issue referenced in PR-3780.

### Observability

At session startup, `ToolRegistry::schema_byte_sizes()` emits a `tracing::info!` event:

```
organon tool-declaration sizes: eager=47832B deferred=8194B (49 tools)
```

This lets operators measure the actual reduction before committing to the deferred path.

### Size guarantee

Tests assert a **≥50% byte-size reduction** across the full builtin set when `deferred-schemas` is enabled (`deferred_load_shrinks_request_size_measurably`).

## Registration lifecycle

All built-in tools are registered via `register_all` or `register_all_with_sandbox`. Registration is two-phase:

1. **Phase 1**: all domain tools are registered.
2. **Phase 2**: `tool_schema` is registered last, capturing a serialized snapshot of every schema from phase 1.

The two-phase split avoids a self-referential ownership cycle: the registry owns the `tool_schema` executor, which needs schema data from the registry. The snapshot (a `Vec<(name, json)>`) breaks the cycle — the executor is self-contained and holds no back-reference to the registry.

## Standards

Full standards: `kanon/crates/basanos/standards/STANDARDS.md`.
