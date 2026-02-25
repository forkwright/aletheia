# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-24)

**Core value:** Agents remember everything important, surface nothing irrelevant, and maintain their own memory health without intervention.
**Current focus:** Phase 3 — Graph Extraction Overhaul

## Current Position

Phase: 3 of 6 (Graph Extraction Overhaul)
Plan: 1 of 3 in current phase (03-01 complete — driver upgrade + vocab redesign)
Status: Phase 3 in progress
Last activity: 2026-02-25 — Plan 03-01 complete; neo4j upgraded to 6.x, neo4j-graphrag added, RELATES_TO eliminated from vocab

Progress: [███████░░░] 45%

## Performance Metrics

**Velocity:**
- Total plans completed: 8
- Average duration: 11 min
- Total execution time: ~1.5 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-test-infrastructure | 3 | 48 min | 16 min |
| 02-data-integrity | 4 | ~66 min | ~17 min |
| 03-graph-extraction-overhaul | 1 | 3 min | 3 min |

**Recent Trend:**
- Last 5 plans: 22 min, ~15 min, 3 min, 4 min, 3 min
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
- [Phase 02-data-integrity / 02-03]: flushToWorkspaceWithRetry is a separate export — keeps flushToWorkspace pure, retry is optional at call site
- [Phase 02-data-integrity / 02-03]: Module-level Map for per-agent flush failure counter — survives across distillation calls without threading state through opts
- [Phase 02-data-integrity / 02-03]: Mock workspace-flush in pipeline.test.ts — isolates counter/event logic from filesystem behavior tested separately
- [Phase 02-data-integrity / 02-03]: /proc/ paths hang on Linux procfs — use file-as-workspace (ENOTDIR) for reliable fast test failures
- [Phase 02-data-integrity]: shouldDistill async keyword removed — function has no await, return type is boolean not Promise<boolean>
- [Phase 02-data-integrity]: mneme modules (store.ts, schema.ts) had zero dead code — no changes needed after Plan 02-01 through 02-03
- [Phase 02.1-fix-addmemories-session-wiring]: sessionId is required (no default) on flushToMemory — empty string is falsy in Python, would trigger 400 from /add_batch enforcement
- [Phase 02.1-fix-addmemories-session-wiring]: reflect.ts uses "reflection" as synthetic session identifier — satisfies non-empty string check; source field disambiguates path type
- [Phase 03-graph-extraction-overhaul / 03-01]: normalize_type() returns None for unknown types instead of RELATES_TO — callers skip relationship rather than persist vague edges
- [Phase 03-graph-extraction-overhaul / 03-01]: Vocab file at ~/.aletheia/graph_vocab.json uses version+relationship_types structure — fails safe to hardcoded defaults on missing/corrupt file
- [Phase 03-graph-extraction-overhaul / 03-01]: GRAPH_EXTRACTION_PROMPT instructs LLM to skip unmatched relationships — no catch-all fallback type

### Pending Todos

- Run `cd infrastructure/runtime && ANTHROPIC_API_KEY=<key> npm run test:corpus:save-baseline` to record real baseline scores before Phase 2 extraction changes

### Blockers/Concerns

- Phase 4 can begin in parallel with Phase 3 once Phase 2 is complete (no graph dependency)
- Mem0 evaluation deferred — need traffic trace to confirm `/add` route is truly unused in production

## Session Continuity

Last session: 2026-02-25
Stopped at: Completed 03-graph-extraction-overhaul 03-01-PLAN.md — driver upgrade + vocab redesign
Resume file: None
