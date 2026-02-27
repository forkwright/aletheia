---
phase: 06-observability
plan: 02
subsystem: observability
tags: [memory-audit, write-receipts, cli, structured-logging]
dependency-graph:
  requires: []
  provides: [memory-audit-cli, write-receipts]
  affects: [entry.ts, finalize.ts, aletheia.ts, reflect.ts]
tech-stack:
  added: []
  patterns: [structured-receipt-logging, corpus-jsonl-audit]
key-files:
  created: []
  modified:
    - infrastructure/runtime/src/entry.ts
    - infrastructure/runtime/src/nous/pipeline/stages/finalize.ts
    - infrastructure/runtime/src/aletheia.ts
    - infrastructure/runtime/src/melete/reflect.ts
decisions:
  - memoryCmd group created in Plan 02 (Plan 01 will reuse — idempotent by design)
  - avgNums/fmtNum/padEnd at module scope per oxlint consistent-function-scoping
  - turn_extraction receipt always logged on ok response (removed conditional)
  - workspace_flush receipt in pipeline.ts uses different message string (Workspace flush receipt vs Memory write receipt) — both are correct as-is
metrics:
  duration: 5 min
  completed: "2026-02-27"
  tasks: 2
  files: 4
requirements: [OBSV-03, OBSV-04]
---

# Phase 6 Plan 02: Memory Audit CLI and Write Receipts Summary

Recall regression detectable via `aletheia memory audit` CLI, and every memory write is auditable via structured `Memory write receipt` log fields.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Recall corpus audit CLI command | e76877b, 008de82 | entry.ts |
| 2 | Write receipts on all memory write paths | b437b29 | finalize.ts, aletheia.ts, reflect.ts |

## What Was Built

### Task 1: `aletheia memory audit` subcommand

Added `memoryCmd` group and `audit` subcommand to `entry.ts`.

Subcommand capabilities:
- Loads JSONL corpus (`~/.aletheia/corpus/recall.jsonl`) with `{query, expected_ids, domain}` entries
- POSTs each query to sidecar `/search` with `top_k: 20`
- Computes precision = `|returned ∩ expected| / |returned|`, recall = `|returned ∩ expected| / |expected|`, F1
- Aggregates per-domain and overall scores
- Baseline save (`--save-baseline`) writes JSON with timestamp, overall, by_domain
- Regression detection: 5% threshold on precision or recall drop from baseline
- Exit code 1 on regression (CI-friendly)
- Graceful handling of missing corpus file

Options: `--url`, `--corpus`, `--save-baseline`, `--agent`

### Task 2: Structured write receipts on all memory write paths

Four write paths now produce structured `Memory write receipt` log fields with consistent schema:

| Path | File | origin value |
|------|------|-------------|
| Turn-fact extraction | finalize.ts | `turn_extraction` |
| Distillation flush | aletheia.ts | `distillation` |
| Reflection flush | reflect.ts | `reflection` |
| Workspace flush | melete/pipeline.ts | N/A (uses "Workspace flush receipt" — pre-existing) |

Receipt fields: `origin`, `agentId`, `sessionId`, `timestamp`, `factCount`, `added`, `skipped`, `errors`

The `turn_extraction` receipt is now always logged on a successful sidecar response (removed the `if (added > 0)` guard) — important for diagnosing extraction quality issues where 0 facts are added.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Lint warnings from inline helper functions**
- **Found during:** Task 1 commit (pre-commit hook ran oxlint)
- **Issue:** `avg`, `fmt`, `pad` helper functions defined inside action handler triggered `consistent-function-scoping`; `Array#sort()` triggered `no-array-sort`
- **Fix:** Moved helpers to module scope as `avgNums`, `fmtNum`, `padEnd`; replaced `.sort()` with `.toSorted()`
- **Files modified:** `infrastructure/runtime/src/entry.ts`
- **Commit:** 008de82

**2. [Rule 3 - Blocking] Pre-existing ruff I001 import sort error blocked commit**
- **Found during:** Task 2 commit (pre-commit hook ran ruff)
- **Issue:** `infrastructure/memory/sidecar/tests/test_health.py` had un-sorted import block (pre-existing, not caused by this plan)
- **Fix:** Removed extra blank line between `import pytest` and `from aletheia_memory.routes` block; pre-commit hook auto-fixed and staged
- **Files modified:** `infrastructure/memory/sidecar/tests/test_health.py`
- **Commit:** b437b29 (auto-staged by pre-commit hook)

## Verification

- `npx tsc --noEmit` — zero type errors
- `npx vitest run src/melete/pipeline.test.ts src/nous/pipeline/stages/finalize.test.ts` — 60 tests pass
- `npx tsx src/entry.ts memory audit --help` — subcommand registered with all options
- `grep -r "Memory write receipt" infrastructure/runtime/src/` — 3 matches (finalize.ts, aletheia.ts, reflect.ts)

## Self-Check

Files created/modified:
- `infrastructure/runtime/src/entry.ts` — modified (memoryCmd + audit)
- `infrastructure/runtime/src/nous/pipeline/stages/finalize.ts` — modified (receipt)
- `infrastructure/runtime/src/aletheia.ts` — modified (receipt)
- `infrastructure/runtime/src/melete/reflect.ts` — modified (receipt)

Commits:
- e76877b: feat(06-02): add memory audit CLI subcommand
- 008de82: fix(06-02): module scope helpers + toSorted()
- b437b29: feat(06-02): write receipts on all memory write paths

## Self-Check: PASSED

All files verified present. All commits verified in git log.
