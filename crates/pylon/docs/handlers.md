# Pylon handler reference

HTTP gateway for Aletheia. Built on Axum with SSE streaming, JWT auth, and layered security
middleware. All v1 routes are prefixed `/api/v1/`.

Machine-readable spec: `GET /api/docs/openapi.json`

---

## Middleware stack

Every request traverses the following stack, outermost first. Later sections note any
per-handler exceptions.

```
                         HTTP Request
                              │
                              ▼
             ┌────────────────────────────────┐
             │        Security Headers        │  X-Frame-Options: DENY
             │                                │  X-Content-Type-Options: nosniff
             │                                │  X-XSS-Protection: 0
             │                                │  Referrer-Policy: strict-origin…
             │                                │  Content-Security-Policy: default-src 'self'
             │                                │  Strict-Transport-Security (TLS only)
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │              CORS              │  Validates Origin against allow-list.
             │                                │  Handles preflight OPTIONS.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │          Request ID            │  Generates ULID; stored in request
             │                                │  extensions for downstream layers.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │        HTTP Trace Span         │  OpenTelemetry span with method,
             │                                │  path, request_id, status code.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │          Compression           │  gzip / brotli / zstd
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │         HTTP Metrics           │  Prometheus: request count,
             │                                │  latency histogram per method/path.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │       Error Enrichment         │  Injects request_id into 4xx/5xx
             │                                │  JSON error bodies.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │          Body Limit            │  Default 1 MB max request body.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │       CSRF Validation *        │  Custom header required on
             │                                │  POST / PUT / DELETE / PATCH.
             └────────────────┬───────────────┘  * when enabled in config
                              │
                              ▼
             ┌────────────────────────────────┐
             │      Per-IP Rate Limit *       │  Sliding window, keyed on
             │                                │  source IP (or X-Forwarded-For
             │                                │  when trust_proxy = true).
             └────────────────┬───────────────┘  * when enabled in config
                              │
                              ▼
             ┌────────────────────────────────┐
             │    Per-User Rate Limit *       │  Token bucket per user.
             │                                │  Three classes: General, LLM
             │                                │  (messages/stream), Tool.
             └────────────────┬───────────────┘  * when enabled in config
                              │
                              ▼
             ┌────────────────────────────────┐
             │     JWT Claims (per handler)   │  Bearer token extracted and
             │                                │  validated; yields sub, role,
             │                                │  optional nous_id scope.
             └────────────────┬───────────────┘
                              │
                              ▼
             ┌────────────────────────────────┐
             │          Route Handler         │
             └────────────────────────────────┘
```

**Layer ordering constraints** (from `router.rs` comments):

- Request ID must be added *before* the trace layer so the span captures the ID.
- Error enrichment must be *inside* compression (body is uncompressed there) but *outside*
  CSRF and rate limiting so their error responses get the request ID injected.
- Security headers are outermost so they apply to every response, including error responses
  from middleware.

### Error response envelope

All 4xx / 5xx responses share this JSON shape:

```json
{
  "error": {
    "code": "session_not_found",
    "message": "Session abc123 not found.",
    "request_id": "01HXYZ…",
    "details": ["optional", "list"]
  }
}
```

5xx bodies return `"An internal error occurred."` — the real detail is logged server-side only.

---

## Infrastructure

### `GET /api/health`

Liveness and readiness check. No auth required.

**Middleware path:** full stack minus JWT Claims (no auth extractor).

**Response `200 OK` / `503 Service Unavailable`:**

```json
{
  "status": "healthy",
  "version": "0.13.0",
  "uptime_seconds": 3721,
  "checks": [
    { "name": "session_store", "status": "pass", "message": null },
    { "name": "providers",     "status": "warn", "message": "no LLM providers registered" },
    { "name": "nous_actors",   "status": "pass", "message": null }
  ]
}
```

`status` values: `"healthy"` (all pass), `"degraded"` (any warn), `"unhealthy"` (any fail).
HTTP 503 is returned only when status is `"unhealthy"`.

---

### `GET /metrics`

Prometheus text-format exposition. No auth required.

