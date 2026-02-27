---
phase: 04-extraction-pipeline-completion
plan: 02
subsystem: memory
tags: [deduplication, embeddings, cosine-similarity, fastapi, typescript, fetch]

requires:
  - phase: 04-extraction-pipeline-completion-plan-01
    provides: sidecarUrl in DistillationOpts, pipeline.ts contradiction invalidation pattern

provides:
  - POST /dedup/batch sidecar endpoint — in-memory pairwise cosine dedup over submitted batch
  - deduplicateFactsViaSidecar() TypeScript function calling /dedup/batch
  - Cross-chunk semantic dedup wired into extractFromMessages() after mergeExtractions()

affects:
  - future phases touching distillation pipeline or memory flush
  - any phase that adds new sidecar endpoints (pattern established here)

tech-stack:
  added: []
  patterns:
    - "In-memory greedy clustering for pairwise cosine dedup — no Qdrant needed for batch dedup"
    - "Fail-open sidecar calls — dedup unavailability never blocks distillation"
    - "_cosine_similarity helper in routes.py for vector comparison without scipy dependency"

key-files:
  created:
    - infrastructure/memory/sidecar/tests/test_routes.py (dedup tests appended to existing)
  modified:
    - infrastructure/memory/sidecar/aletheia_memory/routes.py
    - infrastructure/runtime/src/melete/extract.ts
    - infrastructure/runtime/src/melete/extract.test.ts
    - infrastructure/runtime/src/melete/pipeline.ts

key-decisions:
  - "dedup_batch does pairwise greedy clustering — first occurrence of a near-duplicate cluster wins; preserves insertion order"
  - "Cross-chunk dedup only runs when extraction was chunked (chunks.length > 1) — single-chunk sessions have no cross-chunk duplicates"
  - "_cosine_similarity added to routes.py (not scipy/numpy) — zero new dependencies for a pure Python dot-product implementation"
  - "catch param renamed err->error per oxlint unicorn/catch-error-name rule (auto-fixed as Rule 3)"

patterns-established:
  - "Sidecar endpoint pattern: pairwise in-memory dedup over submitted batch, not Qdrant search"
  - "Fail-open TypeScript sidecar call: on error/non-200, return original facts, log warn"

requirements-completed: [EXTR-02]

duration: 7min
completed: 2026-02-26
---

# Phase 04 Plan 02: Extraction Pipeline Completion Summary

**Cross-chunk semantic dedup via POST /dedup/batch sidecar endpoint and TypeScript deduplicateFactsViaSidecar integration**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-26T18:01:59Z
- **Completed:** 2026-02-26T18:09:09Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Added `POST /dedup/batch` FastAPI endpoint with pairwise cosine similarity dedup using greedy clustering at configurable threshold (default 0.90)
- Added `deduplicateFactsViaSidecar()` TypeScript function that calls `/dedup/batch`, fails open on error
- Wired cross-chunk dedup into `extractFromMessages()` — runs after `mergeExtractions()` when session was chunked and `sidecarUrl` is provided
- 9 new tests total (4 Python, 5 TypeScript) covering empty input, no duplicates, near-duplicate removal, threshold behavior, fail-open error handling

## Task Commits

1. **Task 1: POST /dedup/batch sidecar endpoint** - `e306021` (feat)
2. **Task 2: TypeScript sidecar dedup integration** - `ebd07ce` + `a7ccbd9` (feat + lint fix)

**Plan metadata:** _(pending — created in final commit)_

## Files Created/Modified

- `infrastructure/memory/sidecar/aletheia_memory/routes.py` — `import math`, `DeduplicateRequest`, `_cosine_similarity()`, `POST /dedup/batch` endpoint
- `infrastructure/memory/sidecar/tests/test_routes.py` — 4 dedup tests appended to existing test file
- `infrastructure/runtime/src/melete/extract.ts` — `deduplicateFactsViaSidecar()`, `opts` param on `extractFromMessages()`
- `infrastructure/runtime/src/melete/extract.test.ts` — 5 `deduplicateFactsViaSidecar` tests
- `infrastructure/runtime/src/melete/pipeline.ts` — passes `sidecarUrl` through to `extractFromMessages()`, lint fix

## Decisions Made

- `dedup_batch` uses greedy first-occurrence clustering — first text in each similarity cluster is retained, preserving insertion order
- Cross-chunk dedup only activates when `chunks.length > 1` — no-op for single-chunk extractions (where no cross-chunk duplicates are possible)
- `_cosine_similarity` implemented in pure Python with `math.sqrt` — avoids adding numpy/scipy dependency
- `deduplicateFactsViaSidecar` is fail-open: any sidecar error or non-200 response returns the original facts unchanged

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed oxlint catch-error-name warning blocking commit**
- **Found during:** Task 2 commit attempt
- **Issue:** Pre-commit hook rejected `catch (err)` — oxlint requires `catch (error)` per unicorn/catch-error-name rule
- **Fix:** Renamed `err` to `error` in extract.ts and pipeline.ts catch blocks
- **Files modified:** `infrastructure/runtime/src/melete/extract.ts`, `infrastructure/runtime/src/melete/pipeline.ts`
- **Verification:** `npx oxlint src/` reports 0 warnings and 0 errors
- **Committed in:** `a7ccbd9` (separate lint fix commit)

**2. [Rule 1 - Bug] _cosine_similarity helper did not exist in routes.py**
- **Found during:** Task 1 implementation
- **Issue:** Plan said "use the existing `_cosine_similarity()` helper" but it was not present in routes.py
- **Fix:** Added the helper — pure Python implementation using `math` module
- **Files modified:** `infrastructure/memory/sidecar/aletheia_memory/routes.py`
- **Verification:** 4 dedup tests all pass
- **Committed in:** `e306021` (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking lint, 1 missing helper)
**Impact on plan:** Both fixes necessary for correct implementation. No scope creep.

## Issues Encountered

- `test_routes.py` existed from Plan 04-01 (not a new file) — dedup tests were appended to the existing file rather than created fresh
- Routes.py changes from Task 1 were committed alongside the test file in `e306021`; the extract.ts additions were already in `ebd07ce` from Plan 04-01 (pre-staged work from prior execution)

## Self-Check: PASSED

All files verified present. All commits verified in git history.

## Next Phase Readiness

- Cross-chunk semantic dedup is fully wired: sidecar endpoint live, TypeScript integration complete
- Dedup is fail-open — safe to deploy without sidecar running
- EXTR-02 requirement satisfied
- Phase 04 remaining plans can proceed
