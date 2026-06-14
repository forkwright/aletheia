# integration-tests

**Purpose:** integration-tests crate in the Aletheia workspace.

## Key types

| Type | Purpose |
|------|---------|
| `TestHarness` | Current public type or boundary; see L3/source for exact fields |
| `SubstrateCanarySuite` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `integration-tests::harness` - public items from `src/harness.rs`
- `integration-tests::tests/r722_substrate_canary` - public items from `tests/r722_substrate_canary.rs`

## When to look here

- When work touches `crates/integration-tests` or downstream imports from `integration-tests`.
- For exact signatures, load `_llm/L3-api-index/integration-tests.md` if present, then source.

## Recent changes

Shared TestHarness and a 25-scenario substrate canary suite cover operator-independent runtime claims.
