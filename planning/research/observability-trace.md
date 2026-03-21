# R721: Observability and Trace Architecture

## Question

How is tracing implemented in aletheia? What spans, events, and outputs exist? How should operators configure, debug with, and extend the tracing system?

## Findings

### 1. tracing stack

Aletheia uses the `tracing` ecosystem exclusively:

| Crate | Version | Purpose |
|-------|---------|---------|
| `tracing` | 0.1 | Span/event macros across all crates |
| `tracing-subscriber` | 0.3 | Subscriber registry, `EnvFilter`, JSON formatter |
| `tracing-appender` | 0.2 | Daily-rolling file appender, non-blocking writer |
| `flate2` | 1 | Gzip compression for rotated trace files |

No OpenTelemetry, OTLP, or Langfuse integration exists today.

### 2. initialization

Two initialization paths exist:

**Server mode** (`crates/aletheia/src/commands/server.rs:863-920`):
- Dual-layer subscriber: console + file
- Console: text or JSON (`--json-logs` flag), level from `RUST_LOG` or `--log-level`
- File: always JSON, daily rolling (`aletheia.log.YYYY-MM-DD`), level from `logging.level` config
- Non-blocking file writer via background thread; `WorkerGuard` kept alive for process lifetime

**Minimal mode** (`crates/koina/src/tracing_init.rs`):
- Single-layer stdout subscriber (text or JSON)
- Used by CLI commands and tests
- Default filter: `aletheia=info,warn`

**TUI mode** (`crates/theatron/tui/src/lib.rs:57-79`):
- File-only output to `~/.local/share/aletheia/tui.log`
- Daily rolling, no console output (TUI owns the terminal)

### 3. span hierarchy

The system implements a layered span tree. Every spawned async task uses `.instrument(span)` to propagate context.

```
http_request (method, path, request_id, status_code)
  |
  +-- send_turn (session.id, session.key, nous.id, request_id, idempotency_key)
        |
        +-- nous_actor (nous.id)
              |
              +-- pipeline (nous_id, session_id, pipeline.model,
              |             pipeline.total_duration_ms, pipeline.stages_completed,
              |             pipeline.tool_calls)
              |     |
              |     +-- pipeline_stage (stage, duration_ms, status)
              |     |     stage = context | recall | history | guard | execute | finalize
              |     |
              |     +-- llm_call (llm.provider, llm.model, llm.duration_ms,
              |           llm.tokens_in, llm.tokens_out, llm.status, llm.retries, llm.stream)
              |
              +-- extraction (nous.id)
              +-- skill_extraction (nous.id, candidate.id)
              +-- distillation (nous.id, session.id)
```

Other top-level spans:

| Span | Location | Fields |
|------|----------|--------|
| `message_dispatcher` | `crates/aletheia/src/dispatch.rs:24` | (none) |
| `dispatch` | `crates/aletheia/src/dispatch.rs:38` | `channel`, `sender` |
| `daemon_runner` | `crates/aletheia/src/commands/server.rs:385` | (none) |
| `task_execute` | `crates/daemon/src/runner.rs:636` | `task_id`, `task_name`, `nous_id` |
| `sse_bridge` | `crates/pylon/src/handlers/sessions/streaming.rs:364` | (none) |
| `credential_refresh` | `crates/symbolon/src/credential.rs:456` | (none) |
| `shutdown_signal` | `crates/pylon/src/server.rs:204` | (none) |
| `log_retention` | `crates/aletheia/src/commands/server.rs:850` | (none) |
| `health_poller` | `crates/nous/src/manager.rs:407` | (none) |

### 4. correlation IDs

Three primary identifiers flow through the span tree:

| ID | Format | Created at | Present in |
|----|--------|-----------|------------|
| `request_id` | ULID | `pylon/src/middleware.rs:72` | `http_request`, `send_turn`, error responses |
| `nous_id` / `nous.id` | Config string | Nous config | `nous_actor`, `pipeline`, `task_execute`, background tasks |
| `session_id` / `session.id` | String | Session store | `send_turn`, `pipeline`, `distillation` |

Error responses (4xx/5xx) are enriched with `request_id` by the `enrich_error_response` middleware (`pylon/src/middleware.rs:77-122`). This allows operators to correlate a user-visible error to its trace output.

### 5. key events

**Lifecycle events:**
- `"actor started"` (info) with nous.id
- `"daemon started"` / `"daemon shutting down"` (info)
- `"registered task"` (info) with task metadata

**Turn events:**
- `"turn_completed"` (info) with `input_tokens`, `output_tokens`, `tool_calls_count`, `duration_ms`, `model`

**Error events:**
- `"turn failed"` (error) in streaming handler
- `"background task panicked"` (warn) in actor background tasks
- `"task failed"` (warn) in daemon runner with error context

