# mneme

**Purpose:** Session store and memory engine for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `SessionStore` | Current public type or boundary; see L3/source for exact fields |
| `KnowledgeStore` | Current public type or boundary; see L3/source for exact fields |
| `TraceIngestLayer` | Current public type or boundary; see L3/source for exact fields |
| `RecallEngine` | Current public type or boundary; see L3/source for exact fields |
| `validate_memory_path` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- No public Rust items were extracted; treat this crate as a facade, binary, or data crate and read source on demand.

## When to look here

- When work touches `crates/mneme` or downstream imports from `mneme`.
- For exact signatures, load `_llm/L3-api-index/mneme.md` if present, then source.

## Recent changes

The facade boundary was realigned; downstream crates should import memory/session/recall types through mneme re-exports.
