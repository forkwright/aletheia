# aletheia-classify

**Purpose:** aletheia-classify crate in the Aletheia workspace.

## Key types

| Type | Purpose |
|------|---------|
| `AuthorClass` | Current public type or boundary; see L3/source for exact fields |
| `index` | Current public type or boundary; see L3/source for exact fields |
| `as_str` | Current public type or boundary; see L3/source for exact fields |
| `ArtifactMetadata` | Current public type or boundary; see L3/source for exact fields |
| `AuthorProbs` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia-classify::classifier` - public items from `src/classifier.rs`
- `aletheia-classify::error` - public items from `src/error.rs`

## When to look here

- When work touches `crates/aletheia-classify` or downstream imports from `aletheia-classify`.
- For exact signatures, load `_llm/L3-api-index/aletheia-classify.md` if present, then source.

## Recent changes

L3 coverage was added for the classifier crate as part of the full workspace refresh.
