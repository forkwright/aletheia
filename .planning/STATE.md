# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-24)

**Core value:** Agents remember everything important, surface nothing irrelevant, and maintain their own memory health without intervention.
**Current focus:** Phase 4 — Extraction Pipeline Completion

## Current Position

Phase: 4 of 6 (Extraction Pipeline Completion)
Plan: 4 of 4 in current phase (04-04 complete — AbortSignal threading, cancelDistillation export, POST distill/cancel endpoint)
Status: Phase 4 complete
Last activity: 2026-02-26 — Plan 04-04 complete; AbortSignal threading through full pipeline, cancel API endpoint

Progress: [██████████] 72%

## Performance Metrics

**Velocity:**
- Total plans completed: 11
- Average duration: 11 min
- Total execution time: ~1.8 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-test-infrastructure | 3 | 48 min | 16 min |
| 02-data-integrity | 4 | ~66 min | ~17 min |
| 03-graph-extraction-overhaul | 2 | ~9 min | ~5 min |
| 04-extraction-pipeline-completion | 4 | 30 min | 7.5 min |

**Recent Trend:**
- Last 5 plans: ~15 min, 3 min, 4 min, 3 min, 7 min
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
- [Phase 03-graph-extraction-overhaul / 03-02]: SimpleKGPipeline cached at module level — reinit on OAuth rotation via refresh_pipeline_on_token_rotate
- [Phase 03-graph-extraction-overhaul / 03-02]: extract_graph / extract_graph_batch are fire-and-forget — failures log warning but never block memory writes
- [Phase 03-graph-extraction-overhaul / 03-02]: additional_relationship_types: False enforces vocab at pipeline write time, not post-write
- [Phase 04-extraction-pipeline-completion / 04-01]: EXTR-06 bypass documented via docstring comment — no dead infer=False parameter added
- [Phase 04-extraction-pipeline-completion / 04-01]: invalidate_text uses embedding-based Qdrant search (not triple parsing) for free-form contradiction matching; 0.80 similarity threshold
- [Phase 04-extraction-pipeline-completion / 04-01]: Neo4j invalidation is best-effort — both qdrant_id match and text fragment match attempted; failures are non-fatal
- [Phase 04-extraction-pipeline-completion / 04-01]: sidecarUrl added as optional DistillationOpts field — no mandatory change at existing call sites; NousManager.setSidecarUrl() wires via getSidecarUrl()
- [Phase 04-extraction-pipeline-completion / 04-02]: dedup_batch uses greedy first-occurrence clustering — first text in each near-duplicate cluster wins, preserves insertion order
- [Phase 04-extraction-pipeline-completion / 04-02]: _cosine_similarity implemented in pure Python (math.sqrt) — avoids adding numpy/scipy to sidecar dependencies
- [Phase 04-extraction-pipeline-completion / 04-02]: Cross-chunk dedup only runs when extraction was chunked (chunks.length > 1) — no-op for single-chunk extractions
- [Phase 04-extraction-pipeline-completion / 04-02]: deduplicateFactsViaSidecar is fail-open — any sidecar error returns original facts, never blocks distillation
- [Phase 04-extraction-pipeline-completion / 04-03]: Cross-chunk contradiction pass runs for 2+ fact extractions (option B) — no wasChunked flag needed, avoids extract.ts API change
- [Phase 04-extraction-pipeline-completion / 04-03]: checkEvolutionBeforeFlush uses Promise.allSettled in batches of 5 — parallel but bounded, fail-open per memory
- [Phase 04-extraction-pipeline-completion / 04-03]: Response.mockImplementation(async () => new Response(...)) required for multi-call fetch mocks — body streams single-use
- [Phase 04-extraction-pipeline-completion / 04-03]: FlushOptions.sidecarUrl added; spread conditionally in pipeline.ts to satisfy exactOptionalPropertyTypes
- [Phase 04-extraction-pipeline-completion / 04-04]: AbortController created per-distillSession call; external opts.signal linked via addEventListener to internal controller
- [Phase 04-extraction-pipeline-completion / 04-04]: Signal propagated through opts object in summarizeInStages (not positional param) — avoids breaking existing test callers
- [Phase 04-extraction-pipeline-completion / 04-04]: Cancel returns {ok: true, cancelled: boolean} immediately — does not await distillation; pipeline finally handles cleanup

### Pending Todos

- Run `cd infrastructure/runtime && ANTHROPIC_API_KEY=<key> npm run test:corpus:save-baseline` to record real baseline scores before Phase 2 extraction changes

### Blockers/Concerns

- Phase 4 can begin in parallel with Phase 3 once Phase 2 is complete (no graph dependency)
- Mem0 evaluation deferred — need traffic trace to confirm `/add` route is truly unused in production

## Session Continuity

Last session: 2026-02-26
Stopped at: Completed 04-extraction-pipeline-completion 04-04-PLAN.md — AbortSignal threading and cancel API endpoint
Resume file: None
