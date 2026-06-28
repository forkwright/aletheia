# Graceful-Degradation Audit

> Audit of aletheia components where a single local failure causes a global crash.
> Every claim is backed by a `crate/src/file.rs:line` citation.

## Methodology

1. Searched all non-test Rust code for `panic!`, `unreachable!`, `unimplemented!`, `.unwrap()`, and `.expect()`.
2. Read daemon task lifecycle (`crates/daemon/src/runner/`), nous actor patterns (`crates/nous/src/actor/`), and pylon error handling (`crates/pylon/src/`).
3. Traced error propagation from local failure to process boundary for each component.
4. Focused on production code only; `#[cfg(test)]` blocks and `tests/` directories were excluded.

## Nous Actors

### Pipeline turn handling

The actor run loop (`crates/nous/src/actor/mod.rs:270`) processes messages sequentially. Normal turns run inside a **panic boundary**: the pipeline spawns as a separate `tokio::task` and the `JoinHandle` is awaited inside `handle_pipeline_result` (`crates/nous/src/actor/turn.rs:452`).

- If the pipeline panics, the actor catches the `JoinError`, increments `pipeline_panic_count`, and returns an error to the caller. The actor **continues** processing subsequent messages (`turn.rs:460-488`).
- After `degraded_panic_threshold` panics within `degraded_window_secs`, the actor enters `Degraded` mode and rejects new turns with `ServiceDegraded` (`mod.rs:324-328`).

### Cross-nous message handling

**Resolved:** `handle_cross_message` (`mod.rs:459`) now calls `execute_turn_with_panic_boundary` (`mod.rs:470`). Cross-nous pipeline panics are caught as `JoinError` on the same boundary as normal turns; the actor continues processing subsequent messages.

### Background tasks

Background tasks (extraction, distillation, skill analysis) are spawned into a `JoinSet` and reaped each loop iteration (`background.rs:31`). Panics are caught, logged, and counted in `background_panic_count`, but they **do not** trigger degraded mode or actor restart (`background.rs:37-48`).

### Actor restart by manager

The manager's `health_cycle` (`crates/nous/src/manager.rs:721`) pings actors and restarts dead ones with exponential backoff (`crates/nous/src/manager.rs:788-826`). Restart drains the old `JoinHandle` with a timeout taken from `nousBehavior.manager_restart_drain_timeout_secs` (`crates/taxis/src/config/behavior/nous.rs:28`, `crates/nous/src/manager.rs:833-839`). Panics during a full manager `drain` are caught as `JoinError` and logged; they do not propagate (`crates/nous/src/manager.rs:1181-1184`).

### Manager health poller

`NousManager::start_health_poller` (`crates/nous/src/manager.rs:895`) takes an `Arc<NousManager>`, a poll interval, and a `CancellationToken`, and returns a supervisor `JoinHandle`. The supervisor spawns an inner `health_cycle` poller (`crates/nous/src/manager.rs:928`); if that task panics, the supervisor catches the `JoinError`, stores the error in `poller_last_error`, increments `poller_restart_count` and a metric, waits a short backoff, and respawns the inner poller (`crates/nous/src/manager.rs:1284-1325`). The supervisor stops when the cancel token fires. Health checks therefore cannot stop permanently for all actors managed by this manager.

The inner `health_cycle` reads the actor dead-threshold, restart backoff cap, restart drain timeout, and restart-decay window from `nousBehavior` (`crates/taxis/src/config/behavior/nous.rs:24-34`). The supervisor state is exposed through `poller_snapshot()` (`crates/nous/src/manager.rs:1242`) as `ManagerPollerSnapshot` (`crates/nous/src/message.rs:126`), which includes `running`, `restart_count`, and `last_error` for health observers.

Production runtime starts this supervisor after actors are spawned, using `nousBehavior.manager_health_interval_secs`, a shutdown child token, and the runtime `TaskTracker` (`crates/aletheia/src/runtime/mod.rs:716-733`). Pylon detailed health reads `poller_snapshot()` into the `nous_health_poller` check so operators and the desktop service-health panel can see liveness, restart count, and last error (`crates/pylon/src/handlers/health.rs:94-113`, `251-288`).

## Pylon Handlers

### Per-request failure isolation

Pylon builds an Axum router with standard middleware layers (`router.rs:45`). There is **no** `catch_panic` layer installed, but Axum's default behavior aborts the request when a handler panics; it does **not** crash the server process.

Handler errors are converted to HTTP responses via `ApiError::into_response` (`error.rs`). The `JoinError` test confirms that a panicked task inside a handler becomes a `500 Internal Server Error` without leaking internals (`error.rs:754-771`).

### Streaming turn spawns

The streaming handler spawns three concurrent tasks (`streaming.rs:281`, `525`, `570`). The outer handler awaits their completion and maps failures to SSE error events. Panics in these tasks are caught as `JoinError` and do not escape the request.

### Signal handler startup

