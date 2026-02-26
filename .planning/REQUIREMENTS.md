# Requirements: Aletheia Memory System Overhaul

**Defined:** 2026-02-24
**Core Value:** Agents remember everything important, surface nothing irrelevant, and maintain their own memory health without intervention.

## v1 Requirements

Requirements for this milestone. Each maps to roadmap phases.

### Testing

- [x] **TEST-01**: Ground-truth conversation corpus of 20-30 annotated conversations with expected facts, decisions, and contradictions
- [x] **TEST-02**: End-to-end test coverage for all memory write paths (per-turn extraction, distillation extraction, workspace flush)
- [x] **TEST-03**: End-to-end test coverage for all memory read paths (vector recall, graph recall, tiered fallback)
- [x] **TEST-04**: Precision/recall baseline measurement against ground-truth corpus before any quality changes
- [x] **TEST-05**: Regression test suite that validates extraction quality against corpus after each change

### Data Integrity

- [x] **INTG-01**: Distillation locking uses SQLite-backed lock table with crash recovery (startup scan clears locks older than 10 minutes)
- [x] **INTG-02**: Distillation mutations wrapped in explicit SQLite transaction with rollback on any failure
- [x] **INTG-03**: Workspace flush has retry queue with configurable attempts and escalation via `memory:health_degraded` event after N consecutive failures
- [x] **INTG-04**: Workspace flush produces receipts (success/failure logged with agent ID, timestamp, fact count)
- [x] **INTG-05**: Orphaned Qdrant entries (source:after_turn from dead code paths) cleaned up in bulk
- [x] **INTG-06**: All Qdrant write paths enforce required metadata (session_id, source, agent_id) — no orphan-producing paths remain
- [x] **INTG-07**: Dead code audit removes all unused memory-related code paths, imports, and unreachable branches

### Graph Layer

- [x] **GRPH-01**: Neo4j RELATES_TO rate below 30% (verified with Cypher count query), down from 81%
- [x] **GRPH-02**: `neo4j-graphrag` SimpleKGPipeline integrated with `allowed_types` vocabulary constraint in extraction prompt
- [x] **GRPH-03**: Extraction prompt enumerates controlled vocabulary with worked examples — LLM selects from vocabulary, not free-form generation
- [x] **GRPH-04**: `neo4j-driver` deprecated package replaced with `neo4j` 6.1.0
- [x] **GRPH-05**: Entity extraction validates against controlled vocabulary before Neo4j write — rejects or normalizes unknown types
- [x] **GRPH-06**: Relationship vocabulary pruned to actively-used types based on empirical analysis of current graph data

### Extraction Pipeline

- [x] **EXTR-01**: Contradiction field from extract.ts wired downstream to temporal invalidation endpoint — contradictions trigger automatic fact invalidation
- [x] **EXTR-02**: Cross-chunk semantic dedup pass via sidecar after mergeExtractions() — cosine similarity check prevents near-duplicate facts from different chunks
- [x] **EXTR-03**: Cross-chunk contradiction detection — second LLM pass on merged facts identifies contradictions spanning chunks
- [x] **EXTR-04**: AbortSignal threaded through distillation pipeline — long distillations can be cancelled via API
- [x] **EXTR-05**: Evolution endpoint wired into main distillation flow — new facts that supersede old ones produce one coherent fact, not two contradicting entries
- [x] **EXTR-06**: Direct-write paths (add_direct, add_batch) enforce `infer=False` on Mem0 to prevent double-extraction

### Recall Quality

- [x] **RECL-01**: Reinforcement loop wired — memory IDs from recall results call reinforce endpoint, boosting frequently-accessed memories
- [x] **RECL-02**: Decay applied to memories not accessed within configurable window — rarely-recalled memories lose salience
- [x] **RECL-03**: Noise filter strengthened — expanded regex patterns + improved extraction prompt reduce noise rate below 5% (from ~13%)
- [x] **RECL-04**: Neo4j query timeout set to 800ms (from unbounded) — recall never blocks on slow graph queries
- [x] **RECL-05**: Qdrant and Neo4j queries run in parallel with independent timeouts — total recall latency under 1s P95
- [x] **RECL-06**: Semantic domain disambiguation prevents cross-domain bleed within an agent — "tools" in leatherwork context doesn't surface vehicle maintenance memories
- [x] **RECL-07**: Sufficiency gate thresholds tuned against ground-truth corpus — configurable confidence threshold for when to invoke graph fallback

