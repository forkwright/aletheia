# skene

**Purpose:** Shared API client, domain types, SSE parser, and streaming infrastructure consumed by all Aletheia UIs (koilon TUI and proskenion desktop).

## Key types

| Type | Purpose |
|------|---------|
| `ApiClient` | HTTP client: agents, sessions, history, auth, costs, streaming |
| `StreamEvent` | Per-turn events: TurnStart, TextDelta, ThinkingDelta, ToolStart, TurnComplete |
| `SseStream` | Transforms byte stream into parsed SSE events (full spec) |
| `NousId` / `SessionId` | Domain identifier newtypes (separate from koina's server-side IDs) |
| `Agent` / `Session` | Wire types for agent info and session records |

## Public API surface

- `skene::api::client` — `ApiClient` for all REST and streaming operations
- `skene::events` — `StreamEvent` enum for per-turn streaming
- `skene::sse` — `SseEvent`, `SseStream` wire-level parser
- `skene::id` — `NousId`, `SessionId`, `TurnId`, `ToolId`, `PlanId` newtypes

## When to look here

- When building or modifying a UI client (shared API surface for all UIs lives here)
- When extending the SSE streaming protocol or adding new stream event types
