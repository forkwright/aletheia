# krites

**Purpose:** Embedded Datalog engine with HNSW and graph support for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `Db` | Current public type or boundary; see L3/source for exact fields |
| `DataValue` | Current public type or boundary; see L3/source for exact fields |
| `TriggerConfig` | Current public type or boundary; see L3/source for exact fields |
| `FixedRule` | Current public type or boundary; see L3/source for exact fields |
| `FjallStorage` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `krites::async_surface` - public items from `src/async_surface.rs`
- `krites::counterfactual` - public items from `src/counterfactual.rs`
- `krites::data/expr/expr_impl` - public items from `src/data/expr/expr_impl.rs`
- `krites::data/expr` - public items from `src/data/expr/mod.rs`
- `krites::data/expr/op` - public items from `src/data/expr/op.rs`

## When to look here

- When work touches `crates/krites` or downstream imports from `krites`.
- For exact signatures, load `_llm/L3-api-index/krites.md` if present, then source.

## Recent changes

Eval truth work decoupled recall scenarios and added TriggerConfig/timeout helpers around the embedded engine tests.
