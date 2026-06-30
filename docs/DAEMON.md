# KAIROS: autonomous daemon

> Specification for the oikonomos daemon subsystem and its KAIROS operating mode.

---

## Overview

**Oikonomos** (crate: `aletheia-oikonomos`, directory: `crates/daemon/`) is the per-nous background task runner. It manages scheduled tasks, periodic attention checks, maintenance cycles, and autonomous operation.

**KAIROS** is the daemon's autonomous mode - an agent that operates continuously without human prompting. Named for the Greek concept of the opportune moment: the daemon observes, waits, and acts when the time is right.

---

## Scope

### What oikonomos does

| Subsystem | Module | Purpose |
|-----------|--------|---------|
| Task runner | `runner.rs` | Cron/interval scheduling with failure tracking and graceful shutdown |
| Scheduling | `schedule.rs` | Cron expressions (jiff-native parser), intervals, one-shots, jitter |
| Prosoche | `prosoche.rs` | Periodic attention checks - agent surveys environment |
| Maintenance | `maintenance/` | Trace rotation, drift detection, DB monitoring, retention, knowledge maintenance, fact-extraction persistence |
| Coordination | `coordination.rs` | Reserved child-agent concurrency boundary; no spawn/join lifecycle is wired yet |
| Triggers | `triggers.rs` | Reserved external trigger boundary; no file watcher or webhook dispatch is wired yet |
| Watchdog | `watchdog.rs` | Per-task heartbeat monitor wired into `TaskRunner` when enabled |
| State | `state.rs` | fjall persistence, workspace config, single-instance locking |

### What oikonomos does NOT do

- **Dispatch orchestration** - energeia handles prompt dispatch, session management, and QA
- **Cross-project coordination** - dianoia (#2291) handles attention allocation across projects
- **Planning** - dianoia handles plan lifecycle, phase gates, stuck detection
- **External event triggers** - file-watcher and webhook config fields are reserved; the daemon does not listen for those events yet
- **Child-agent lifecycle management** - `Coordinator` stores the intended concurrency limit, but it does not spawn, join, kill, or track children yet

### Relationship to dianoia

Oikonomos and dianoia are siblings, not parent/child:

- **Oikonomos**: runs on a clock. Schedules tasks, fires on cron expressions, manages maintenance. Runtime liveness uses the systemd watchdog heartbeat, and the per-task watchdog monitors in-flight daemon tasks when `[maintenance.watchdog].enabled = true`.
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
              ┌────────────┴────────────┐
              ▼                         ▼
        ┌──────────┐ ┌──────────┐
        │ Prosoche │ │  Maint.  │
        │ (attend) │ │ (rotate, │
        │          │ │  gc, ...)│
        └────┬─────┘ └──────────┘
             │
             ▼
        ┌──────────┐
        │  Daemon  │
        │  Bridge  │
        └────┬─────┘
             │
             ▼
        ┌──────────┐
        │   Nous   │
        │  Actor   │
        └──────────┘
```

`TriggerRouter` and `Coordinator` remain reserved API boundaries outside the
live diagram until external event dispatch and child-agent lifecycle tracking
are implemented.

### Scheduling

Tasks use the `Schedule` enum:
- `Cron(expr)` - standard 5/6-field cron expressions via jiff-native parser
- `Interval(duration)` - fixed repeat
- `Once(timestamp)` - fire-and-forget
- `Startup` - run once at process start

Jitter is deterministic per task ID (hash-based), preventing thundering herd when multiple tasks share the same cron expression.

Active windows restrict execution to time ranges (e.g., `active_window: Some((8, 23))` for 8am-11pm).

### Built-in tasks

| Task | Schedule | Status | Purpose |
|------|----------|--------|---------|
| Prosoche | Cron | Implemented | Periodic attention check |
| TraceRotation | Cron | Implemented | Compress and rotate old trace files |
| DriftDetection | Cron | Implemented | Compare instance against template config |
| DbSizeMonitor | Cron | Implemented | Alert on database growth |
| RetentionExecution | Cron | Opt-in (`maintenance.retention.enabled`) | Execute data retention policy |
| DecayRefresh | Interval (4h) | Implemented, gated by `knowledge_maintenance_enabled` | Refresh temporal decay scores |
| EntityDedup | Interval (6h) | Implemented, gated by `knowledge_maintenance_enabled` | Merge duplicate knowledge graph entities |
| GraphRecompute | Interval (8h) | Implemented, gated by `knowledge_maintenance_enabled` | Recompute PageRank, centrality |
| SkillDecay | Cron (06:00) | Implemented, gated by `knowledge_maintenance_enabled` | Retire stale skills |
| DerivedFactsMaterialize | Interval (6h) | Implemented, gated by `knowledge_maintenance_enabled` | Materialize derived Datalog rules |
| SerendipityDiscovery | Cron | Opt-in (`maintenance.knowledge_maintenance_serendipity.enabled`) | Discover unexpected knowledge connections |
| OpsFactExtraction | Cron/startup | Implemented (requires knowledge executor) | Persist operational fact-extraction results for later recall/audit |
| EmbeddingRefresh | Cron | **Not implemented** | Re-embed stale entities — blocked on EmbeddingProvider bridge |
| KnowledgeGc | Cron | **Not implemented** | Orphan removal, expired edge pruning — no store contract |
| IndexMaintenance | Interval | Implemented | Rebuild gnosis code-graph index for the workspace (#5963) |
| GraphHealthCheck | Cron | **Not implemented** | Diagnostic health check — no diagnostic contract |
| SelfAudit | Cron | **Not implemented** | Self-assessment against quality metrics |
| EvolutionSearch | Cron | Opt-in (`maintenance.cron_tasks.evolution`) | Mutate and benchmark agent configs |
| SelfReflection | Cron | Opt-in (`maintenance.cron_tasks.reflection`) | Agent evaluates recent performance |

### Failure handling

- Consecutive failures trigger exponential backoff (1min → 5min → 15min)
- 3 consecutive failures auto-disable the task
- Catch-up mode: missed cron windows within 24h are executed on startup
- Systemd watchdog heartbeat covers whole-service liveness when the service unit enables `WatchdogSec`
- The per-task watchdog cancels and reschedules hung in-flight daemon tasks when `maintenance.watchdog.enabled` is true

---

## Status

Oikonomos is implemented. KAIROS mode is scaffolded - the subsystems exist and are tested, and maintenance now includes persistent fact extraction plus drift/knowledge maintenance wiring. The full autonomous prosoche-to-decision-to-action loop remains the boundary for KAIROS completion. Integration with dianoia for cross-project attention is planned for Phase 05e.

Event-driven triggers and child-agent coordination are also still scaffolded:
their public types and config fields reserve the future boundary, but no
runtime path starts file watchers, listens for webhooks, or manages child-agent
lifecycles today.

See [PROSOCHE.md](PROSOCHE.md) for the attention subsystem detail.