**Middleware path:** full stack minus JWT Claims.

**Response `200 OK`** — Content-Type: `text/plain; version=0.0.4; charset=utf-8`

Plain Prometheus text format. Metrics include HTTP request counts and latency histograms
labelled by method and path.

---

### `GET /api/docs/openapi.json`

OpenAPI 3 specification. No auth required.

**Response `200 OK`** — JSON OpenAPI document.

---

## Sessions

All session endpoints require a valid Bearer token (`Claims` extractor). State-changing
endpoints additionally require CSRF header when CSRF is enabled.

```
POST /api/v1/sessions/{id}/messages  ─── Idempotency-Key header (optional, max 64 chars)
                                     │
                    ┌────────────────┴──────────────────┐
                    │        Idempotency Cache           │  TTL 5 min, LRU cap 10 k
                    │                                    │
                    │  Miss  ──► run turn ──► cache ──► SSE stream
                    │  Hit   ──► replay cached response
                    │  In-flight  ──► 409 Conflict
                    └───────────────────────────────────┘
```

### `POST /api/v1/sessions`

Create a new session.

**Request body:**

```json
{ "nous_id": "pronoea", "session_key": "my-chat" }
```

`session_key` is a client-chosen string for deduplication (e.g. a stable UI identifier).

**Response `201 Created`** — `SessionResponse`:

```json
{
  "id": "01HXYZ…",
  "nous_id": "pronoea",
  "session_key": "my-chat",
  "status": "active",
  "model": "anthropic/claude-opus-4-6",
  "name": null,
  "message_count": 0,
  "token_count_estimate": 0,
  "created_at": "2026-03-19T10:00:00Z",
  "updated_at": "2026-03-19T10:00:00Z"
}
```

---

### `GET /api/v1/sessions`

List sessions.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `nous_id` | string | Filter by agent |
| `limit` | u32 | Max results (default server-side) |

**Response `200 OK`** — `ListSessionsResponse`:

```json
{
  "sessions": [
    {
      "id": "01HXYZ…",
      "nous_id": "pronoea",
      "session_key": "my-chat",
      "status": "active",
      "message_count": 12,
      "updated_at": "2026-03-19T10:05:00Z",
      "display_name": "Planning session"
    }
  ]
}
```

---

### `GET /api/v1/sessions/{id}`

Get session detail.

**Path:** `id` — session ULID.

**Response `200 OK`** — `SessionResponse` (same shape as create response).
**Response `404 Not Found`** — session not found.

---

### `DELETE /api/v1/sessions/{id}`

Archive a session (soft delete). Equivalent to `POST /api/v1/sessions/{id}/archive`.

**Response `200 OK`** — `SessionResponse` with `status: "archived"`.

---

### `POST /api/v1/sessions/{id}/archive`

Archive a session.

**Response `200 OK`** — `SessionResponse` with `status: "archived"`.

---

### `POST /api/v1/sessions/{id}/unarchive`

Reactivate an archived session.

**Response `200 OK`** — `SessionResponse` with `status: "active"`.

---

### `DELETE /api/v1/sessions/{id}/purge`

Permanently delete a session and all its messages. Irreversible.

**Response `200 OK`** — empty body.
**Response `404 Not Found`** — session not found.

---

### `PUT /api/v1/sessions/{id}/name`

Rename a session.

**Request body:**

```json
{ "name": "Planning session" }
```

**Response `200 OK`** — `SessionResponse` with updated `name`.

---

### `POST /api/v1/sessions/{id}/messages`

Send a user message and stream the agent's response as Server-Sent Events.

**Request headers:**

| Header | Required | Description |
|--------|----------|-------------|
| `Authorization` | Yes | `Bearer <token>` |
| `Idempotency-Key` | No | Client key (max 64 chars) for retry deduplication |

**Request body:**

```json
{ "content": "What is the capital of France?" }
```

**Response `200 OK`** — `text/event-stream`

Each SSE event has `data: <json>`. Event types:

| Type | Fields | Description |
|------|--------|-------------|
| `text_delta` | `text: string` | Incremental assistant text |
| `thinking_delta` | `thinking: string` | Extended thinking text (when enabled) |
| `tool_use` | `id, name, input` | Tool invocation |
| `tool_result` | `tool_use_id, content, is_error` | Tool result |
| `message_complete` | `stop_reason, usage: {input_tokens, output_tokens}` | Turn end |
| `error` | `code, message` | Error during turn |

**Response `409 Conflict`** — Idempotency-Key already in-flight.

**Middleware path note:** POST — passes through CSRF check if enabled.

---

### `GET /api/v1/sessions/{id}/history`

Retrieve conversation history.

**Query parameters:**

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | u32 | 50 | Max messages (1–1000) |
| `before` | i64 | — | Return messages with `seq` < this value (pagination) |

**Response `200 OK`** — `HistoryResponse`:

```json
{
  "messages": [
    {
      "id": 1,
      "seq": 1,
      "role": "user",
      "content": "What is the capital of France?",
      "tool_call_id": null,
      "tool_name": null,
      "created_at": "2026-03-19T10:00:01Z"
    },
    {
      "id": 2,
      "seq": 2,
      "role": "assistant",
      "content": "Paris.",
      "tool_call_id": null,
      "tool_name": null,
      "created_at": "2026-03-19T10:00:02Z"
    }
  ]
}
```

`role` values: `"user"`, `"assistant"`, `"tool"`. Tool result messages include
`tool_call_id` and `tool_name`.

---

### `POST /api/v1/sessions/stream`

TUI streaming protocol. Creates or retrieves a session by `(agent_id, session_key)` and
streams the turn using the webchat event format (distinct from the SSE format above).

**Request body:**

```json
{ "agentId": "pronoea", "message": "Hello", "sessionKey": "main" }
```

`sessionKey` defaults to `"main"` if omitted.

**Response `200 OK`** — `text/event-stream` of `WebchatEvent`:

| Type | Fields |
|------|--------|
| `turn_start` | `session_id, nous_id, turn_id` |
| `thinking_delta` | `thinking` |
| `text_delta` | `text` |
| `tool_start` | `id, name, input` |
| `tool_result` | `tool_use_id, content, is_error` |
| `turn_complete` | `stop_reason, usage, thinking_tokens, tool_iterations` |
| `error` | `code, message` |

---

### `GET /api/v1/events`

Global SSE channel used by the TUI dashboard. Streams server-side events across all sessions
for the authenticated user. No request body.

**Response `200 OK`** — `text/event-stream`

---

## Nous (agents)

### `GET /api/v1/nous`

List all registered nous agents.

**Response `200 OK`** — `NousListResponse`:

```json
{
  "nous": [
    { "id": "pronoea", "name": "Pronoea", "model": "anthropic/claude-opus-4-6", "status": "active" }
  ]
}
```

---

### `GET /api/v1/nous/{id}`

Get detailed status of a single agent.

**Response `200 OK`** — `NousStatus`:

```json
{
  "id": "pronoea",
  "model": "anthropic/claude-opus-4-6",
  "context_window": 200000,
  "max_output_tokens": 4096,
  "thinking_enabled": true,
  "thinking_budget": 10000,
  "max_tool_iterations": 10,
  "status": "active"
}
```

**Response `404 Not Found`** — nous not found.

---

### `GET /api/v1/nous/{id}/tools`

List tools available to a nous agent.

**Response `200 OK`** — `ToolsResponse`:

```json
{
  "tools": [
    {
      "name": "read_file",
      "description": "Read a file from the workspace.",
      "category": "Builtin",
      "auto_activate": true
    }
  ]
}
```

`auto_activate: false` means the tool is lazy and must be explicitly enabled before use.

**Response `404 Not Found`** — nous not found.

---

## Configuration

Config endpoints require an `Operator` or `Admin` role.

### `GET /api/v1/config`

Full redacted runtime configuration.

**Response `200 OK`** — JSON object with all config sections. Secrets are redacted.

---

### `GET /api/v1/config/{section}`

Single config section.

**Path:** `section` — one of: `agents`, `gateway`, `channels`, `bindings`, `embedding`,
`data`, `packs`, `maintenance`, `pricing`.

