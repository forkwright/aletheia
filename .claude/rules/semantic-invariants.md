# Semantic Invariants

Design decisions that must not be regressed. Each is backed by an automated test.
If you need to change one, update the test first and explain why in the commit message.

## Error Hierarchy

- **All errors use `snafu` with context selectors.** Never use bare strings or `.unwrap()` in library code.
  Every error carries module context via `Location` tracking.

- **Error variants per crate boundary:** ConfigError (taxis), SessionError (mneme),
  ProviderError (hermeneus), ToolError (organon), PipelineError (nous),
  PlanningError (dianoia). Don't collapse or rename these.

## Graph Vocabulary

- **RELATES_TO must never appear in controlled vocabulary or extraction prompts.**
  It was eliminated in the vocab redesign (0% density validated across 1,194 edges).
  Reintroducing it as a "fallback" undoes the semantic typing system.
  Test: `tests/test_vocab.py` -> `test_relates_to_not_in_vocab`, `test_graph_extraction_prompt_no_relates_to`

- **Unknown relationship types return None, not a fallback.**
  `normalize_type()` returns `None` for unmatched types. The caller decides what to do.
  Test: `tests/test_vocab.py` -> `test_normalize_unknown_returns_none`

## Tool Registry

- **`ToolRegistry::register()` makes tools immediately resolvable via `get()`.**
  The registry is the single source of truth for available tools.

## Event Naming

- **Event names follow `noun:verb` format** (e.g., `turn:before`, `tool:called`).
  Adding a new event requires adding it to the typed event enum.

## Visibility

- **`pub(crate)` by default.** `pub` only for cross-crate API surface.
  Every public item is a commitment.

---

## How to Add a New Invariant

1. Add the test to the relevant crate's test suite
2. Add a comment at the code site
3. Document it in this file
4. CI will enforce it automatically
