# organon

**Purpose:** Tool registry, definitions, and built-in tool executors.

## Key types

| Type | Purpose |
|------|---------|
| `ToolRegistry` | Current public type or boundary; see L3/source for exact fields |
| `ToolDef` | Current public type or boundary; see L3/source for exact fields |
| `ToolTag` | Current public type or boundary; see L3/source for exact fields |
| `KnowledgeSearchService` | Current public type or boundary; see L3/source for exact fields |
| `WorkingCheckpoint` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `organon::builtins/computer_use/executor` - public items from `src/builtins/computer_use/executor.rs`
- `organon::builtins/energeia` - public items from `src/builtins/energeia/mod.rs`
- `organon::builtins/energeia/shared` - public items from `src/builtins/energeia/shared.rs`
- `organon::builtins` - public items from `src/builtins/mod.rs`
- `organon::builtins/skill_read` - public items from `src/builtins/skill_read.rs`

## When to look here

- When work touches `crates/organon` or downstream imports from `organon`.
- For exact signatures, load `_llm/L3-api-index/organon.md` if present, then source.

## Recent changes

Tool definitions now carry typed tags, registry tag lookup is supported, file-ref interpolation is available, and working_checkpoint is registered.
