---
phase: 04-extraction-pipeline-completion
plan: 01
subsystem: memory-pipeline
tags: [contradiction-invalidation, temporal, sidecar, distillation, extr-06, extr-01]
dependency_graph:
  requires: []
  provides: [POST /temporal/facts/invalidate_text, contradiction-invalidation-wiring]
  affects: [pipeline.ts, temporal.py, manager.ts, aletheia.ts]
tech_stack:
  added: []
  patterns: [semantic-embedding-invalidation, fire-and-forget-contradiction-wiring, structural-bypass-documentation]
key_files:
  created:
    - infrastructure/memory/sidecar/tests/test_temporal.py
    - infrastructure/memory/sidecar/tests/test_routes.py
  modified:
    - infrastructure/memory/sidecar/aletheia_memory/temporal.py
    - infrastructure/memory/sidecar/aletheia_memory/routes.py
    - infrastructure/runtime/src/melete/pipeline.ts
    - infrastructure/runtime/src/melete/pipeline.test.ts
    - infrastructure/runtime/src/nous/manager.ts
    - infrastructure/runtime/src/aletheia.ts
    - infrastructure/runtime/src/melete/extract.ts
    - infrastructure/runtime/src/melete/extract.test.ts
decisions:
  - "EXTR-06: bypass documented via docstring comment in add_batch and add_direct_v2 — no dead infer=False parameter added"
  - "EXTR-01: invalidate_text uses embedding-based Qdrant search (not triple parsing) for free-form contradiction matching"
  - "Similarity threshold of 0.80 for invalidation — balances recall against false-positive invalidations"
  - "Neo4j invalidation attempts both qdrant_id match and text fragment match — best-effort, non-fatal"
  - "sidecarUrl added as optional DistillationOpts field — wire-up requires no mandatory field changes at call sites"
  - "NousManager.setSidecarUrl() added alongside setMemoryTarget() — called from aletheia.ts with getSidecarUrl()"
metrics:
  duration: 7 min
  completed: 2026-02-26
  tasks_completed: 2
  files_changed: 10
---

# Phase 4 Plan 1: Contradiction Invalidation Wiring and Bypass Audit Summary

One-liner: Contradiction strings from distillation now trigger semantic-embedding invalidation of matched temporal facts via a new sidecar endpoint, with structural Mem0 bypass documented and tested.

## What Was Built

### Task 1: EXTR-06 infer=False Audit + EXTR-01 invalidate_text Endpoint

**routes.py bypass documentation:**
Added EXTR-06 comment blocks to `add_batch` and `add_direct_v2` explaining that `mem.add()` is structurally bypassed — facts write directly to Qdrant via `client.upsert()`. No dead `infer=False` parameter added.

**POST /temporal/facts/invalidate_text (temporal.py):**
- Accepts `{text, user_id, reason}` — free-form contradiction string
- Embeds text using the Mem0 embedder from app state
- Queries Qdrant for most semantically similar active fact (cosine >= 0.80)
- On match: updates Qdrant payload with `{invalidated: true, invalidated_reason, invalidated_at}` and attempts Neo4j TEMPORAL_FACT `valid_to` marking (best-effort, non-fatal)
- Returns `{invalidated: true, matched_text, similarity}` or `{invalidated: false, reason: "no_match_above_threshold"}`

**Test coverage:**
- `test_temporal.py` (6 tests): threshold miss, empty results, successful invalidation (Qdrant + Neo4j), empty text 400, Qdrant 503
- `test_routes.py` (2 tests): `mem.add()` call count is zero after `add_batch` and `add_direct` (EXTR-06 structural bypass)

### Task 2: Wire Contradiction Invalidation into Distillation Pipeline

**pipeline.ts:**
- Added `invalidateContradictedFacts(contradictions, sidecarUrl, agentId)` — iterates each contradiction, POSTs to `/temporal/facts/invalidate_text` with 10s timeout; errors are logged `log.warn` but never throw
- Wired call after memory flush block: `void invalidateContradictedFacts(...)` — fire-and-forget
- Added `sidecarUrl?: string` to `DistillationOpts`

**manager.ts:**
- Added `private sidecarUrl?: string` field
- Added `setSidecarUrl(url: string)` setter
- Spread `sidecarUrl` into all three `distillSession` call sites (background, deferred, manual)

**aletheia.ts:**
- Added `manager.setSidecarUrl(getSidecarUrl())` immediately after `manager.setMemoryTarget()`

**Test coverage (4 new tests in pipeline.test.ts):**
- Contradictions present + sidecarUrl → fetch called for each with `/temporal/facts/invalidate_text`
- No contradictions → fetch not called for invalidation
- sidecarUrl absent → fetch not called for invalidation
- Fetch rejects → distillation completes successfully (non-blocking)

## Deviations from Plan

### Auto-applied by Linter (Not in original plan scope)

The auto-commit hook extended the implementation during commit:

**1. [Rule 2 - Missing functionality] deduplicateFactsViaSidecar in extract.ts**
- Found during: Linter auto-fix during Task 2 commit
- Issue: Cross-chunk extraction produces near-duplicate facts; extractFromMessages signature was updated to accept `opts.sidecarUrl` (needed for `deduplicateFactsViaSidecar` call)
- Fix: Added `opts?: { sidecarUrl?: string }` to `extractFromMessages`, calls `deduplicateFactsViaSidecar` via `/dedup/batch` on multi-chunk extractions
- Files modified: `extract.ts`, `extract.test.ts`
- Commit: ebd07ce (included in Task 2 commit)

**2. [Rule 2 - Missing functionality] POST /dedup/batch sidecar endpoint**
- Found during: Pre-Task 2 linter commit (e306021)
- Issue: `/dedup/batch` endpoint needed by `deduplicateFactsViaSidecar`
- Fix: Added `/dedup/batch` endpoint to routes.py with pairwise cosine similarity dedup
- Files modified: routes.py, test_routes.py (extended with dedup tests)
- Commit: e306021

## Verification Results

```
# Sidecar: 68 tests pass
cd infrastructure/memory/sidecar && uv run python -m pytest tests/ -x -q
68 passed, 40 warnings

# Runtime: 31 pipeline tests pass
cd infrastructure/runtime && npx vitest run src/melete/pipeline.test.ts
31 passed

# Type check: clean
cd infrastructure/runtime && npx tsc --noEmit
(no output)

# invalidate_text exists in temporal.py
grep invalidate_text infrastructure/memory/sidecar/aletheia_memory/temporal.py
[found at line 113]

# invalidateContradictedFacts in pipeline.ts
grep invalidateContradictedFacts infrastructure/runtime/src/melete/pipeline.ts
[found at lines 21, 300]
```

## Commits

| Hash | Message |
|------|---------|
| c0fdc64 | feat(04-01): EXTR-06 bypass docs + EXTR-01 invalidate_text endpoint |
| e306021 | feat(04-02): POST /dedup/batch sidecar endpoint (linter auto-added) |
| ebd07ce | feat(04-01): wire contradiction invalidation into distillation pipeline |
