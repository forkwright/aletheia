# KAIROS: autonomous daemon

> Specification for the oikonomos daemon subsystem and its KAIROS operating mode.

---

## Overview

**Oikonomos** (crate: `aletheia-oikonomos`, directory: `crates/daemon/`) is the per-nous background task runner. It manages scheduled tasks, periodic attention checks, maintenance cycles, and autonomous operation.

**KAIROS** is the daemon's autonomous mode — an agent that operates continuously without human prompting. Named for the Greek concept of the opportune moment: the daemon observes, waits, and acts when the time is right.

---

## Scope

### What oikonomos does

| Subsystem | Module | Purpose |
|-----------|--------|---------|
| Task runner | `runner.rs` | Cron/interval scheduling with failure tracking and graceful shutdown |
| Scheduling | `schedule.rs` | Cron expressions (jiff-native parser), intervals, one-shots, jitter |
| Prosoche | `prosoche.rs` | Periodic attention checks — agent surveys environment | 
| Maintenance | `maintenance/` | Trace rotation, drift detection, DB monitoring, retention |
| Coordination | `coordination.rs` | Child agent spawning with concurrency limits |
| Triggers | `triggers.rs` | Event-driven activation: file watchers, webhooks |
| Watchdog | `watchdog.rs` | Process heartbeat monitoring with auto-recovery |
| State | `state.rs` | SQLite persistence, workspace config, single-instance locking |

### What oikonomos does NOT do

- **Dispatch orchestration** — energeia handles prompt dispatch, session management, and QA
- **Cross-project coordination** — dianoia (#2291) handles attention allocation across projects
- **Planning** — dianoia handles plan lifecycle, phase gates, stuck detection

### Relationship to dianoia

Oikonomos and dianoia are siblings, not parent/child:

- **Oikonomos**: runs on a clock. Schedules tasks, fires on cron expressions, manages maintenance. Think cron + systemd watchdog.
- **Dianoia**: runs on intent. Orchestrates multi-step plans, allocates attention across projects, detects stuck states. Think project manager.

They share no code. Oikonomos calls nous actors via `DaemonBridge`; dianoia will coordinate via the planning adapter. Both are leaf crates with no cross-dependency.

---

## KAIROS mode

KAIROS mode composes the daemon subsystems into continuous autonomous operation:

```
                    ┌─────────────┐
                    │  TaskRunner  │
                    │  (cron loop) │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ Prosoche │ │  Maint.  │ │ Triggers │
        │ (attend) │ │ (rotate, │ │  (watch,  │
        │          │ │  gc, ...)│ │  webhook) │
        └────┬─────┘ └──────────┘ └─────┬────┘
             │                          │
             ▼                          ▼
        ┌──────────┐           ┌──────────────┐
        │  Daemon   │           │ Coordination │
        │  Bridge   │           │ (child spawn)│
        └────┬─────┘           └──────────────┘
             │
             ▼
        ┌──────────┐
        │   Nous   │
        │  Actor   │
        └──────────┘
```

### Scheduling

Tasks use the `Schedule` enum:
- `Cron(expr)` — standard 5/6-field cron expressions via jiff-native parser
- `Interval(duration)` — fixed repeat
- `Once(timestamp)` — fire-and-forget
- `Startup` — run once at process start

Jitter is deterministic per task ID (hash-based), preventing thundering herd when multiple tasks share the same cron expression.

Active windows restrict execution to time ranges (e.g., `active_window: Some((8, 23))` for 8am-11pm).

### Built-in tasks

| Task | Schedule | Purpose |
|------|----------|---------|
| Prosoche | Cron | Periodic attention check |
| TraceRotation | Cron | Compress and rotate old trace files |
| DriftDetection | Cron | Compare instance against template config |
| DbSizeMonitor | Cron | Alert on database growth |
| RetentionExecution | Cron | Execute data retention policy |
| DecayRefresh | Cron | Refresh temporal decay scores |
| EntityDedup | Cron | Merge duplicate knowledge graph entities |
| GraphRecompute | Cron | Recompute PageRank, centrality |
| EmbeddingRefresh | Cron | Re-embed stale entities |
| KnowledgeGc | Cron | Orphan removal, expired edge pruning |
| IndexMaintenance | Cron | Rebuild/optimize graph indexes |
| GraphHealthCheck | Cron | Diagnostic health check |
| SkillDecay | Cron | Retire stale skills |
| SelfAudit | Cron | Self-assessment against quality metrics |
| EvolutionSearch | Cron | Mutate and benchmark agent configs |
| SelfReflection | Cron | Agent evaluates recent performance |

### Failure handling

- Consecutive failures trigger exponential backoff (1min → 5min → 15min)
- 3 consecutive failures auto-disable the task
- Catch-up mode: missed cron windows within 24h are executed on startup
- Watchdog monitors heartbeat; auto-restarts unresponsive tasks

---

## Status

Oikonomos is implemented. KAIROS mode is scaffolded — the subsystems exist and are tested (185 tests), but full autonomous operation (prosoche → decision → action loop) is not yet wired end-to-end. Integration with dianoia for cross-project attention is planned for Phase 05e.

See [PROSOCHE.md](PROSOCHE.md) for the attention subsystem detail.
