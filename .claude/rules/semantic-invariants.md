# Semantic Invariants

Design decisions that must not be regressed. Each is backed by an automated test.
If you need to change one, update the test first and explain why in the commit message.

## Error Hierarchy

- **All errors extend `AletheiaError`.** Never throw bare `Error` or string.
  Every error carries `code`, `module`, `message`, `timestamp`.
  Test: `src/invariants.test.ts` → "INVARIANT: error hierarchy"

- **Error codes are UPPER_SNAKE_CASE with MODULE_ prefix.**
  Defined once in `koina/error-codes.ts`. Never duplicate descriptions.
  Test: `src/invariants.test.ts` → "INVARIANT: error codes"

- **Error subclasses per module boundary:** ConfigError (taxis), SessionError (mneme),
  ProviderError (hermeneus), ToolError (organon), PipelineError (nous),
  PlanningError (dianoia), TransportError (semeion). Don't collapse or rename these.

## Graph Vocabulary

- **RELATES_TO must never appear in CONTROLLED_VOCAB or GRAPH_EXTRACTION_PROMPT.**
  It was eliminated in the vocab redesign (0% density validated across 1,194 edges).
  Reintroducing it as a "fallback" undoes the semantic typing system.
  Test: `tests/test_vocab.py` → `test_relates_to_not_in_vocab`, `test_graph_extraction_prompt_no_relates_to`

- **Unknown relationship types return None, not a fallback.**
  `normalize_type()` returns `None` for unmatched types. The caller decides what to do.
  Test: `tests/test_vocab.py` → `test_normalize_unknown_returns_none`

## Safe Wrappers

- **`trySafe` / `trySafeAsync` never propagate exceptions.** They log and return fallback.
  Used for non-critical operations where failure should not crash the pipeline.
  Test: `src/invariants.test.ts` → "INVARIANT: trySafe wrappers"

## Tool Registry

- **`ToolRegistry.register()` makes tools immediately resolvable via `get()`.**
  The registry is the single source of truth for available tools.
  Test: `src/invariants.test.ts` → "INVARIANT: tool registry"

## Event Bus

- **Event names follow `noun:verb` format** (e.g., `turn:before`, `tool:called`).
  Defined as a union type in `koina/event-bus.ts`. Adding a new event requires
  adding it to the `EventName` type — never use arbitrary strings.

## Module Exports

- **Key modules must export their public API.** Specifically:
  - `koina/errors.js` → all error subclasses
  - `koina/error-codes.js` → `ERROR_CODES` object
  - `koina/safe.js` → `trySafe`, `trySafeAsync`
  Test: `src/invariants.test.ts` → "INVARIANT: module exports"

---

## How to Add a New Invariant

1. Add the test to `src/invariants.test.ts` (TypeScript) or `tests/test_invariants.py` (Python)
2. Add a `# INVARIANT:` comment at the code site
3. Document it in this file
4. The pre-commit hook and CI will enforce it automatically
