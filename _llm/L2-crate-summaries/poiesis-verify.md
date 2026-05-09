# poiesis-verify

**Purpose:** Report claim verification: arithmetic evaluation and source resolution.

## Key types

| Type | Purpose |
|------|---------|
| `VerifyError` | Current public type or boundary; see L3/source for exact fields |
| `Verifier` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `verify` | Current public type or boundary; see L3/source for exact fields |
| `verify_file` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-verify::error` - public items from `src/error.rs`
- `poiesis-verify::lib` - public items from `src/lib.rs`
- `poiesis-verify::manifest` - public items from `src/manifest.rs`

## When to look here

- When work touches `crates/poiesis/verify` or downstream imports from `poiesis-verify`.
- For exact signatures, load `_llm/L3-api-index/poiesis-verify.md` if present, then source.

## Recent changes

Typed content-drop errors and verification drift fixes are reflected in the refreshed API surface.
