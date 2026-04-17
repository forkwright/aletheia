# Phase 09: Dispatch orchestration

## Goal
Background task scheduling, cron jobs, and pipeline dispatch stages for autonomous agent operation.

## Success criteria
- Cron expressions execute tasks with < 1s drift per day
- Dispatch pipeline supports Validation, HealthCheck, and post-processing stages
- Task state is persisted across daemon restarts
- After-action records are written as structured JSONL

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Cron expressions execute tasks with < 1s drift per day | 24-hour run shows task execution drift >= 1s |
| Dispatch pipeline supports Validation, HealthCheck, and post-processing stages | Stage test shows missing stage or incorrect ordering |
| Task state is persisted across daemon restarts | Restart test shows lost or duplicate tasks |
| After-action records are written as structured JSONL | JSONL parser fails on emitted records or schema mismatch |

## Scope

### In scope
- energeia crate: dispatch orchestration, pipeline stages
- daemon crate: background tasks, cron, prosoche
- fjall storage backend for task state

### Out of scope
- Distributed scheduling across multiple nodes
- GPU scheduling

## Requirements
- REQ-01: Cron uses standard 5-field expressions with optional timezone
- REQ-02: Dispatch stages are composable and skippable via config
- REQ-03: Task retries use exponential backoff with jitter
- REQ-04: JSONL records include timestamp, task_id, and outcome fields

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Task store | fjall over SQLite | LSM-tree better for append-only task logs |
| Cron library | cron_clock over tokio-cron-scheduler | More standard expression parsing |

## Open questions
- Should tasks support dependencies (DAG execution)? (Deferred to Phase 13)

## Dependencies
- Phase 08 complete
- systemd or launchd for daemon supervision
