# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-24)

**Core value:** Agents remember everything important, surface nothing irrelevant, and maintain their own memory health without intervention.
**Current focus:** Phase 2 — Data Integrity

## Current Position

Phase: 2 of 6 (Data Integrity)
Plan: 2 of 4 in current phase (02-01 and 02-02 complete)
Status: Phase 2 in progress
Last activity: 2026-02-25 — Plans 02-01 and 02-02 complete; SQLite distillation locking + orphan purge + metadata enforcement

Progress: [████░░░░░░] 25%

## Performance Metrics

**Velocity:**
- Total plans completed: 3
- Average duration: 16 min
- Total execution time: 0.8 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-test-infrastructure | 3 | 48 min | 16 min |
| 02-data-integrity | 2 | ~44 min | ~22 min |

**Recent Trend:**
- Last 5 plans: 22 min, 22 min, ~15 min, 3 min
- Trend: Stable

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Pre-phase]: Fix Neo4j rather than remove — graph relationships add value for relationship reasoning, concept clustering
- [Pre-phase]: Evaluate Mem0 sidecar during Phase 2 — direct-to-Qdrant writes already bypass most of Mem0; may be dead weight
- [Pre-phase]: Production-grade showcase target — memory is Aletheia's core differentiator
- [Phase 01-test-infrastructure]: Corpus sourced from real server memory files — expected annotations represent pipeline-historically-correct values (acceptable for baseline per plan guidance)
- [Phase 01-test-infrastructure]: Static wiring verification for finalize.ts instead of dynamic — vi.mock at module level conflicts with real extractTurnFacts tests in same file
- [Phase 01-test-infrastructure]: Avoid /proc/ in test invalid-path tests — procfs mkdirSync hangs on Linux rather than failing fast
- [Phase 01-test-infrastructure]: Integration tests mock at HTTP boundary (fetch), not function boundary — exercises full code path including URL construction, response parsing, and error handling
- [Phase 01-test-infrastructure]: Corpus benchmark runner uses Jaccard token overlap (threshold=0.3) for semantic matching — avoids embedding API calls, tunable via env vars
- [Phase 01-test-infrastructure]: baseline.json committed as placeholder (API key unavailable at execution time) — user must run test:corpus:save-baseline to record real scores before Phase 2 changes extraction
- [Phase 02-data-integrity / 02-02]: Enforce validation via explicit 400 in route handlers rather than Pydantic required fields (preserves 422 for schema errors, 400 for business-logic missing fields)
- [Phase 02-data-integrity / 02-02]: aletheia.ts addMemories caller does not pass session_id to /add_batch — documented in route comment; caller needs update before memory flush path works with enforcement
- [Phase 02-data-integrity / 02-02]: /add (Mem0) enforcement deferred — traffic trace needed before enforcing session_id/agent_id on that path
- [Phase 02-data-integrity / 02-01]: SQLite PRIMARY KEY conflict used for lock acquisition — simpler than SELECT+INSERT
- [Phase 02-data-integrity / 02-01]: Retry wraps full runDistillationMutations call, not individual writes — transaction semantics make partial retry safe
- [Phase 02-data-integrity / 02-01]: Single-retry on failure does not rethrow on double failure — next scheduled distillation retries naturally

### Pending Todos

- Run `cd infrastructure/runtime && ANTHROPIC_API_KEY=<key> npm run test:corpus:save-baseline` to record real baseline scores before Phase 2 extraction changes

### Blockers/Concerns

- Phase 3 requires `/gsd:research-phase` before planning — neo4j-graphrag SimpleKGPipeline configuration and `allowed_types` schema design are non-trivial
- Phase 4 can begin in parallel with Phase 3 once Phase 2 is complete (no graph dependency)
- Mem0 evaluation deferred to Phase 2 — need traffic trace to confirm `/add` route is truly unused in production

## Session Continuity

Last session: 2026-02-25
Stopped at: Completed 02-data-integrity 02-01-PLAN.md — crash-safe SQLite distillation locking and atomic transaction rollback
Resume file: None