**Response `200 OK`** — JSON object for the section.
**Response `404 Not Found`** — unknown section name.

---

### `POST /api/v1/config/reload`

Re-read config from disk, validate, and apply hot-reloadable values. Equivalent to sending
`SIGHUP` to the process.

**Response `200 OK`:**

```json
{
  "restart_required": ["bindings.port"],
  "message": "Config reloaded. Some changes require a restart."
}
```

`restart_required` lists config keys whose new values cannot be applied without restarting.

---

### `PUT /api/v1/config/{section}`

Update and persist a config section. Deep-merges the request body into the current section,
validates the result, writes to disk, and broadcasts via the config watch channel.

**Request body:** JSON object matching the section schema.

**Response `200 OK`:** Updated section + `restart_required` list.
**Response `422 Unprocessable Entity`:** Validation failed; `details` lists errors.

---

## Knowledge

Knowledge endpoints are feature-gated on the `knowledge` feature and require a valid Bearer
token. Write operations (forget, restore, confidence update) require CSRF header when enabled.

```
GET  /api/v1/knowledge/facts
GET  /api/v1/knowledge/facts/{id}
POST /api/v1/knowledge/facts/{id}/forget
POST /api/v1/knowledge/facts/{id}/restore
PUT  /api/v1/knowledge/facts/{id}/confidence
GET  /api/v1/knowledge/entities
GET  /api/v1/knowledge/entities/{id}/relationships
GET  /api/v1/knowledge/search
GET  /api/v1/knowledge/timeline
```

### `GET /api/v1/knowledge/facts`

List facts with filtering, sorting, and pagination.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `nous_id` | string | Filter by agent |
| `sort` | string | Sort field (e.g. `confidence`, `created_at`) |
| `order` | `asc` \| `desc` | Sort direction |
| `filter` | string | Text filter |
| `fact_type` | string | Epistemic type filter |
| `tier` | string | Epistemic tier filter |
| `limit` | u32 | Max results |
| `offset` | u32 | Pagination offset |
| `include_forgotten` | bool | Include forgotten facts |

**Response `200 OK`** — JSON array of fact summaries.

---

### `GET /api/v1/knowledge/facts/{id}`

Fact detail with relationships and similar facts.

**Response `200 OK`** — Fact with `relationships` and `similar_facts` arrays.
**Response `404 Not Found`** — fact not found.

---

### `POST /api/v1/knowledge/facts/{id}/forget`

Mark a fact as forgotten. Does not delete — recoverable via restore.

**Response `200 OK`** — Updated fact.

---

### `POST /api/v1/knowledge/facts/{id}/restore`

Restore a forgotten fact.

**Response `200 OK`** — Updated fact.

---

### `PUT /api/v1/knowledge/facts/{id}/confidence`

Update confidence score for a fact.

**Request body:**

```json
{ "confidence": 0.85 }
```

**Response `200 OK`** — Updated fact.

---

### `GET /api/v1/knowledge/entities`

List entities in the knowledge graph.

**Response `200 OK`** — JSON array of entity summaries.

---

### `GET /api/v1/knowledge/entities/{id}/relationships`

Relationships for a single entity.

**Response `200 OK`** — JSON array of relationships with `relation`, `target_id`, `target_label`.

---

### `GET /api/v1/knowledge/search`

Full-text search over the knowledge graph.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `q` | string | Search query |
| `nous_id` | string | Scope to agent |
| `limit` | u32 | Max results |

**Response `200 OK`** — JSON array of matching facts with relevance scores.

---

### `GET /api/v1/knowledge/timeline`

Fact activity timeline ordered by recency.

**Response `200 OK`** — JSON array of timeline entries.

---

## Routing notes

- Legacy unversioned paths (e.g. `/api/nous`) return **410 Gone** with a migration hint
  pointing to the equivalent `/api/v1/` path.
- Unknown paths return **404 Not Found**.

---

## Keeping this document in sync

`cargo test -p aletheia-pylon -- handler_doc` runs a test that verifies this file contains
an entry for every route registered in `src/router.rs`. Add new routes to both files.
