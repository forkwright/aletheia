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

## Observability

### Metrics (Prometheus)

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_http_requests_total` | Counter | `method`, `path`, `status` | Total HTTP requests (path normalized to prevent label explosion) |
| `aletheia_http_request_duration_seconds` | Histogram | `method`, `path` | Request latency in seconds (buckets: 0.005s to 10s) |
| `aletheia_active_sessions` | Gauge | - | Number of active sessions |
| `aletheia_uptime_seconds` | Gauge | - | Server uptime in seconds |

### Spans

| Span | Location | Fields |
|------|----------|--------|
| `session_create` | `handlers/sessions/mod.rs` | `agent_id` |
| `session_list` | `handlers/sessions/mod.rs` | - |
| `session_get` | `handlers/sessions/mod.rs` | - |
| `session_archive` | `handlers/sessions/mod.rs` | - |
| `session_unarchive` | `handlers/sessions/mod.rs` | - |
| `session_delete` | `handlers/sessions/mod.rs` | - |
| `session_send` | `handlers/sessions/mod.rs` | - |
| `session_send_streaming` | `handlers/sessions/streaming.rs` | `agent_id` |
| `config_get` | `handlers/config.rs` | - |
| `config_reload` | `handlers/config.rs` | - |
| `config_update` | `handlers/config.rs` | - |

### Log Events

| Level | Event | When |
|-------|-------|------|
| `info` | `idempotency cache hit` | Returning cached response for duplicate request |
| `warn` | `failed to load config, using defaults` | Config file read/parse error at startup |
| `warn` | `config reload failed, keeping current config` | Hot reload validation or merge failure |
| `warn` | `idempotency cache lock was poisoned, recovering` | Mutex poison recovery |
| `error` | `internal server error` | Unhandled 500 error with details |
| `error` | `turn failed` | SSE streaming turn execution error |
| `error` | `config deserialization failed after merge` | Post-merge validation error |
