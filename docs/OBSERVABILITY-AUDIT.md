# Observability Contracts Audit

**Audit date:** 2026-04-16
**Method:** Static analysis - grep for `#[instrument]`, `tracing::instrument`, Prometheus metric registrations (`register_int_counter_vec!` / `register_histogram_vec!` / etc.), and structured `warn!`/`error!` call sites across all workspace crates. Checked entry points for the four user-facing crates: pylon, nous, hermeneus, organon.
**Closes:** #3259

---

## 1. Tracing spans (`#[instrument]`)

### Coverage by crate

| Crate | Entry points with `#[instrument]` | Notes |
|-------|----------------------------------|-------|
| **pylon** | sessions (create, get, list, delete, update, restore, send, streaming), config (get/set/reload), knowledge bulk_import | `nous.rs` handlers (list, get_status, tools, recover) have **no span** - gap |
| **nous** | actor run loop, pipeline stages, finalize, recall, execute (both paths), instinct (tool dispatch), cross-router | Good coverage across the hot path |
| **hermeneus** | anthropic client complete + complete_streaming, cc process (two fns), stream accumulator, error classifier, fallback | Core LLM call paths all instrumented |
| **organon** | triage tool, `ToolRegistry::execute` | `ToolRegistry::execute` now creates a `tool_execute` span (`crates/organon/src/registry/mod.rs:174`). Individual tool executors still vary in instrumentation. |
| daemon | execution, watchdog, cron jobs, prosoche, self_prompt, lifecycle hooks | Good coverage |
| episteme | consolidation engine, embedding, knowledge store ops | Good coverage |
| agora | semeion client | Covered |
| graphe | store read/write ops | Covered |
| symbolon | auth, jwt, credential refresh/pkce | Covered |
| diaporeia | auth, transport, resource config | Covered |

### Gaps

**GAP-SPAN-1 (pylon/nous.rs):** `list`, `get_status`, `tools`, `recover` handlers have no `#[instrument]`. These are user-facing nous management endpoints - missing spans means no distributed trace context for nous listing/recovery operations.

**GAP-SPAN-2 (organon/ToolRegistry):** Resolved. `ToolRegistry::execute` now creates a `tool_execute` span with `tool.name`, `tool.reversibility`, `tool.approval`, `tool.duration_ms`, and `tool.status` fields (`crates/organon/src/registry/mod.rs:174`). Individual tool executors still vary in instrumentation; the remaining gap is per-tool coverage, not the dispatch span.

---

## 2. Prometheus metrics

### Registered metric sets

| Crate | Metrics file | Metrics registered |
|-------|-------------|-------------------|
| **pylon** | `src/metrics.rs` | `aletheia_http_requests_total` (counter, labels: method/path/status), `aletheia_http_request_duration_seconds` (histogram), `aletheia_active_sessions` (gauge), `aletheia_uptime_seconds` (gauge) |
| **nous** | `src/metrics.rs` | `aletheia_pipeline_turns_total`, `aletheia_pipeline_stage_duration_seconds`, `aletheia_pipeline_errors_total`, `aletheia_cache_read_tokens_total`, `aletheia_cache_creation_tokens_total`, `aletheia_background_task_failures_total`, `aletheia_tool_failures_total`, `aletheia_stream_events_dropped_total`, `aletheia_nous_inbox_saturation_total` |
| **hermeneus** | `src/metrics.rs` | `aletheia_llm_tokens_total`, `aletheia_llm_cost_total`, `aletheia_llm_requests_total`, `aletheia_llm_cache_tokens_total`, `aletheia_llm_request_duration_seconds`, `aletheia_llm_ttft_seconds`, `aletheia_llm_circuit_breaker_transitions_total`, `aletheia_llm_concurrency_limit`, `aletheia_llm_concurrency_latency_ewma_seconds`, `aletheia_llm_concurrency_in_flight` |
| **organon** | `src/metrics.rs` | `aletheia_tool_invocations_total` (counter, labels: tool_name/status), `aletheia_tool_duration_seconds` (histogram) |
| daemon | `src/metrics.rs` | (not audited above - see §5) |
| dianoia | `src/metrics.rs` | (not audited above - see §5) |
| energeia | `src/metrics/prometheus.rs` | (not audited above - see §5) |
| episteme | `src/metrics.rs` | (not audited above - see §5) |
| graphe | `src/metrics.rs` | (not audited above - see §5) |
| melete | `src/metrics.rs` | (not audited above - see §5) |
| symbolon | `src/metrics.rs` | (not audited above - see §5) |

