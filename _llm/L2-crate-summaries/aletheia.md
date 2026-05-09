# aletheia

**Purpose:** Aletheia cognitive agent runtime.

## Key types

| Type | Purpose |
|------|---------|
| `Cli` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia::error` - public items from `src/error.rs`

## When to look here

- When work touches `crates/aletheia` or downstream imports from `aletheia`.
- For exact signatures, load `_llm/L3-api-index/aletheia.md` if present, then source.

## Recent changes

CLI scaffolds now persist seed skills, guard dry-run consolidation, and route drift-audit fixes through the binary wiring.