**Maintenance events:**
- `"maintenance: trace rotation complete"` (info) with files_rotated, files_pruned
- `"maintenance: drift detection complete"` (info)
- `"maintenance: retention complete"` (info)

### 6. instrumentation coverage

~60 functions carry `#[instrument]` attributes across the codebase. Key areas:

| Crate | Functions instrumented | Notes |
|-------|----------------------|-------|
| `mneme` (store) | ~20 | All session/message CRUD, recall |
| `pylon` (handlers) | ~10 | All HTTP handler functions |
| `symbolon` (auth) | ~7 | Auth, JWT, API key operations |
| `nous` (pipeline) | ~6 | Pipeline, instinct, distillation |
| `nous` (cross) | ~5 | Cross-nous message routing |
| `melete` | ~1 | Distillation |

All production `tokio::spawn` calls use `.instrument(span)` for context propagation. Uninstrumented spawns exist only in test code.

### 7. configuration

**TOML config** (`aletheia.toml`):

```toml
[logging]
# Directory for daily-rolling JSON log files.
# Relative to instance root. Default: {instance}/logs/
logDir = "/var/log/aletheia"

# Days to retain before background cleanup. Default: 14
retentionDays = 30

# Minimum level for file output. Default: "warn"
# Accepts tracing directives: "warn", "error", "aletheia=debug,warn"
level = "aletheia=debug,warn"

[maintenance.traceRotation]
enabled = true         # Default: true
maxAgeDays = 14        # Default: 14
maxTotalSizeMb = 500   # Default: 500
compress = true        # Default: true (gzip)
maxArchives = 30       # Default: 30
```

**Environment variables:**
- `RUST_LOG`: overrides console filter (`aletheia_nous=trace,aletheia_pylon=debug,warn`)
- `ALETHEIA_LOGGING__LOG_DIR`: override log directory
- `ALETHEIA_LOGGING__LEVEL`: override file log level
- `ALETHEIA_LOGGING__RETENTION_DAYS`: override retention

**CLI flags** (server command):
- `--log-level <level>`: console level (default: info)
- `--json-logs`: switch console output to JSON

### 8. file rotation

The `TraceRotator` (`crates/daemon/src/maintenance/trace_rotation.rs`) runs as a background daemon task:

1. Scans `logs/traces/` for files older than `max_age_days`
2. Moves to `logs/traces/archive/`, optionally gzip-compresses
3. Prunes oldest archives beyond `max_archives` count
4. Creates replacement empty file at original path (allows active writers to finish on old inode)
5. Runs at server startup and every 24 hours

Log retention (separate from trace rotation) prunes daily log files in `logs/` after `retention_days`.

### 9. debugging workflow

To trace a user-visible error back to its cause:

1. **Get the request ID** from the error response body (`error.request_id` field)
2. **Search log files** for the ULID: `grep <request_id> logs/aletheia.log.*`
3. The `http_request` span contains method, path, status code, and duration
4. Child spans (`send_turn`, `pipeline`, `llm_call`) show the full execution path
5. Pipeline stage spans reveal which stage failed and its duration

To increase verbosity for a running server:
- Restart with `RUST_LOG=aletheia=debug` for all crates at debug level
- Target specific crates: `RUST_LOG=aletheia_nous=trace,aletheia_hermeneus=debug,warn`
- File output can be set independently: `logging.level = "aletheia=debug,warn"` in TOML

To increase verbosity without restart: not supported (requires process restart).

### 10. metrics (Prometheus)

Separate from tracing, Prometheus metrics are exposed at `/metrics` (`crates/pylon/src/handlers/metrics.rs`). HTTP request count and duration are recorded by the `record_http_metrics` middleware. Turn completion triggers `crate::metrics::record_turn()`.

## Recommendations

### R1: install a structured panic handler (high priority)

`RUST.md` mandates a custom panic hook that logs to the structured log file. No panic handler is installed today. A panic in any async task silently disappears unless the `JoinHandle` is awaited and the `JoinError` explicitly logged. The daemon runner does catch task panics, but a panic on the main thread or in non-runner tasks would only hit stderr.

```rust
std::panic::set_hook(Box::new(|info| {
    tracing::error!(panic = %info, "process panicked");
}));
```

This belongs in `init_tracing()` or immediately after it in the server startup path.

### R2: add openTelemetry export layer (medium priority)

The current file-based JSON output is adequate for single-instance debugging but does not support:
- Distributed trace correlation across instances
- Trace visualization (flame graphs, waterfall views)
- Alerting on span duration thresholds

Adding an optional OTLP export layer would enable operators to ship traces to Jaeger, Grafana Tempo, or Datadog. The `tracing-opentelemetry` crate integrates directly with the existing `tracing_subscriber::registry()` as an additional layer, requiring no changes to existing instrumentation.

