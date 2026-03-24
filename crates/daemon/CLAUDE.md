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