### Observability

- [ ] **OBSV-01**: Unified memory health endpoint returns: noise rate, orphan count, RELATES_TO rate, recall latency P95, Qdrant entry counts per domain, workspace flush success rate
- [ ] **OBSV-02**: `memory:health_degraded` event emitted when any health metric crosses configured threshold
- [ ] **OBSV-03**: Corpus audit tooling — CLI command to run ground-truth corpus against current system and report precision/recall delta
- [ ] **OBSV-04**: Memory write receipts visible in diagnostics — every write path produces a traceable receipt

## v2 Requirements

Deferred to future milestone. Tracked but not in current roadmap.

### Advanced Self-Healing

- **HEAL-01**: Autonomous contradiction resolution during sleep-time — not just detection, but LLM-driven merge/invalidation
- **HEAL-02**: Community clustering health — PageRank/community detection on Neo4j graph with periodic refresh
- **HEAL-03**: Cross-domain serendipitous discovery — surface unexpected connections between agent domains

### Advanced Recall

- **ADVR-01**: BM25 as third recall tier (behind vector + graph) via Qdrant sparse vectors
- **ADVR-02**: Embedding model migration tooling — planned reindex with zero-downtime switchover

## Out of Scope

| Feature | Reason |
|---------|--------|
| Real-time graph writes on every message | Latency killer — async after-turn is correct pattern |
| LLM-generated Cypher queries | Hallucination and schema drift risk — use predefined parameterized Cypher |
| Embedding model hot-swapping | Requires full reindex — plan as explicit migration |
| Cross-agent memory sharing | Privacy boundary collapse risk — defer to explicit export with approval gates |
| Autonomous memory deletion | Irreversible data loss risk — soft decay + cold archive, no hard delete |
| Distributed multi-instance deployment | Single-server is the target for this milestone |
| UI changes beyond health visibility | Memory audit scope, not UI scope |
| Replacing Qdrant or Neo4j engines | Fix the implementation, not the stack |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| TEST-01 | Phase 1 | Complete |
| TEST-02 | Phase 1 | Complete |
| TEST-03 | Phase 1 | Complete |
| TEST-04 | Phase 1 | Complete |
| TEST-05 | Phase 1 | Complete |
| INTG-01 | Phase 2 | Complete |
| INTG-02 | Phase 2 | Complete |
| INTG-03 | Phase 2 | Complete |
| INTG-04 | Phase 2 | Complete |
| INTG-05 | Phase 2 | Complete |
| INTG-06 | Phase 2 | Complete |
| INTG-07 | Phase 2 | Complete |
| GRPH-01 | Phase 3 | Complete |
| GRPH-02 | Phase 3 | Complete |
| GRPH-03 | Phase 3 | Complete |
| GRPH-04 | Phase 3 | Complete |
| GRPH-05 | Phase 3 | Complete |
| GRPH-06 | Phase 3 | Complete |
| EXTR-01 | Phase 4 | Complete |
| EXTR-02 | Phase 4 | Complete |
| EXTR-03 | Phase 4 | Complete |
| EXTR-04 | Phase 4 | Complete |
| EXTR-05 | Phase 4 | Complete |
| EXTR-06 | Phase 4 | Complete |
| RECL-01 | Phase 5 | Complete |
| RECL-02 | Phase 5 | Complete |
| RECL-03 | Phase 5 | Complete |
| RECL-04 | Phase 5 | Complete |
| RECL-05 | Phase 5 | Complete |
| RECL-06 | Phase 5 | Complete |
| RECL-07 | Phase 5 | Complete |
| OBSV-01 | Phase 6 | Pending |
| OBSV-02 | Phase 6 | Pending |
| OBSV-03 | Phase 6 | Pending |
| OBSV-04 | Phase 6 | Pending |

**Coverage:**
- v1 requirements: 35 total
- Mapped to phases: 35
- Complete: 31 (89%)
- Pending: 4 (OBSV-01–04)
- Unmapped: 0

---
*Requirements defined: 2026-02-24*
*Last updated: 2026-02-26 — stale checkboxes fixed per v1.0 audit*
