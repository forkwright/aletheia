# daemon (oikonomos)

Per-nous background task runner: cron scheduling, maintenance services, prosoche attention, watchdog. 6K lines.

## Read first

1. `src/runner.rs`: TaskRunner (main run loop, task dispatch, failure tracking, backoff)
2. `src/schedule.rs`: TaskDef, Schedule, TaskAction, BuiltinTask (scheduling primitives)
3. `src/bridge.rs`: DaemonBridge trait (decoupled nous communication)
4. `src/maintenance/mod.rs`: MaintenanceConfig, aggregated maintenance sub-modules
5. `src/prosoche.rs`: ProsocheCheck (periodic attention check-in)
6. `src/watchdog.rs`: Watchdog (process heartbeat monitoring and auto-recovery)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `TaskRunner` | `runner.rs` | Per-nous task scheduler with cron, interval, one-shot, and startup modes |
| `TaskDef` | `schedule.rs` | Task definition: id, schedule, action, timeout, active window |
| `Schedule` | `schedule.rs` | When to run: Cron, Interval, Once, Startup |
| `BuiltinTask` | `schedule.rs` | Built-in task enum: Prosoche, TraceRotation, DriftDetection, DbSizeMonitor, etc. |
| `DaemonBridge` | `bridge.rs` | Trait for sending prompts to nous without direct dependency |
| `ProsocheCheck` | `prosoche.rs` | Periodic attention check: calendar, tasks, system health |
| `Watchdog` | `watchdog.rs` | Heartbeat tracker with configurable timeout and auto-restart |
| `MaintenanceConfig` | `maintenance/mod.rs` | Aggregated config for trace rotation, drift, DB monitoring, retention, knowledge |
| `TraceRotator` | `maintenance/trace_rotation.rs` | Rotate and gzip-compress old trace files |
| `DriftDetector` | `maintenance/drift_detection.rs` | Compare live instance against template for config drift |
| `DbMonitor` | `maintenance/db_monitor.rs` | Database file size monitoring with warning/alert thresholds |
| `TaskStateStore` | `state.rs` | SQLite-backed persistence for task execution state across restarts |

## Patterns

- **Bridge decoupling**: `DaemonBridge` trait implemented in binary crate, avoids daemon -> nous dependency.
- **Failure backoff**: consecutive failures trigger exponential backoff; `backoff_delay()` caps at configurable max.
- **Active windows**: tasks can be restricted to time-of-day ranges (e.g., prosoche only during waking hours).
- **Catch-up**: missed cron windows within 24h are executed on startup (configurable per task).
- **In-flight tracking**: tasks run as spawned tokio tasks, tracked for timeout detection.
- **Cron tasks**: evolution (config variant search), reflection (self-evaluation), graph cleanup (orphan removal).

## Common tasks

| Task | Where |
|------|-------|
| Add built-in task | `src/schedule.rs` (BuiltinTask enum) + `src/execution.rs` (handler) + `src/runner.rs` (registration) |
| Add maintenance service | New file in `src/maintenance/`, add config field to `MaintenanceConfig` |
| Add cron task | New file in `src/cron/`, add config to `CronConfig` |
| Modify prosoche checks | `src/prosoche.rs` (AttentionCategory enum, check logic) |
| Modify watchdog | `src/watchdog.rs` (WatchdogConfig, restart logic) |

## Dependencies

Uses: koina, chrono, cron, jiff, rusqlite, tokio, snafu, tracing
Used by: aletheia (binary)

## Observability

### Metrics (Prometheus)

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_watchdog_restarts_total` | Counter | `process_id` | Watchdog-initiated process restarts |
| `aletheia_watchdog_hung_processes` | Gauge | - | Number of processes currently detected as hung |
| `aletheia_cron_executions_total` | Counter | `task_name`, `status` | Cron task executions (ok/error) |
| `aletheia_cron_duration_seconds` | Histogram | `task_name` | Cron task duration (buckets: 0.1s to 600s) |

### Spans

_No `#[instrument]` spans in this crate. Spans created via `tracing::info_span!` at task spawn points._

### Log Events

| Level | Event | When |
|-------|-------|------|
| `info` | `daemon started` | TaskRunner initialization with task count |
| `info` | `daemon shutting down` | Graceful shutdown initiated |
| `info` | `watchdog: registered process` | New process added to health monitoring |
| `info` | `watchdog: process heartbeat resumed` | Previously hung process recovered |
| `info` | `task scheduled` | Cron/interval task registered |
| `info` | `task completed` | Task execution finished successfully |
| `info` | `task state restored FROM SQLite` | Persistence recovery on startup |
| `warn` | `watchdog: process missed heartbeat` | Heartbeat timeout detected |
| `warn` | `watchdog: restart loop detected` | Rapid restart threshold exceeded |
| `warn` | `task execution failed` | Task error with retry scheduled |
| `warn` | `task execution skipped — no executor configured` | Missing handler for task type |
| `warn` | `retention execution failed` | Data cleanup task error |
| `error` | `watchdog: failed to restart process` | Restart command failure |
| `error` | `task panicked` | Spawned task panic caught |