### Coverage against required signal categories

| Signal | Present | Where | Notes |
|--------|---------|-------|-------|
| **Request count** | Yes | `aletheia_http_requests_total` (pylon) | Labels: method, path (normalized), status |
| **Latency** | Yes | `aletheia_http_request_duration_seconds` (pylon), `aletheia_llm_request_duration_seconds` (hermeneus), `aletheia_pipeline_stage_duration_seconds` (nous), `aletheia_tool_duration_seconds` (organon) | End-to-end + per-subsystem |
| **Error rate** | Yes (derivable) | `aletheia_http_requests_total[status]`, `aletheia_pipeline_errors_total`, `aletheia_llm_requests_total[status]`, `aletheia_tool_invocations_total[status]` | Error rate = `rate(counter{status="5xx"})` / `rate(counter)`. No dedicated error-rate gauge - that's correct for Prometheus. |
| **Active sessions** | Yes | `aletheia_active_sessions` (pylon) | Updated via `update_system_gauges` |
| **Queue saturation** | Yes | `aletheia_nous_inbox_saturation_total` (nous) | Counts bounded actor inbox send timeouts that become `InboxFull`/`service_busy` |
| **Queue depth** | **No** | - | No metric tracks the NousActor inbox channel depth. `session_count` per actor is polled by the manager but not exposed as a Prometheus gauge. |

### Gaps

**GAP-METRIC-1 (queue depth):** No `aletheia_nous_inbox_depth` gauge or histogram. Saturation that reaches a send timeout is visible through `aletheia_nous_inbox_saturation_total`, but exact queue depth is still not sampled from the bounded actor inbox.

**GAP-METRIC-2 (nous init - `dead_code` note):** `nous/src/metrics.rs` `init()` is annotated `#[cfg_attr(not(test), expect(dead_code, ...))]` with the comment "startup pre-registration, not yet wired into server boot sequence." This means nous metrics are lazy-initialized on first use instead of pre-registered. For counters that works, but a freshly-started server that has never handled a turn will show nothing on the `/metrics` endpoint for nous metrics until the first event fires.

---

## 3. Structured warn/error log coverage

### Critical failure paths - coverage confirmed

| Path | Level | Key events logged |
|------|-------|------------------|
| Auth rejection (diaporeia) | `warn` | Missing/malformed Authorization header, jwt_manager unavailable, invalid Bearer token |
| RBAC denial (diaporeia) | `warn` | Resource RBAC denied (no role resolved), MCP RBAC denied |
| Rate limiter poison recovery | `warn` | Lock poisoned - recovery logged |
| LLM stream errors (diaporeia) | `warn`/`error` | SSE transport send failures |
| Actor health miss / kill (nous) | `warn`/`error` | Actor missed health check, unresponsive kill |
| Manager shutdown failure | `error` | Actor shutdown send failed, tracker task failed |
| Nous finalize (tool serialize fail) | `warn` | Failed to serialize tool call input |
| Recall / skill search fail | `warn` | Skill search failed, task panicked, failed to increment skill access |
| Research spawn failures | `warn` | Researcher timed out, researcher failed, spawn failed, task panicked |
| Bootstrap cascade warnings | `warn` | Section too large to pack, section skipped |
| Daemon watchdog | `warn`/`error` | Watchdog trigger missed, task max retries exceeded |
| Inflight tracking | `warn`/`error` | Ack past deadline, result send failed, join error |
| Krites HNSW | `warn` | Degree exceeded in put operations |
| Krites transact | `error` | Cleanup of persisted range failed, lock poison recovery |

### Gaps

**GAP-LOG-1 (pylon nous.rs handlers):** The `recover` handler restarts a nous actor. There is no `warn!/error!` on failure paths in this handler - if the recovery call returns an error it propagates as an `ApiError` (HTTP 500) without a structured log event with context about which nous actor failed and why.