Config shape:

```toml
[telemetry.otlp]
enabled = false
endpoint = "http://localhost:4317"
service_name = "aletheia"
```

### R3: add langfuse integration for LLM observability (medium priority)

The `llm_call` spans already capture provider, model, token counts, duration, and retry counts. A Langfuse layer or post-processing step could export these as Langfuse generations, enabling:
- Cost tracking per session/agent
- Latency percentile monitoring per model
- Prompt/completion logging for evaluation

Two approaches:
1. **Tracing layer**: custom `tracing::Layer` that filters `llm_call` spans and exports to Langfuse API. Tight integration, real-time.
2. **Log post-processor**: parse JSON log files and batch-export to Langfuse. Simpler, decoupled, but delayed.

Approach 1 is preferred for production use. The Langfuse Rust SDK does not exist; the HTTP API would need a thin client.

### R4: support runtime log level changes (low priority)

Changing log levels requires a process restart. `tracing-subscriber` supports `reload::Layer` which allows swapping the `EnvFilter` at runtime via an API endpoint or signal handler. This would allow operators to increase verbosity for debugging without downtime.

### R5: add `tool_execute` span in organon (low priority)

The span hierarchy shows `pipeline_stage(stage="execute")` containing `llm_call` spans, but individual tool executions within the execute stage do not have their own spans. Adding a `tool_execute` span with `tool_name`, `tool_id`, and `duration_ms` fields would close the gap between "the LLM asked to call a tool" and "the tool returned a result."

### R6: add span fields to `sse_bridge` and `credential_refresh` (low priority)

These spans carry no identifying fields, making them hard to correlate in multi-session environments. `sse_bridge` should carry `session.id`; `credential_refresh` should carry the credential type and user context.

## Gotchas

1. **`pipeline_span.enter()` guard**: `pipeline.rs:401` uses `.enter()` instead of `.instrument()`. This is safe because the guard is held synchronously across the sequential pipeline stages, but adding an `.await` between the guard creation and drop would silently break span correlation. The code is correct today but fragile.

2. **Console and file levels are independent**: an operator setting `RUST_LOG=debug` will get verbose console output but file output remains at the config-specified level (default `warn`). This is by design but may confuse operators expecting file output to match console.

3. **No dynamic reload**: changing `logging.level` in TOML requires a restart. `RUST_LOG` is read once at startup.

4. **JSON field naming**: span fields use mixed conventions. HTTP spans use `http.method` (dotted), pipeline spans use `pipeline.total_duration_ms` (dotted), LLM spans use `llm.provider` (dotted), but the top-level turn event uses flat `input_tokens`. The dotted convention is preferred for structured log querying.

5. **`WorkerGuard` lifetime**: the non-blocking file writer flushes on `WorkerGuard` drop. If the guard is dropped prematurely (e.g., moved into a struct that is dropped early), final log events are lost. The current code correctly holds the guard in the server `run()` function scope.

6. **Trace rotation vs log retention**: two separate systems handle file cleanup. `TraceRotator` targets `logs/traces/`; log retention targets `logs/`. The daily-rolling appender writes to `logs/`. If `logs/traces/` is not populated by anything, trace rotation runs as a no-op. Operators may confuse these.

## References

| Item | Location |
|------|----------|
| Server tracing init | `crates/aletheia/src/commands/server.rs:863-920` |
| Simple tracing init | `crates/koina/src/tracing_init.rs` |
| TUI tracing init | `crates/theatron/tui/src/lib.rs:57-79` |
| LoggingSettings struct | `crates/taxis/src/config.rs:782-809` |
| TraceRotationConfig | `crates/daemon/src/maintenance/trace_rotation.rs:12-43` |
| HTTP trace layer | `crates/pylon/src/router.rs:125-152` |
| Request ID middleware | `crates/pylon/src/middleware.rs:66-122` |
| Pipeline spans | `crates/nous/src/pipeline.rs:393-472` |
| LLM call spans | `crates/hermeneus/src/anthropic/client.rs:176-189, 456-468` |
| Actor spawn | `crates/nous/src/actor/spawn.rs:81-82` |
| Background task spans | `crates/nous/src/actor/background.rs:59-81, 153-174, 242-255` |
| Daemon task execution | `crates/daemon/src/runner.rs:636-655` |
| Dispatcher spans | `crates/aletheia/src/dispatch.rs:24-47` |
| Config reference doc | `docs/CONFIGURATION.md:372-388` |
| Rust tracing standard | `standards/RUST.md` (Logging section) |
| Universal logging standard | `standards/STANDARDS.md` (Logging and Observability section) |