**Resolved:** `spawn_sighup_handler` (`server.rs:428`) and `shutdown_signal_with` (`server.rs:508`) now treat signal-installation failures as warnings and continue serving without that signal path. SIGHUP handler installation returning `None` simply disables config reload on signal; Ctrl+C or SIGTERM installation failures cause the corresponding future to pend forever rather than panic. The `.expect()` calls cited in the 2026-04 audit have been removed.

## Daemon Workers

### Task runner lifecycle

The daemon `TaskRunner` ticks every second and spawns tasks via `tokio::spawn` (`lifecycle.rs:149`). Each `JoinHandle` is stored in `in_flight` and polled individually in `check_in_flight` (`inflight.rs:37`).

- **Errors:** Logged and recorded as task failures (`inflight.rs:77-85`).
- **Panics:** Caught as `JoinError`, logged, and recorded as failures (`inflight.rs:92-94`).
- **Isolation:** One task's failure does **not** affect the runner loop or other tasks.
- **Auto-disable:** After 3 consecutive failures a task is automatically disabled (`tracking.rs:63`).

### Maintenance Tasks

Maintenance builtins (trace rotation, drift detection, whole-instance backup, etc.) are dispatched via `tokio::task::spawn_blocking` inside the action (`execution.rs:145`). Blocking-task panics are caught by the same `check_in_flight` path and do not propagate.

### Graceful Shutdown

Shutdown uses a `CancellationToken` tree: a root token with child tokens per runner (`runtime/mod.rs:661`). On shutdown, in-flight tasks are aborted (`lifecycle.rs:68`) and the binary-level `TaskTracker` waits with a 10-second timeout (`server/mod.rs:231`).

## Session Store

Session state lives in a `mneme::store::SessionStore` wrapped in a `tokio::sync::Mutex` (`manager.rs:79`). Tokio mutexes **do not poison** on panic, so a panic during session CRUD cannot corrupt the store for other actors. Errors are returned as `Result` and propagated up the call stack.

## Knowledge Store

The knowledge store (`mneme::knowledge_store::KnowledgeStore`) is accessed through `Arc<KnowledgeStore>` shared across actors. It is not guarded by a mutex at the actor level; the underlying `fjall` key-value store handles concurrency. Failures return errors. There is no `unwrap()` or `expect()` in the production knowledge-store code path that would crash the process.

## Provider Registry

The `ProviderRegistry` (`hermeneus/src/provider.rs:280`) holds a `Vec<ProviderEntry>`. Each entry carries a `ProviderHealthTracker` (`health.rs:82`).

- **Per-provider health:** `record_success` / `record_error` update state machine (`Up → Degraded → Down`). A single provider failure does not affect other providers.
- **Mutex poisoning handled gracefully:** `health.rs:103-107` uses `unwrap_or_else(PoisonError::into_inner)` to recover stale state.

**Resolved:** The `ConcurrencyLimiter` (`hermeneus/src/concurrency.rs`) now uses `parking_lot::Mutex` (`concurrency.rs:24`), which does not poison on panic. The `.expect()` calls on mutex locking are eliminated; a panic while holding the lock no longer crashes subsequent LLM requests.

## Tool Executors

Tool execution runs inside `organon` builtins. The sandbox module (`organon/src/sandbox/policy.rs`) applies Landlock/seccomp on Linux and is a no-op elsewhere. Tool errors are returned as `Result` to the caller; panics inside a tool would be caught by the nous pipeline's panic boundary. There is no `unwrap()` in the production tool-execution path that would crash the process.

## Maintenance Tasks

