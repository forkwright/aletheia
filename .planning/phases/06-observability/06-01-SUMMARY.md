---
phase: 06-observability
plan: 01
subsystem: memory-sidecar, runtime-cli
tags: [health-monitoring, observability, threshold-evaluation, cli]
dependency-graph:
  requires: [05.1-01]
  provides: [OBSV-01, OBSV-02]
  affects: [infrastructure/memory/sidecar/aletheia_memory/routes.py, infrastructure/runtime/src/entry.ts]
tech-stack:
  added: []
  patterns: [asyncio.gather for parallel metric collection, deque rolling window for P95, Zod default schema patterns]
key-files:
  created:
    - infrastructure/memory/sidecar/tests/test_health.py
  modified:
    - infrastructure/memory/sidecar/aletheia_memory/routes.py
    - infrastructure/runtime/src/koina/event-bus.ts
    - infrastructure/runtime/src/taxis/schema.ts
    - infrastructure/runtime/src/entry.ts
decisions:
  - "_compute_p95 returns None for < 5 samples — insufficient data should not trigger false thresholds"
  - "asyncio.gather(return_exceptions=True) for parallel Qdrant+Neo4j collection — each subsystem failure is non-fatal"
  - "Threshold query param as JSON string — allows CLI to pass full config without routing through sidecar config"
  - "pad helper reuses module-scope padEnd from entry.ts — avoids consistent-function-scoping lint error"
  - "readJson already top-level imported in entry.ts — dynamic re-import skipped to avoid no-shadow lint error"
metrics:
  duration: 8 min
  completed: 2026-02-27
  tasks: 2
  files: 5
---

# Phase 06 Plan 01: Memory Health Observability Summary

Extended sidecar /health endpoint with semantic memory metrics (noise rate, orphan count, RELATES_TO rate, recall P95, per-agent Qdrant counts), threshold evaluation yielding healthy/degraded/critical status, and wired an `aletheia memory health` CLI command that emits memory:health_degraded / memory:health_recovered events.

## Completed Tasks

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Extend sidecar /health with semantic metrics and latency capture | b437b29 | routes.py, tests/test_health.py |
| 2 | Health thresholds config, event registration, and CLI command | 7b926bf | event-bus.ts, schema.ts, entry.ts |

## Outcomes

### Task 1: Sidecar /health Extension

- Added `_RECALL_LATENCY_SAMPLES: deque[float] = deque(maxlen=100)` at module scope
- Added `_compute_p95()` — returns None when < 5 samples
- Instrumented `/search` to append `time.time() - t0` to the deque on every call
- Added `_collect_qdrant_metrics()` — orphan count, per-agent counts, total entries
- Added `_collect_noise_rate()` — samples 500 entries, applies `_RECALL_NOISE_PATTERNS`
- Added `_collect_neo4j_metrics()` — RELATES_TO / total relationships ratio; None on failure
- Added `_parse_thresholds()` — parses JSON query param with defaults fallback
- Added `_evaluate_thresholds()` — healthy / degraded / critical status derivation
- Extended `health_check` to use `asyncio.gather(return_exceptions=True)` for parallel collection
- Response shape: `{status, ok, version, llm, checks, qdrant, neo4j, recall, thresholds}`
- Created `tests/test_health.py` with 18 tests covering all helpers and Neo4j failure paths

### Task 2: TypeScript Runtime Changes

- Added `memory:health_recovered` to `EventName` union in `koina/event-bus.ts`
- Added `MemoryHealthThresholdsConfig` Zod schema with 5 fields and defaults
- Added `memoryHealth: MemoryHealthThresholdsConfig` to `AletheiaConfigSchema`
- Exported `MemoryHealthThresholds` type
- Added `aletheia memory health` subcommand under existing `memoryCmd` group
- CLI reads config thresholds, passes as JSON query param to `/health`
- Formats table output: Noise rate, Orphan count, RELATES_TO rate, Recall P95, Flush success rate
- Emits `memory:health_degraded` or `memory:health_recovered` via `eventBus`
- Exits 1 on degraded/critical

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Plan 06-02 ran before 06-01 and pre-committed routes.py and test_health.py**
- Found during: Task 1 commit attempt
- Issue: git showed "nothing to commit" — 06-02's last commit (b437b29) already included routes.py and test_health.py as part of writing the health test import fix
- Fix: Verified content was correct, counted b437b29 as Task 1 commit, proceeded with Task 2
- Files modified: None additional (already committed)

**2. [Rule 1 - Bug] `readJson` shadowing in dynamic import**
- Found during: Task 2 commit (oxlint no-shadow)
- Issue: `readJson` imported at module scope and re-imported inside action handler
- Fix: Removed dynamic re-import, used top-level `readJson` directly
- Files modified: entry.ts

**3. [Rule 1 - Bug] `pad` helper inside action handler**
- Found during: Task 2 commit (oxlint consistent-function-scoping)
- Issue: `pad` function defined inside action handler captures no outer scope variables
- Fix: Removed `pad`, used existing module-scope `padEnd` helper instead
- Files modified: entry.ts

**4. [Rule 1 - Bug] `!= null` instead of `!== null`**
- Found during: Task 2 commit (oxlint eqeqeq)
- Issue: 8 uses of `!= null` for null checks
- Fix: Replaced all with explicit `!== null && !== undefined` form
- Files modified: entry.ts

## Verification Results

- `python -m pytest tests/test_health.py -x -v`: 18 passed
- `npx tsc --noEmit`: clean
- `npx tsx src/entry.ts memory health --help`: shows subcommand
- `npx tsx src/entry.ts memory --help`: shows audit + health subcommands

## Self-Check: PASSED

Files exist:
- infrastructure/memory/sidecar/aletheia_memory/routes.py: FOUND (contains `_RECALL_LATENCY_SAMPLES`, `noise_rate`, `_compute_p95`)
- infrastructure/memory/sidecar/tests/test_health.py: FOUND (18 tests)
- infrastructure/runtime/src/koina/event-bus.ts: FOUND (contains `memory:health_recovered`)
- infrastructure/runtime/src/taxis/schema.ts: FOUND (contains `noiseRateMax`)
- infrastructure/runtime/src/entry.ts: FOUND (contains `memory health` subcommand)

Commits exist:
- b437b29: Task 1 (routes.py + test_health.py — via 06-02 pre-commit)
- 7b926bf: Task 2 (event-bus.ts + schema.ts + entry.ts)
