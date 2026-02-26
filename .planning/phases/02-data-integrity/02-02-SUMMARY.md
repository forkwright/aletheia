---
phase: 02-data-integrity
plan: 02
subsystem: database
tags: [qdrant, python, fastapi, metadata, orphan-cleanup, data-integrity]

# Dependency graph
requires: []
provides:
  - Qdrant orphan purge script with safe dry-run default and --execute mode
  - Metadata validation guards on /add_direct and /add_batch (HTTP 400 on missing agent_id/session_id)
  - /add route enforcement deferred with documented rationale
  - Test suite for metadata enforcement (8 tests, no live services required)
affects: [03-knowledge-graph, 04-reflection-consolidation]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Validation-first pattern: check required fields before any I/O and raise 400"
    - "Dry-run default: destructive scripts require explicit --execute flag"
    - "Mocked boundary testing: TestClient with patched QdrantClient/Mem0 for fast unit validation"

key-files:
  created:
    - infrastructure/memory/scripts/purge-qdrant-orphans.py
    - infrastructure/memory/sidecar/tests/__init__.py
    - infrastructure/memory/sidecar/tests/test_metadata_enforcement.py
  modified:
    - infrastructure/memory/sidecar/aletheia_memory/routes.py

key-decisions:
  - "Enforce validation via explicit route handler checks (raise HTTPException 400) rather than making Pydantic fields required — preserves 422 for true schema errors, makes intent explicit"
  - "Deferred session_id enforcement on /add (Mem0 path) — traffic trace needed before enforcing; documented with rationale in route comment"
  - "Noted aletheia.ts addMemories caller does not pass session_id to /add_batch — documented in route comment as caller gap needing separate fix"

patterns-established:
  - "Missing required Qdrant metadata fields: check agent_id, session_id, source presence before upsert"
  - "Purge script log-before-delete: always write orphan IDs to timestamped JSON file before any deletion"

requirements-completed: [INTG-05, INTG-06]

# Metrics
duration: 3min
completed: 2026-02-25
---

# Phase 02 Plan 02: Data Integrity Summary

**Qdrant orphan purge script (dry-run by default, --execute required) and HTTP 400 validation guards on /add_direct and /add_batch blocking writes missing agent_id or session_id**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-25T15:58:59Z
- **Completed:** 2026-02-25T16:02:18Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Standalone purge script scrolls entire Qdrant collection in batches of 200, identifies points missing session_id/source/agent_id, and in dry-run prints count + source distribution + 5 sample IDs without deleting anything
- /add_direct and /add_batch now reject requests missing agent_id or session_id with HTTP 400 before any embedding/Qdrant call occurs
- /add (Mem0 path) gets a documented deferred-enforcement comment referencing the traffic-trace blocker in STATE.md
- 8 pytest tests confirm: 400 on missing agent_id, 400 on missing session_id, 400 on both missing, 400 on empty string, valid requests pass validation; fully mocked (no live Qdrant/Mem0)

## Task Commits

Each task was committed atomically:

1. **Task 1: Qdrant orphan purge script** - `4c24099` (feat)
2. **Task 2: Enforce required metadata on Qdrant write paths** - `117f2ae` (feat)

## Files Created/Modified
- `infrastructure/memory/scripts/purge-qdrant-orphans.py` - Standalone purge script; dry-run default, --execute for deletion, logs orphan IDs to timestamped JSON before delete
- `infrastructure/memory/sidecar/aletheia_memory/routes.py` - Added 400 validation guards to /add_direct and /add_batch; deferred-enforcement comment on /add with rationale
- `infrastructure/memory/sidecar/tests/__init__.py` - Empty init for tests package
- `infrastructure/memory/sidecar/tests/test_metadata_enforcement.py` - 8 tests covering validation accept/reject on both endpoints

## Decisions Made
- Used explicit `raise HTTPException(400)` in route handlers rather than making Pydantic fields required (which would produce 422) — 400 is the appropriate code for "request understood but missing required business-logic fields"
- `aletheia.ts` `addMemories()` doesn't pass session_id to `/add_batch` — documented in route comment as a known caller gap; fixing that caller is out of scope for this plan but must be done before the memory flush path works end-to-end with enforcement active

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] qdrant_client API: PointsSelector wrapper not needed**
- **Found during:** Task 1 (purge script creation)
- **Issue:** Initial script used `PointsSelector(points=PointIdsList(points=...))` but the `client.delete()` method accepts `PointIdsList` directly — wrapping in PointsSelector is not the correct API shape
- **Fix:** Removed `PointsSelector` import and wrapper, passed `PointIdsList` directly
- **Files modified:** infrastructure/memory/scripts/purge-qdrant-orphans.py
- **Verification:** `python3 -c "from qdrant_client.models import PointIdsList; print('OK')"` confirmed; syntax check passed
- **Committed in:** 4c24099 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (API usage correction)
**Impact on plan:** Minor correctness fix discovered during initial implementation; no scope change.

### Deferred Items

**aletheia.ts addMemories caller missing session_id**
- The `addMemories(agentId, memories)` function in `infrastructure/runtime/src/aletheia.ts` calls `/add_batch` without passing `session_id`
- With enforcement now active on `/add_batch`, this path will return 400 until the caller is updated
- Fix requires updating `addMemories` signature to accept `sessionId` parameter and passing it through — tracked for a future plan
- Documented in `/add_batch` route comment

## Issues Encountered
None — both tasks executed cleanly after the qdrant_client API fix.

## User Setup Required
None — no external service configuration required.

## Next Phase Readiness
- Orphan purge script ready to run against production Qdrant: `python purge-qdrant-orphans.py --host 192.168.0.29 --port 6333` (dry-run first, then --execute)
- /add_direct is now enforced and safe for new writes
- /add_batch enforcement is active but aletheia.ts addMemories caller needs session_id fix before that path works
- /add route enforcement remains deferred pending traffic analysis

## Self-Check: PASSED

All artifacts verified present:
- `infrastructure/memory/scripts/purge-qdrant-orphans.py` — FOUND
- `infrastructure/memory/sidecar/tests/test_metadata_enforcement.py` — FOUND
- `.planning/phases/02-data-integrity/02-02-SUMMARY.md` — FOUND
- Commit `4c24099` (Task 1) — FOUND
- Commit `117f2ae` (Task 2) — FOUND

---
*Phase: 02-data-integrity*
*Completed: 2026-02-25*
