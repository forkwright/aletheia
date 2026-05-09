# koina

**Purpose:** Core types, errors, and tracing for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `Classifiable` | Current public type or boundary; see L3/source for exact fields |
| `AppError` | Current public type or boundary; see L3/source for exact fields |
| `MetricsRegistry` | Current public type or boundary; see L3/source for exact fields |
| `SystemEvent` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `koina::base64` - public items from `src/base64.rs`
- `koina::cleanup` - public items from `src/cleanup.rs`
- `koina::credential` - public items from `src/credential.rs`
- `koina::defaults` - public items from `src/defaults.rs`
- `koina::disk_space` - public items from `src/disk_space.rs`

## When to look here

- When work touches `crates/koina` or downstream imports from `koina`.
- For exact signatures, load `_llm/L3-api-index/koina.md` if present, then source.

## Recent changes

Timestamps use jiff consistently; tracing, cleanup events, and the Classifiable trait were wired into shared foundations.
