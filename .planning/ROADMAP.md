# Roadmap: Aletheia Memory System Audit & Overhaul

## Overview

Six phases that move from broken to production-grade: establish measurable ground truth first, fix data integrity failures that make all persistence unreliable, overhaul the graph layer from 81% generic to typed relationships, complete the extraction pipeline wiring, improve recall quality on a solid foundation, then instrument observability to confirm everything stays working.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Test Infrastructure** - Ground-truth corpus and end-to-end test coverage for all memory paths (gap closure in progress)
- [x] **Phase 2: Data Integrity** - Crash-safe locking, transactional rollback, workspace flush reliability, orphan cleanup (completed 2026-02-25)
- [x] **Phase 2.1: Fix addMemories Session Wiring** - INSERTED: Gap closure — wire session_id through distillation → Qdrant path (completed 2026-02-25)
- [ ] **Phase 3: Graph Extraction Overhaul** - Neo4j RELATES_TO below 30%, typed relationships via neo4j-graphrag
- [ ] **Phase 4: Extraction Pipeline Completion** - Contradiction wiring, cross-chunk dedup, AbortSignal, Mem0 infer=False
- [ ] **Phase 5: Recall Quality** - Reinforcement loop, evolution wiring, noise filtering, latency improvements
- [ ] **Phase 6: Observability** - Unified health endpoint, degraded event emission, corpus audit tooling

## Phase Details

### Phase 1: Test Infrastructure
**Goal**: Measurable ground truth exists so every subsequent fix can be verified against precision/recall metrics, not just "no error thrown"
**Depends on**: Nothing (first phase)
**Requirements**: TEST-01, TEST-02, TEST-03, TEST-04, TEST-05
**Success Criteria** (what must be TRUE):
  1. A 20-30 conversation corpus exists with manually annotated expected facts, decisions, and contradictions — runnable against the system
  2. Running `npx vitest run` covers all memory write paths (per-turn extraction, distillation extraction, workspace flush) with no untested branches
  3. Running `npx vitest run` covers all memory read paths (vector recall, graph recall, tiered fallback)
  4. A baseline precision/recall score is recorded before any quality changes — the number exists and is referenced in subsequent phases
  5. A regression test command runs the corpus and reports precision/recall delta, callable after any extraction change
**Plans:** 4 plans (3 complete, 1 gap closure)

Plans:
- [x] 01-01-PLAN.md — Ground-truth corpus: annotation types + 20-30 conversations sourced from server
- [x] 01-02-PLAN.md — E2E integration tests for all memory write and read paths
- [x] 01-03-PLAN.md — Corpus benchmark runner + baseline precision/recall recording
- [ ] 01-04-PLAN.md — Gap closure: record real baseline scores with live API key

### Phase 2: Data Integrity
**Goal**: All memory write paths are crash-safe, transactional, and reliably persistent — no silent data loss
**Depends on**: Phase 1
**Requirements**: INTG-01, INTG-02, INTG-03, INTG-04, INTG-05, INTG-06, INTG-07
**Success Criteria** (what must be TRUE):
  1. Killing the process mid-distillation and restarting does not leave sessions locked indefinitely — startup scan clears stale locks within 10 minutes
  2. A distillation failure rolls back all mutations atomically — partial writes do not persist
  3. Workspace flush failures emit `memory:health_degraded` after N consecutive failures and are visible in logs with agent ID, timestamp, and fact count
  4. All 607+ orphaned Qdrant entries are removed and no new orphan-producing write paths remain — every write enforces session_id, source, and agent_id
  5. Dead code audit is complete — no unused imports, unreachable branches, or bypassed paths remain in memory modules
**Plans:** 4/4 plans complete

Plans:
- [ ] 02-01-PLAN.md — Crash-safe distillation locking (SQLite lock table) and atomic transaction rollback
- [ ] 02-02-PLAN.md — Qdrant orphan purge script and metadata enforcement on write paths
- [ ] 02-03-PLAN.md — Workspace flush resilience with health events and structured receipts
- [ ] 02-04-PLAN.md — Dead code audit of memory modules (mneme, distillation)

### Phase 2.1: Fix addMemories Session Wiring
**INSERTED** — Gap closure from v1.0 milestone audit
**Goal**: Distillation-extracted facts reach Qdrant — the addMemories → /add_batch path passes session_id so enforcement doesn't reject the call
**Depends on**: Phase 2
**Requirements**: (closes INTG-CROSS-01 integration gap; affected: INTG-06, INTG-02)
**Gap Closure:** Closes integration gap and broken flow from v1.0 audit
**Success Criteria** (what must be TRUE):
  1. `addMemories()` accepts and forwards `session_id` to `/add_batch` — no 400 response on valid distillation calls
  2. The distillation → Qdrant flow completes end-to-end — extracted facts are stored, not silently dropped
**Plans:** 1/1 plans complete

Plans:
- [x] 02.1-01-PLAN.md — Wire session_id through MemoryFlushTarget interface and all callers

