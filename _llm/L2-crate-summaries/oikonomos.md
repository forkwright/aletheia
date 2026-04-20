# oikonomos (daemon)

**Purpose:** Per-nous background task runner with cron scheduling, maintenance services, prosoche attention checks, and watchdog process monitoring.

## Key types

| Type | Purpose |
|------|---------|
| `TaskRunner` | Per-nous scheduler: cron, interval, one-shot, and startup task modes |
| `TaskDef` | Task definition: id, schedule, action, timeout, active window |
| `BuiltinTask` | Built-in tasks: Prosoche, TraceRotation, DriftDetection, DbSizeMonitor |
| `DaemonBridge` | Trait for sending prompts to nous without direct dependency |
| `Watchdog` | Heartbeat tracker with configurable timeout and auto-restart |

## Public API surface

- `daemon::runner` - `TaskRunner` lifecycle (start, register, stop)
- `daemon::schedule` - `TaskDef`, `Schedule`, `BuiltinTask`
- `daemon::bridge` - `DaemonBridge` trait (implemented in aletheia crate)

## When to look here

- When adding a new recurring background task (add variant to `BuiltinTask`, schedule in `TaskDef`)
- When debugging prosoche check-ins or watchdog restart behavior
