# theatron-core

Shared API client, types, SSE parser, and streaming infrastructure for Aletheia UIs. 3K lines.

## Read first

1. `src/api/client.rs`: ApiClient (HTTP client for gateway REST API)
2. `src/api/types.rs`: Agent, Session, HistoryMessage, SseEvent, TurnOutcome, CostSummary
3. `src/events.rs`: StreamEvent enum (per-turn streaming: text deltas, tool calls, plan steps)
4. `src/sse.rs`: SseEvent, SseStream (wire protocol parser for reqwest byte streams)
5. `src/id.rs`: NousId, SessionId, TurnId, ToolId, PlanId (domain identifier newtypes)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `ApiClient` | `api/client.rs` | HTTP client: agents, sessions, history, auth, costs, streaming |
| `StreamEvent` | `events.rs` | Per-turn events: TurnStart, TextDelta, ThinkingDelta, ToolStart, ToolResult, ToolApprovalRequired, PlanProposed, TurnComplete |
| `SseEvent` | `sse.rs` | Parsed SSE wire event (event, data, id, retry fields) |
| `SseStream` | `sse.rs` | Transforms byte stream into parsed SSE events (full spec: multi-line data, comments, retry) |
| `NousId` | `id.rs` | Agent identifier newtype |
| `SessionId` | `id.rs` | Session identifier newtype |
| `TurnId` | `id.rs` | Turn identifier newtype |
| `Agent` | `api/types.rs` | Agent info: id, name, model, status |
| `Session` | `api/types.rs` | Session info: id, name, agent, message count, timestamps |
| `SseEvent` (api) | `api/types.rs` | Deserialized dashboard SSE events (agent status, session updates, costs) |
| `ApiError` | `api/error.rs` | Error type for all API operations |

## Patterns

- **Shared client**: Single reqwest Client with connection pool shared across REST, streaming, and SSE paths.
- **SSE parser**: Hand-rolled state machine handling the full SSE spec (data, event, id, retry, comments, multi-line data).
- **Domain newtypes**: NousId, SessionId, etc. prevent mixing identifiers across API boundaries.
- **Dual SSE types**: Wire-level SseEvent (sse.rs) vs domain-level SseEvent (api/types.rs) separate parsing from semantics.

## Common tasks

| Task | Where |
|------|-------|
| Add API endpoint | `src/api/client.rs` (method on ApiClient) + `src/api/types.rs` (request/response types) |
| Add stream event | `src/events.rs` (StreamEvent enum) + `src/api/streaming.rs` (parser) |
| Add domain ID type | `src/id.rs` |
| Add SSE dashboard event | `src/api/types.rs` (SseEvent enum) + `src/api/sse.rs` (parser) |

## Dependencies

Uses: koina, reqwest, serde_json, tokio, snafu
Used by: theatron-tui, theatron-desktop
