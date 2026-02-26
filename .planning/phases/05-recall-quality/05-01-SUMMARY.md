---
phase: 05-recall-quality
plan: 01
subsystem: memory
tags: [reinforcement, jaccard, exponential-decay, neo4j, memory-lifecycle]

# Dependency graph
requires:
  - phase: 04-extraction-pipeline-completion
    provides: AbortSignal threading and finalize stage wiring patterns
provides:
  - RecallResult with memoryIds and memoryTexts fields for downstream reinforcement
  - tokenJaccardOverlap helper and selective reinforcement in finalize stage
  - exponential_decay_penalty (lambda=0.05, ~14-day half-life) in evolution.py
  - Linear decay penalty replaced with time-based exponential decay in routes.py
affects: [future recall-quality plans, any plan touching finalize or scoring logic]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Fire-and-forget reinforcement via void prefix — never blocks turn completion
    - Jaccard usage detection (>=0.25 threshold) to filter unused recall hits
    - Exponential decay multiplier pattern: score *= exp(-lambda * days_inactive)
    - New memories (no last_accessed) receive full salience until first access

key-files:
  created:
    - infrastructure/memory/sidecar/tests/test_evolution.py
  modified:
    - infrastructure/runtime/src/nous/recall.ts
    - infrastructure/runtime/src/nous/pipeline/types.ts
    - infrastructure/runtime/src/nous/pipeline/stages/finalize.ts
    - infrastructure/runtime/src/nous/recall.test.ts
    - infrastructure/runtime/src/nous/pipeline/stages/finalize.test.ts
    - infrastructure/memory/sidecar/aletheia_memory/evolution.py
    - infrastructure/memory/sidecar/aletheia_memory/routes.py

key-decisions:
  - "Jaccard threshold 0.25 is permissive — false positives (extra reinforcement) are cheap, false negatives (missed reinforcement) hurt learning"
  - "exponential_decay_penalty uses days-since-last-access (not accumulated tick count) for time-based correctness"
  - "New memories (no last_accessed in Neo4j) receive no decay penalty — full salience until first access event"
  - "_apply_confidence_weight max_penalty parameter removed — exponential decay is now parameter-free at call sites"
  - "Decay endpoint Cypher intentionally never writes last_accessed — separation of concerns between decay and reinforce paths"

patterns-established:
  - "Reinforcement pattern: memoryIds+memoryTexts in RecallResult -> threaded via TurnState.recalledMemoryIds/Texts -> consumed in finalize"
  - "Exponential decay: score multiplier = math.exp(-0.05 * days_inactive), replaces linear decays * 0.02"

requirements-completed: [RECL-01, RECL-02]

# Metrics
duration: 4min
completed: 2026-02-26
---

# Phase 5 Plan 01: Recall Quality - Reinforcement Loop and Exponential Decay Summary

**Closed feedback loop: Jaccard-gated reinforcement of used memories and time-based exponential decay replacing linear penalty**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-26T21:11:50Z
- **Completed:** 2026-02-26T21:15:07Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- RecallResult now surfaces memoryIds and memoryTexts from deduped hits, threaded through TurnState to finalize
- finalize.ts reinforces only memories with Jaccard overlap >= 0.25 against the response text — fire-and-forget
- exponential_decay_penalty with lambda=0.05 (~14-day half-life) replaces linear `decays * 0.02` in routes.py
- 47 tests total pass: 37 TypeScript (recall + finalize) and 10 Python (evolution)

## Task Commits

Each task was committed atomically:

1. **Task 1: Thread memory IDs through recall and wire reinforcement in finalize** - `50ecf46` (feat)
2. **Task 2: Replace linear decay with exponential decay formula** - `bd8c33e` (feat)
3. **Task 2 tests: Evolution endpoint behavior tests** - `63d792d` (test)

## Files Created/Modified
- `infrastructure/runtime/src/nous/recall.ts` - Added memoryIds and memoryTexts fields to RecallResult
- `infrastructure/runtime/src/nous/pipeline/types.ts` - Added recalledMemoryIds and recalledMemoryTexts to TurnState
- `infrastructure/runtime/src/nous/pipeline/stages/finalize.ts` - tokenJaccardOverlap, reinforceUsedMemories, wired at end of finalize
- `infrastructure/runtime/src/nous/recall.test.ts` - Tests for memoryIds/memoryTexts presence in result
- `infrastructure/runtime/src/nous/pipeline/stages/finalize.test.ts` - Jaccard tests and selective reinforcement tests
- `infrastructure/memory/sidecar/aletheia_memory/evolution.py` - exponential_decay_penalty function
- `infrastructure/memory/sidecar/aletheia_memory/routes.py` - Import + use exponential_decay_penalty, query last_accessed from Neo4j
- `infrastructure/memory/sidecar/tests/test_evolution.py` - Created: decay curve tests, endpoint behavior tests

## Decisions Made
- Jaccard threshold 0.25 is permissive — false positives (reinforcing a tangentially relevant memory) cost almost nothing, false negatives (failing to reinforce a used memory) hurt the feedback loop
- exponential_decay_penalty uses days-since-last-access computed from the Neo4j `last_accessed` timestamp, not a tick counter
- New memories without a MemoryAccess Neo4j node receive no decay — they retain full salience until first access event
- Removed `max_penalty` parameter from `_apply_confidence_weight` — exponential decay is self-bounding, parameter unnecessary

## Deviations from Plan

None - plan executed exactly as written. Task 1 and task 2 were already partially committed in prior sessions (50ecf46 and bd8c33e); the test file for Task 2 was the remaining piece.

## Issues Encountered
- `test_decay_does_not_modify_last_accessed` initially failed with 503 because the decay endpoint reads `request.app.state.memory` — fixed by creating a dedicated FastAPI app with mock memory wired to `app.state.memory` rather than using the shared client fixture

## Next Phase Readiness
- RECL-01 and RECL-02 complete — reinforcement and decay infrastructure fully operational
- Ready for remaining recall-quality plans in phase 05

---
*Phase: 05-recall-quality*
*Completed: 2026-02-26*

## Self-Check: PASSED

All files exist and all commits verified:
- `infrastructure/memory/sidecar/tests/test_evolution.py` - FOUND
- `.planning/phases/05-recall-quality/05-01-SUMMARY.md` - FOUND
- `50ecf46` (Task 1 commit) - FOUND
- `bd8c33e` (Task 2 commit) - FOUND
- `63d792d` (Task 2 tests commit) - FOUND
