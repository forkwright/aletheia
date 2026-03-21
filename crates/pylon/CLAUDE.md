# pylon

HTTP gateway: Axum handlers, SSE streaming, auth middleware, rate limiting. 11K lines.

## Read first

1. `src/router.rs`: Route construction and middleware layer ordering (read the comments)
2. `src/state.rs`: AppState (shared state everything references)
3. `src/handlers/sessions/streaming.rs`: SSE streaming + idempotency
4. `src/middleware/mod.rs`: CSRF, request ID, rate limiting (per-IP + per-user)
5. `src/error.rs`: ApiError enum and HTTP status mapping

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `AppState` | `state.rs` | Shared state: stores, managers, JWT, config, shutdown token |
| `ApiError` | `error.rs` | Error enum mapping to HTTP status codes |
| `Claims` | `extract.rs` | JWT auth extractor |
| `SecurityConfig` | `security.rs` | CORS, CSRF, TLS, rate limit config |
| `SseEvent` | `stream.rs` | Message streaming events (TextDelta, ToolUse, etc.) |
| `RateLimiter` | `middleware/rate_limiter.rs` | Per-IP sliding window limiter |
| `UserRateLimiter` | `middleware/user_rate_limiter.rs` | Per-user token bucket with endpoint categories |
| `IdempotencyCache` | `idempotency.rs` | TTL + LRU dedup for message sends |

## Handler structure

All handlers: `async fn(State(Arc<AppState>), Claims, ...) -> Result<impl IntoResponse, ApiError>`

| Endpoint | Handler file |
|----------|-------------|
| `GET /api/health` | `handlers/health.rs` |
| `GET /metrics` | `handlers/metrics.rs` |
| `*/api/v1/sessions/*` | `handlers/sessions/` |
| `*/api/v1/nous/*` | `handlers/nous.rs` |
| `*/api/v1/config/*` | `handlers/config.rs` |
| `*/api/v1/knowledge/*` | `handlers/knowledge.rs` |

## Patterns

- **Middleware order** (outermost first): security headers, CORS, request ID, trace, compression, metrics, error enrichment, body limit, CSRF, rate limiting, then routes.
- **SSE streaming**: spawns tokio task subscribing to nous turn stream, converts to SseEvent, wraps in `Sse<Stream>` with KeepAlive.
- **Idempotency**: `Idempotency-Key` header on POST /messages. Cache hit replays response. In-flight returns 409.
- **CSRF**: custom header required on POST/PUT/DELETE/PATCH. Auto-generates per-instance token at startup.
- **5xx errors**: generic message to clients, real detail logged internally only.
- **OpenAPI**: utoipa derive on handlers, served at `GET /api/docs/openapi.json`.
- **Config hot-reload**: SIGHUP handler re-reads config, validates, swaps via watch channel.

## Common tasks

| Task | Where |
|------|-------|
| Add API endpoint | `src/handlers/` (handler fn) + `src/router.rs` (route) + `src/openapi.rs` (spec) |
| Add middleware | `src/middleware.rs` (fn) + `src/router.rs` (layer at correct position) |
| Add error type | `src/error.rs` (ApiError variant + IntoResponse mapping) |
| Add request type | `src/handlers/sessions/types.rs` or inline |

## Dependencies

Uses: koina, taxis, nous, hermeneus, mneme, organon, symbolon, axum, tower, tokio, snafu
Used by: aletheia (binary)
