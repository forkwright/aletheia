# pylon

**Purpose:** Axum HTTP gateway with SSE streaming, JWT auth middleware, rate limiting, CSRF protection, and idempotency handling.

## Key types

| Type | Purpose |
|------|---------|
| `AppState` | Shared state: stores, managers, JWT config, shutdown token |
| `ApiError` | Error enum with HTTP status code mapping |
| `Claims` | JWT auth extractor for Axum handlers |
| `RateLimiter` | Per-IP sliding window rate limiter |
| `IdempotencyCache` | TTL + LRU deduplication for message sends |

## Public API surface

- `pylon::router` — route construction and middleware layer ordering
- `pylon::state` — `AppState` shared across all handlers
- `pylon::handlers` — handler modules: sessions, streaming, nous, auth, costs
- `pylon::error` — `ApiError` with HTTP status mapping

## When to look here

- When adding a new HTTP endpoint (add handler in `src/handlers/`, register in `src/router.rs`)
- When modifying auth middleware, rate limits, or SSE streaming behavior
