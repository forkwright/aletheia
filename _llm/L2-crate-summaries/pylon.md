# pylon

**Purpose:** Axum HTTP gateway for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `AppState` | Current public type or boundary; see L3/source for exact fields |
| `ApiError` | Current public type or boundary; see L3/source for exact fields |
| `Claims` | Current public type or boundary; see L3/source for exact fields |
| `EventBus` | Current public type or boundary; see L3/source for exact fields |
| `AgentPerformance` | Current public type or boundary; see L3/source for exact fields |
| `QualityMetricsResponse` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `pylon::error` - public items from `src/error.rs`
- `pylon::event_bus` - public items from `src/event_bus.rs`
- `pylon::extract` - public items from `src/extract.rs`
- `pylon::handlers/config` - public items from `src/handlers/config.rs`
- `pylon::handlers/events` - public items from `src/handlers/events.rs`

## When to look here

- When work touches `crates/pylon` or downstream imports from `pylon`.
- For exact signatures, load `_llm/L3-api-index/pylon.md` if present, then source.

## Recent changes

Admin auth enforcement, OpenAPI honesty, SSE/handler drift fixes, and meta-insights endpoints are part of the gateway shape.
