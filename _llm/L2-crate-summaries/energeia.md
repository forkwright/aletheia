# energeia

**Purpose:** Dispatch orchestration - actualization of plans into execution.

## Key types

| Type | Purpose |
|------|---------|
| `AgentSdkConfig` | Current public type or boundary; see L3/source for exact fields |
| `McpServerConfig` | Current public type or boundary; see L3/source for exact fields |
| `AgentSdkEngine` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `default_model` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `energeia::agent_sdk` - public items from `src/agent_sdk.rs`
- `energeia::backend/backend_impl` - public items from `src/backend/backend_impl.rs`
- `energeia::backend` - public items from `src/backend.rs`
- `energeia::budget` - public items from `src/budget.rs`
- `energeia::cost_ledger` - public items from `src/cost_ledger.rs`

## When to look here

- When work touches `crates/energeia` or downstream imports from `energeia`.
- For exact signatures, load `_llm/L3-api-index/energeia.md` if present, then source.

## Recent changes

L3 was refreshed for dispatch orchestration after cross-crate substrate changes.
