# poiesis-lint

**Purpose:** Report prose linting: banned words, citation checks, structure checks.

## Key types

| Type | Purpose |
|------|---------|
| `LintError` | Current public type or boundary; see L3/source for exact fields |
| `Finding` | Current public type or boundary; see L3/source for exact fields |
| `FindingKind` | Current public type or boundary; see L3/source for exact fields |
| `LineFix` | Current public type or boundary; see L3/source for exact fields |
| `LintConfig` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-lint::error` - public items from `src/error.rs`
- `poiesis-lint::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/lint` or downstream imports from `poiesis-lint`.
- For exact signatures, load `_llm/L3-api-index/poiesis-lint.md` if present, then source.

## Recent changes

Comment filtering and report-lint drift fixes are reflected in the refreshed API surface.
