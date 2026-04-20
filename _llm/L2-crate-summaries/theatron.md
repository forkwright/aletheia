# theatron

**Purpose:** Presentation umbrella crate re-exporting `skene` types for UI consumers. Groups skene (shared), koilon (TUI), and proskenion (desktop) under one import surface.

## Key types

| Type | Purpose |
|------|---------|
| `ApiClient` | Re-exported from skene: HTTP client for gateway REST API |
| `StreamEvent` | Re-exported from skene: per-turn streaming events |
| `NousId` / `SessionId` | Re-exported from skene: domain identifier newtypes |

## Public API surface

- `theatron::*` - re-exports skene's public types; no additional logic
- Sub-crates accessed directly: `aletheia-skene`, `aletheia-koilon`, `aletheia-proskenion`

## When to look here

- When you need a single import point for shared UI types without depending on skene directly
- For understanding the UI crate dependency structure (skene ← koilon, skene ← proskenion)
