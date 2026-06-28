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
             │                                │  when gateway.rateLimit.trustProxy
             │                                │  is true).
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

5xx bodies return `"An internal error occurred."` - the real detail is logged server-side only.

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

**Response `200 OK`** - Content-Type: `text/plain; version=0.0.4; charset=utf-8`

Plain Prometheus text format. Metrics include HTTP request counts and latency histograms
labelled by method and path.

---

### `GET /api/docs/openapi.json`

OpenAPI 3 specification. No auth required.

**Response `200 OK`** - JSON OpenAPI document.

---

## Sessions

All session endpoints require a valid Bearer token (`Claims` extractor). State-changing
endpoints also require CSRF header when CSRF is enabled.

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

**Response `201 Created`** - `SessionResponse`:

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

**Response `200 OK`** - `ListSessionsResponse`:

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

**Path:** `id` - session ULID.

**Response `200 OK`** - `SessionResponse` (same shape as create response).
**Response `404 Not Found`** - session not found.

---

### `DELETE /api/v1/sessions/{id}`

Archive a session (soft delete). Equivalent to `POST /api/v1/sessions/{id}/archive`.

**Response `200 OK`** - `SessionResponse` with `status: "archived"`.

---

### `POST /api/v1/sessions/{id}/archive`

Archive a session.

**Response `200 OK`** - `SessionResponse` with `status: "archived"`.

---

### `POST /api/v1/sessions/{id}/unarchive`

Reactivate an archived session.

**Response `200 OK`** - `SessionResponse` with `status: "active"`.

---

### `DELETE /api/v1/sessions/{id}/purge`

Permanently delete a session and all its messages. Irreversible.

**Response `200 OK`** - empty body.
**Response `404 Not Found`** - session not found.

---

### `PUT /api/v1/sessions/{id}/name`

Rename a session.

**Request body:**

```json
{ "name": "Planning session" }
```

**Response `200 OK`** - `SessionResponse` with updated `name`.

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

**Response `200 OK`** - `text/event-stream`

Each SSE event has `data: <json>`. Event types:

| Type | Fields | Description |
|------|--------|-------------|
| `text_delta` | `text: string` | Incremental assistant text |
| `thinking_delta` | `thinking: string` | Extended thinking text (when enabled) |
| `tool_use` | `id, name, input` | Tool invocation |
| `tool_result` | `tool_use_id, content, is_error` | Tool result |
| `message_complete` | `stop_reason, usage: {input_tokens, output_tokens}` | Turn end |
| `error` | `code, message` | Error during turn |

**Response `409 Conflict`** - Idempotency-Key already in-flight.

**Middleware path note:** POST - passes through CSRF check if enabled.

---

### `GET /api/v1/sessions/{id}/history`

Retrieve conversation history.

**Query parameters:**

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | u32 | 50 | Max messages (1–1000) |
| `before` | i64 | - | Return messages with `seq` < this value (pagination) |

**Response `200 OK`** - `HistoryResponse`:

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

Turn stream protocol. Creates or retrieves a session by `(nous_id, session_key)` and
streams the turn as `TurnStreamEvent` SSE events (used by TUI and desktop clients).

**Request body:**

```json
{ "nous_id": "pronoea", "message": "Hello", "session_key": "main" }
```

`session_key` defaults to `"main"` if omitted.

**Response `200 OK`** - `text/event-stream` of `TurnStreamEvent`:

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

Canonical global SSE channel used by first-party clients. Streams domain events across sessions
for the authenticated user.

Each delivered event uses the SSE topic as the `event:` name, the EventBus cursor as the `id:`,
and the topic payload as `data:`:

```text
event: turn.complete
id: 42
data: {"session_id":"...","nous_id":"...","turn_id":"...","input_tokens":1,"output_tokens":2}
```

