# Phase 06: HTTP gateway

## Goal
Full HTTP API with SSE streaming, rate limiting, field-level validation, and OpenAPI documentation.

## Success criteria
- API serves 1000 concurrent connections with p99 latency under 100ms
- SSE streaming delivers tokens with < 50ms inter-token gap
- Rate limiting rejects excess requests with 429 and Retry-After header
- OpenAPI spec is auto-generated and matches implementation

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| API serves 1000 concurrent connections with p99 latency under 100ms | Load test shows p99 >= 100ms or connection drops |
| SSE streaming delivers tokens with < 50ms inter-token gap | Benchmark shows inter-token gap >= 50ms for 1K token stream |
| Rate limiting rejects excess requests with 429 and Retry-After header | Load test shows 200 OK for requests above limit |
| OpenAPI spec is auto-generated and matches implementation | Spec diff shows endpoint or schema mismatch |

## Scope

### In scope
- pylon crate: Axum routes, middleware, SSE streaming
- Field-level validation via utoipa
- TLS termination (optional)

### Out of scope
- GraphQL or gRPC endpoints
- WebSocket support (deferred)

## Requirements
- REQ-01: All routes have structured logging with request_id propagation
- REQ-02: SSE endpoints send keepalive pings every 15s
- REQ-03: Rate limits are configurable per route and per client
- REQ-04: Validation errors return RFC 7807 Problem Details

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Framework | Axum over Actix-web | Better Tower ecosystem integration |
| API docs | utoipa over hand-written OpenAPI | Derive macros keep spec in sync with code |

## Open questions
- Should we support HTTP/3? (Deferred: not enough client demand)

## Dependencies
- Phase 05 complete
- TLS certificates if TLS feature enabled