See [Daemon Workers](#daemon-workers). Maintenance tasks inherit the same isolation: per-task `JoinHandle` polling, panic catching, and auto-disable after repeated failures.

## Surface List - Components That Crash the Process

No audited production component currently turns a documented local failure into
a process-wide crash. The formerly open crash surfaces are resolved:

1. ~~Krites Datalog engine~~ - **resolved**: float JSON export uses `f64::classify()` for exhaustive handling without `unreachable!`; query-planning invariant violations return typed errors.
2. ~~Pylon signal handler setup~~ - **resolved** (`server.rs:428`, `508`).
3. ~~Nous manager health poller~~ - **resolved** (`crates/nous/src/manager.rs:895`).

## Summary Table

| Component | Failure Mode | Blast Radius | Ideal Behavior | Current Gap |
|---|---|---|---|---|
| Nous actor - normal turn | Pipeline panic caught as `JoinError` | Request | Actor continues, enter degraded mode if repeated | ✅ None |
| Nous actor - cross-nous message | Pipeline panic caught as `JoinError` via `execute_turn_with_panic_boundary` | Request | Actor continues, enter degraded mode if repeated | ✅ None (resolved: `actor/mod.rs:470`) |
| Nous manager health poller | Supervisor respawns panicked inner poller | All actors managed | Supervisor restarts poller on failure | ✅ None (resolved: `crates/nous/src/manager.rs:895`) |
| Daemon task runner | `JoinError` caught per-task in `check_in_flight` | Task | Task disabled after 3 failures | ✅ None |
| Daemon maintenance tasks | `spawn_blocking` panics caught as `JoinError` | Task | Same isolation as async tasks | ✅ None |
| Session store | `tokio::sync::Mutex` - no poisoning | Request | Errors returned to caller | ✅ None |
| Knowledge store | `fjall` internal concurrency, errors returned | Request | Errors returned to caller | ✅ None |
| Provider registry | Per-provider health tracker | Provider | Failed provider marked Down, others unaffected | ✅ None |
| Hermeneus concurrency limiter | `parking_lot::Mutex` - no poisoning | Process | No-poison mutex; limiter survives panics | ✅ None (resolved: `concurrency.rs:24`) |
| Hermeneus health tracker | `unwrap_or_else(PoisonError::into_inner)` | Component | Recovers stale state, continues | ✅ None |
| Tool executors | Errors returned; sandbox no-op on non-Linux | Request | Isolate and return errors | ✅ None |
| Pylon signal handlers | Signal-installation failures log warnings | Signal path | Continue serving without the failed signal path | ✅ None (resolved: `server.rs:428`, `508`) |
| Pylon streaming handlers | Spawned tasks awaited, `JoinError` mapped to SSE | Request | Per-request failure isolation | ✅ None |
| Krites query engine | Float JSON export uses exhaustive `f64::classify()`; query-planning invariants return typed errors | Request | No panic from external input/storage state | ✅ None (resolved: `data/json.rs:55`) |
| Krites query engine - resolved planning invariants | Internal invariant violations during query planning now return typed errors | Process | Return `Result` error to caller | ✅ None (resolved: `query/graph.rs:127`, `query/graph.rs:205`, `query/magic.rs:217`, `query/stratify.rs:203`) |
| Krites storage backend | Assumed-live transaction access now returns a typed error | Process | Return corruption/error instead of panic | ✅ None (resolved: `storage/fjall_backend.rs:194`) |

## Detailed citation index

### Krites
- ~~`crates/krites/src/data/json.rs:65` - `unreachable!` on the f64 IEEE 754 exhaustiveness invariant during JSON export.~~ Resolved: float JSON export now matches `f64::classify()` exhaustively without a panic branch, and `DataValue::Bot` is handled in the `Null | Bot` arm and emitted as `JsonValue::Null`.
- ~~`crates/krites/src/query/graph.rs:127` - empty `safe_pending` stack in topological sort.~~ Resolved: now returns a typed error.
- ~~`crates/krites/src/query/graph.rs:205` - missing `ids[at]` entry in SCC computation.~~ Resolved: now returns a typed error.
- ~~`crates/krites/src/query/magic.rs:217` - defaulted `MagicRulesOrFixed` in query rewrite.~~ Resolved: now returns a typed error.
- ~~`crates/krites/src/query/stratify.rs:203` - missing SCC index for a graph key.~~ Resolved: now returns a typed error.
- ~~`crates/krites/src/storage/fjall_backend.rs:194` - assumed-live `tx` in `FjallWriteTx`.~~ Resolved: now returns a typed error.

### Hermeneus
- `crates/hermeneus/src/concurrency.rs:24` - `parking_lot::Mutex` (resolved: replaces `std::sync::Mutex`; no poisoning on panic).

### Nous
- `crates/nous/src/actor/mod.rs:470` - `handle_cross_message` calls `execute_turn_with_panic_boundary` (resolved: previously called `execute_turn` inline).

### Pylon
- ~~`crates/pylon/src/server.rs:365` - `.expect("failed to install SIGHUP handler")`.~~ Resolved: handler now returns `None` on failure.
- ~~`crates/pylon/src/server.rs:413` - `.expect("failed to install ctrl+c handler")`.~~ Resolved: Ctrl+C failure logs a warning and the future pends.
- ~~`crates/pylon/src/server.rs:419` - `.expect("failed to install SIGTERM handler")`.~~ Resolved: SIGTERM failure logs a warning and the future pends.

### Nous
- `crates/nous/src/manager.rs:895` - `start_health_poller` supervises the inner poller and respawns it on panic (resolved).
- `crates/nous/src/manager.rs:1242` - `poller_snapshot()` exposes supervisor liveness, restart count, and last error.
- `crates/nous/src/message.rs:126` - `ManagerPollerSnapshot` defines the poller state shape.
- `crates/taxis/src/config/behavior/nous.rs:24-34` - manager health/restart thresholds and intervals come from `nousBehavior`.

### Daemon
- `crates/daemon/src/runner/inflight.rs:92` - `JoinError` from panics caught and logged.
- `crates/daemon/src/runner/tracking.rs:63` - Auto-disable after 3 consecutive failures.
- `crates/daemon/src/runner/lifecycle.rs:149` - Tasks spawned via `tokio::spawn` with isolated handles.