### Phase 3: Graph Extraction Overhaul
**Goal**: Neo4j produces typed, meaningful relationships — graph traversal delivers value instead of 81% generic RELATES_TO noise
**Depends on**: Phase 2
**Requirements**: GRPH-01, GRPH-02, GRPH-03, GRPH-04, GRPH-05, GRPH-06
**Success Criteria** (what must be TRUE):
  1. `MATCH ()-[r:RELATES_TO]->() RETURN count(r)` returns below 30% of total relationships — verified with Cypher count query
  2. neo4j-graphrag SimpleKGPipeline is active with `allowed_types` constraint — entity extraction selects from controlled vocabulary, not free-form generation
  3. The deprecated `neo4j-driver` package is replaced with `neo4j` 6.1.0 — no deprecation warnings in sidecar startup
  4. Unknown relationship types are rejected or normalized before Neo4j write — no new unconstrained relationships enter the graph
  5. Relationship vocabulary reflects only actively-used types from empirical analysis of current graph data
**Plans:** 2/3 plans executed

Plans:
- [ ] 03-01-PLAN.md — Foundation: neo4j driver upgrade (6.1.0), vocabulary system redesign with external config, strict normalization
- [ ] 03-02-PLAN.md — SimpleKGPipeline integration: replace Mem0 graph store, LLM adapter, schema enforcement
- [ ] 03-03-PLAN.md — RELATES_TO backfill migration: LLM reclassification of existing edges, rate verification

### Phase 4: Extraction Pipeline Completion
**Goal**: All extraction pipeline components are wired end-to-end — contradiction detection invalidates facts, cross-chunk dedup runs, long distillations are cancellable
**Depends on**: Phase 2
**Requirements**: EXTR-01, EXTR-02, EXTR-03, EXTR-04, EXTR-05, EXTR-06
**Success Criteria** (what must be TRUE):
  1. Extracting a contradiction in a conversation causes the contradicted fact to be marked invalid in storage — the downstream invalidation endpoint is called
  2. Two semantically equivalent facts from different chunks in the same distillation produce one stored fact, not two near-duplicates
  3. A long distillation can be cancelled via API — AbortSignal propagates through the pipeline and stops work cleanly
  4. Evolution endpoint is in the main distillation flow — new facts that supersede old ones produce one coherent entry, not two contradicting entries
  5. Direct-write paths (`add_direct`, `add_batch`) never trigger double-extraction — Mem0 `infer=False` is enforced
**Plans:** 1/4 plans executed

Plans:
- [ ] 04-01-PLAN.md — Contradiction wire-up (invalidate_text endpoint) + infer=False enforcement audit
- [ ] 04-02-PLAN.md — Cross-chunk semantic dedup (dedup/batch endpoint + TS integration)
- [ ] 04-03-PLAN.md — Cross-chunk LLM contradiction detection + evolution pre-flush integration
- [ ] 04-04-PLAN.md — AbortSignal threading + cancel API endpoint

### Phase 5: Recall Quality
**Goal**: Recall is fast, relevant, and self-improving — reinforcement loop active, noise below 5%, latency under 1s P95
**Depends on**: Phase 3, Phase 4
**Requirements**: RECL-01, RECL-02, RECL-03, RECL-04, RECL-05, RECL-06, RECL-07
**Success Criteria** (what must be TRUE):
  1. Recalling memories updates their access scores — memory IDs from recall results call the reinforce endpoint and frequently-accessed memories surface higher
  2. Memories not accessed within the configured window have lower salience scores — decay is applied and measurable
  3. Noise rate in extracted facts is below 5% measured against the ground-truth corpus (down from ~13%)
  4. Recall never blocks on a slow Neo4j query — 800ms timeout enforced, Qdrant and Neo4j queries run in parallel
  5. Recall P95 latency is under 1 second measured against representative queries
  6. "tools" in a leatherwork conversation does not surface vehicle maintenance memories for the same agent — domain disambiguation prevents cross-domain bleed
  7. Sufficiency gate thresholds are tuned against the corpus — configurable confidence threshold determines when graph fallback is invoked
**Plans**: TBD

### Phase 6: Observability
**Goal**: Memory system health is visible, alerting, and continuously verifiable — operators can confirm the system stays working
**Depends on**: Phase 5
**Requirements**: OBSV-01, OBSV-02, OBSV-03, OBSV-04
**Success Criteria** (what must be TRUE):
  1. A single health endpoint returns noise rate, orphan count, RELATES_TO rate, recall latency P95, Qdrant entry counts per domain, and workspace flush success rate
  2. Crossing any health metric threshold emits `memory:health_degraded` event — degraded state is observable without polling
  3. A CLI command runs the ground-truth corpus and reports current precision/recall vs. baseline — regression is detectable
  4. Every memory write path produces a traceable receipt visible in diagnostics — write origin is auditable
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 (can overlap with 3) → 5 → 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Test Infrastructure | 3/4 | Gap closure | - |
| 2. Data Integrity | 4/4 | Complete    | 2026-02-25 |
| 2.1 Fix addMemories Session Wiring | 0/1 | Complete    | 2026-02-25 |
| 3. Graph Extraction Overhaul | 2/3 | In Progress|  |
| 4. Extraction Pipeline Completion | 1/4 | In Progress|  |
| 5. Recall Quality | 0/TBD | Not started | - |
| 6. Observability | 0/TBD | Not started | - |
