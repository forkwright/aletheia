# proskenion

**Purpose:** Dioxus desktop shell for Aletheia (excluded from the workspace build).

## Key types

| Type | Purpose |
|------|---------|
| `See L3 API index` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- Desktop app API is checked through `crates/theatron/proskenion/Cargo.toml`, outside the workspace L3 regen.

## When to look here

- When work touches `crates/theatron/proskenion` or downstream imports from `proskenion`.
- For exact signatures, load `_llm/L3-api-index/proskenion.md` if present, then source.

## Recent changes

Desktop remains excluded from the workspace build; use its standalone manifest for checks.
