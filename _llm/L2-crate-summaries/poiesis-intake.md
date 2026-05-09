# poiesis-intake

**Purpose:** Parse Slack-style request text into a structured report scaffold.

## Key types

| Type | Purpose |
|------|---------|
| `IntakeRequest` | Current public type or boundary; see L3/source for exact fields |
| `ReportScaffold` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-intake::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/intake` or downstream imports from `poiesis-intake`.
- For exact signatures, load `_llm/L3-api-index/poiesis-intake.md` if present, then source.

## Recent changes

Request intake is aligned with aletheia-lexica coherence work.