The optional `topics` query parameter narrows delivery to a comma-separated topic list. When it
is omitted, the stream includes all discoverable global topics. The server sends periodic
`: heartbeat` comments using the configured gateway SSE heartbeat interval. Clients reconnect
with the standard `Last-Event-ID` header, or the `last_event_id` query parameter where headers
are unavailable. If the requested cursor has fallen out of the in-memory journal, the stream
first emits a `stream_gap` control event; slow live subscribers receive `stream_lagged`.

**Response `200 OK`** - `text/event-stream`

---

### `GET /api/v1/events/subscribe`

Compatibility alias for `GET /api/v1/events`. New first-party clients use `/api/v1/events`.

**Response `200 OK`** - `text/event-stream`

---

### `GET /api/v1/events/discovery`

Returns a JSON manifest of available event types, their schemas, and current subscription state.
Used by clients to enumerate supported SSE event kinds before subscribing.

**Response `200 OK`** - JSON event-type manifest.

---

### `POST /api/v1/sessions/{id}/approvals`

Resolve a pending tool-approval request for an active session. Used by desktop and TUI clients
to grant or deny a queued tool call.

**Request body** - `ApprovalResolution`:
```json
{ "approval_id": "01JXKQ2T...", "decision": "approve" }
```

**Response `200 OK`** - Acknowledgement with updated approval state.

---

### `GET /api/v1/ops/tools`

List all registered tool adapters and their current enabled/disabled state. Intended for
operator inspection and debugging.

**Response `200 OK`** - JSON array of tool descriptors.

---

## Nous (agents)

### `GET /api/v1/nous`

List all registered nous agents.

**Response `200 OK`** - `NousListResponse`:

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

**Response `200 OK`** - `NousStatus`:

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

**Response `404 Not Found`** - nous not found.

---

### `GET /api/v1/nous/{id}/tools`

List tools available to a nous agent.

**Response `200 OK`** - `ToolsResponse`:

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

**Response `404 Not Found`** - nous not found.

---

### `POST /api/v1/nous/{id}/recover`

Trigger manual recovery for a nous agent that has entered an error or stuck state. Resets
internal FSM and clears any pending turn buffer.

**Response `200 OK`** - Acknowledgement.

**Response `404 Not Found`** - nous not found.

---

## Configuration

Config endpoints require an `Operator` or `Admin` role.

### `GET /api/v1/config`

Full redacted runtime configuration.

**Response `200 OK`** - JSON object with all config sections. Secrets are redacted.

---

### `GET /api/v1/config/{section}`

Single config section.

**Path:** `section` - one of: `agents`, `gateway`, `channels`, `bindings`, `embedding`,
`data`, `packs`, `maintenance`, `pricing`.

**Response `200 OK`** - JSON object for the section.
**Response `404 Not Found`** - unknown section name.

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
token. Write operations (forget, restore, confidence update, import, ingest) require CSRF
header when enabled.