**GAP-LOG-2 (pylon planning.rs):** `get_verification` and `refresh_verification` handlers load planning verification state and return structured API errors. They no longer return `501 Not Implemented`; the remaining gap is sparse structured logging on load failures beyond the propagated `ApiError`.

---

## 4. Per-crate entry point audit (pylon, nous, hermeneus, organon)

### pylon

Entry points are HTTP handlers in `src/handlers/`. All session handlers are instrumented. Gaps:

| Handler | Instrumented | Logged on error |
|---------|-------------|----------------|
| `sessions/mod.rs` - all 7 session management fns | Yes | Via `ApiError` mapping |
| `sessions/streaming.rs` - `send_message`, `replay` | Yes | Yes (stream drop events tracked by nous metrics) |
| `handlers/config.rs` - get/set/reload | Yes | Yes |
| `handlers/knowledge/bulk_import.rs` | Yes | Yes |
| **`handlers/nous.rs` - list, get_status, tools, recover** | **No** | Recover has no structured warn/error |
| `handlers/health.rs` - health check | No (intentional: health is high-freq, spans would add noise) | N/A |
| `handlers/metrics.rs` - Prometheus scrape endpoint | No (intentional) | N/A |
| `handlers/planning.rs` - get/refresh verification | Partial | Via `ApiError` mapping; sparse structured logs |

### nous

All pipeline stages instrumented. Hot path coverage:

| Stage | Instrumented |
|-------|-------------|
| Actor run loop | Yes (`#[instrument]` on `run`) |
| `handle_turn` (tokio::select!) | Not a public fn; covered by actor span |
| Bootstrap | Yes (4 `#[instrument]` functions) |
| Skills | No dedicated span, called inside instrumented execute |
| Recall | Yes |
| History | No span, called inside instrumented execute |
| Execute (LLM call) | Yes (2 paths) |
| Finalize | Yes |
| Cross-nous router | Yes (3 spans) |
| Distillation | Yes |

### hermeneus

All critical LLM paths instrumented. The `AnthropicProvider::complete` and `complete_streaming` methods both have `#[tracing::instrument]`. Fallback and concurrency management also instrumented.

### organon

`ToolRegistry::execute` now creates a `tool_execute` span (`registry/mod.rs:174`), so the dispatch path is instrumented. Individual tool executors (filesystem, git, memory, etc.) still vary in whether they add their own `#[instrument]`; the remaining gap is per-tool coverage, not the central dispatch span.

---

## 5. Summary - critical gaps

| ID | Severity | Gap | Recommended fix |
|----|----------|-----|----------------|
| GAP-SPAN-1 | Medium | `pylon/nous.rs` handlers missing `#[instrument]` | Add `#[instrument(skip(state, _claims))]` to `list`, `get_status`, `tools`, `recover` |
| GAP-SPAN-2 | Medium | ~~organon/ToolRegistry::execute missing span~~ | Resolved: `tool_execute` span is created in `registry/mod.rs:174` |
| GAP-METRIC-1 | Medium | No queue-depth metric for NousActor inboxes | Add `aletheia_nous_inbox_depth` gauge if operators need pre-timeout depth alerting |
| GAP-METRIC-2 | Low | `nous::metrics::init()` not wired to server boot | Call `nous::metrics::init()` alongside `pylon::metrics::init()` at server startup |
| GAP-LOG-1 | Medium | `pylon/nous.rs` `recover` handler lacks structured error log | Add `error!(nous_id = %id, error = %e, "nous recovery failed")` on error path |

---

## 6. What is working well

- HTTP layer metrics are complete and correct: request count, latency histogram with normalized paths (no label explosion), error status as a label, and active sessions gauge.
- LLM provider metrics are thorough: tokens, cost, latency, TTFT, circuit breaker transitions, adaptive concurrency in-flight and limit.
- Pipeline error classification (`pipeline_errors_total` with stage + error_type labels) enables per-stage alerting.
- Background failure counters (`background_task_failures_total`, `stream_events_dropped_total`, `tool_failures_total`) cover the most common silent data-loss paths.
- All security-relevant events (auth rejection, RBAC denial, rate limit poisoning) are at `warn` with structured fields.
- Sandbox and systemd code is correctly platform-gated - no macOS build breaks from Linux-only security features.