```
GET  /api/v1/knowledge/facts
POST /api/v1/knowledge/facts/import
GET  /api/v1/knowledge/facts/{id}
POST /api/v1/knowledge/facts/{id}/forget
POST /api/v1/knowledge/facts/{id}/restore
PUT  /api/v1/knowledge/facts/{id}/confidence
POST /api/v1/knowledge/ingest
POST /api/v1/knowledge/ingest/webhook
GET  /api/v1/knowledge/entities
POST /api/v1/knowledge/entities/merge
GET  /api/v1/knowledge/entities/{id}/relationships
GET  /api/v1/knowledge/entities/{id}/memories
POST /api/v1/knowledge/entities/{id}/flag
GET  /api/v1/knowledge/search
GET  /api/v1/knowledge/search/explain
GET  /api/v1/knowledge/timeline
GET  /api/v1/knowledge/check
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

**Response `200 OK`** - JSON array of fact summaries.

---

### `GET /api/v1/knowledge/facts/{id}`

Fact detail with relationships and similar facts.

**Response `200 OK`** - Fact with `relationships` and `similar_facts` arrays.
**Response `404 Not Found`** - fact not found.

---

### `POST /api/v1/knowledge/facts/{id}/forget`

Mark a fact as forgotten. Does not delete - recoverable via restore.

**Response `200 OK`** - Updated fact.

---

### `POST /api/v1/knowledge/facts/{id}/restore`

Restore a forgotten fact.

**Response `200 OK`** - Updated fact.

---

### `PUT /api/v1/knowledge/facts/{id}/confidence`

Update confidence score for a fact.

**Request body:**

```json
{ "confidence": 0.85 }
```

**Response `200 OK`** - Updated fact.

---

### `POST /api/v1/knowledge/facts/import`

Bulk-import facts from a JSON array. Each item must satisfy the `FactImport` schema.
Idempotent by fact ID.

**Response `200 OK`** - Import summary with counts of created, skipped, and failed records.

---

### `POST /api/v1/knowledge/ingest`

Ingest unstructured text or a document blob. Extraction and entity-linking run asynchronously;
the response returns an ingest job ID for polling.

**Response `202 Accepted`** - `{ "job_id": "01JXL..." }`

---

### `POST /api/v1/knowledge/ingest/webhook`

Ingest endpoint for external webhook sources (e.g. n8n, Zapier). Validates an HMAC signature
from the `X-Aletheia-Webhook-Signature` header before accepting.

**Response `202 Accepted`** - `{ "job_id": "01JXL..." }`

---

### `GET /api/v1/knowledge/entities`

List entities in the knowledge graph.

**Response `200 OK`** - JSON array of entity summaries.

---

### `POST /api/v1/knowledge/entities/merge`

Merge two or more entities by canonical ID. The surviving entity inherits all relationships
and facts of the merged entities.

**Request body** - `EntityMergeRequest`: `{ "source_ids": ["id-a", "id-b"], "target_id": "id-a" }`

**Response `200 OK`** - Merged entity record.

---

### `GET /api/v1/knowledge/entities/{id}/relationships`

Relationships for a single entity.

**Response `200 OK`** - JSON array of relationships with `relation`, `target_id`, `target_label`.

---

### `GET /api/v1/knowledge/entities/{id}/memories`

Memory records associated with a specific entity, scoped to active nous agents.

**Response `200 OK`** - JSON array of memory records.

---

### `POST /api/v1/knowledge/entities/{id}/flag`

Flag an entity for review (e.g. suspected merge error, stale data, offensive label).

**Request body** - `{ "reason": "stale" }`

**Response `200 OK`** - Updated entity with `flagged: true`.

---

### `GET /api/v1/knowledge/search`

Full-text search over the knowledge graph.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `q` | string | Search query |
| `nous_id` | string | Scope to agent |
| `limit` | u32 | Max results |

**Response `200 OK`** - JSON array of matching facts with relevance scores.

---

### `GET /api/v1/knowledge/search/explain`

Returns the ranked scoring breakdown for the most recent search query in the session. Used for
debugging recall ranking and tuning confidence weights.

**Response `200 OK`** - JSON scoring breakdown per result.

---

### `GET /api/v1/knowledge/timeline`

Fact activity timeline ordered by recency.

**Response `200 OK`** - JSON array of timeline entries.

---

### `GET /api/v1/knowledge/check`

Shallow health check for the knowledge graph store (connectivity, index integrity). Returns
a machine-readable status object rather than a plain `200`.

**Response `200 OK`** - `{ "status": "ok", "fact_count": 142, "entity_count": 37 }`

---

## Workspace

Workspace routes are feature-gated on operator role and scoped to the instance workspace
directory. They surface file, diff, and search operations for agent tooling.

### `GET /api/v1/workspace/files`

List files in the instance workspace directory.

**Response `200 OK`** - JSON array of file entries with `path`, `size`, `modified`.

---

### `GET /api/v1/workspace/git-status`

Current `git status` of the workspace repository. Returns a structured diff summary.

**Response `200 OK`** - JSON with `staged`, `unstaged`, `untracked` arrays.

---

### `GET /api/v1/workspace/diff`

File-level unified diff for the workspace. Scoped by optional `path` query parameter.

**Response `200 OK`** - JSON with per-file unified diff strings.

---

### `POST /api/v1/workspace/open`

Open a file in the workspace by path. Returns its contents and detected language.

**Request body** - `{ "path": "src/main.rs" }`

**Response `200 OK`** - `{ "path": "...", "content": "...", "language": "rust" }`

---

### `GET /api/v1/workspace/search`

Full-text search over workspace files.

**Query parameters:** `q` (required), `limit` (optional, default 20).

**Response `200 OK`** - JSON array of matches with `path`, `line`, `snippet`.

---

## System credentials

Credential endpoints require Admin role and CSRF protection.

### `GET /api/v1/system/credentials`

List all registered credentials (names and types only; secrets are never returned).

**Response `200 OK`** - JSON array of credential descriptors.

---

### `POST /api/v1/system/credentials`

Add a new credential entry.

**Request body** - `CredentialAdd`: `{ "name": "anthropic-prod", "kind": "api-key", "value": "sk-..." }`

**Response `201 Created`** - Created credential descriptor.

---

### `POST /api/v1/system/credentials/rotate`

Rotate all credentials that support automatic rotation. Returns a per-credential rotation result.

**Response `200 OK`** - JSON array of rotation results.

---

### `DELETE /api/v1/system/credentials/{id}`

Remove a credential by ID.

**Response `204 No Content`**

---

### `POST /api/v1/system/credentials/{id}/validate`

Validate a stored credential by making a lightweight probe to the target service.

**Response `200 OK`** - `{ "valid": true }` or `{ "valid": false, "error": "..." }`

---

## Metrics

Metrics endpoints expose aggregated behavioral and cost analytics. Require Operator role.

### `GET /api/v1/metrics/agents`

Aggregate performance metrics across all nous agents.

**Response `200 OK`** - JSON with token usage, latency percentiles, error rates per agent.

---

### `GET /api/v1/metrics/agents/{id}`

Per-agent performance detail.

**Response `200 OK`** - Same schema as `/metrics/agents` scoped to one agent.

---

### `GET /api/v1/metrics/quality`

Quality signal aggregates: recall score distribution, confidence trends, fact churn.

**Response `200 OK`** - JSON quality metrics object.

---

### `GET /api/v1/metrics/tokens`

Token usage summary by agent, session, and model over a configurable time window.

**Response `200 OK`** - JSON with per-dimension token counts.

---

### `GET /api/v1/metrics/costs`

Estimated cost breakdown by provider and model derived from token usage and configured pricing.

**Response `200 OK`** - JSON with cost totals and per-model breakdowns.

---

## Journal

### `GET /api/v1/journal`

Structured audit log of operator actions, config changes, credential rotations, and
significant agent events. Ordered by recency.

**Query parameters:** `limit` (default 100), `since` (ISO-8601 timestamp), `kind` (event type filter).

**Response `200 OK`** - JSON array of journal entries.

---

## Planning

### `GET /api/v1/planning/projects/{project_id}/verification`

Current verification state for a planning project: last run timestamp, passing/failing
checks, and blocking issues.

**Response `200 OK`** - JSON verification state object.

---

### `POST /api/v1/planning/projects/{project_id}/verification/refresh`

Trigger a fresh verification run for the project. Runs synchronously; returns the updated
verification state on completion.

**Response `200 OK`** - Updated verification state.

---

## Routing notes

- Legacy unversioned paths (e.g. `/api/nous`) return **410 Gone** with a migration hint
  pointing to the equivalent `/api/v1/` path.
- Unknown paths return **404 Not Found**.

---

## Keeping this document in sync

`cargo test -p aletheia-pylon -- handler_doc` runs a test that verifies this file contains
an entry for every route registered in `src/router.rs`. Add new routes to both files.
