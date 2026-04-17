# Phase 03: Knowledge engine

## Goal
The agent extracts facts, entities, and relationships from conversations and stores them in a queryable knowledge graph.

## Success criteria
- Extraction pipeline produces structured facts from raw conversation text
- Embedding search recalls relevant facts with recall@5 >= 80%
- Datalog engine answers conjunctive queries in under 100ms
- Knowledge consolidation merges duplicate entities without data loss

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Extraction pipeline produces structured facts from raw conversation text | Manual audit of 100 turns shows < 70% fact extraction accuracy |
| Embedding search recalls relevant facts with recall@5 >= 80% | LongMemEval benchmark shows recall@5 < 80% |
| Datalog engine answers conjunctive queries in under 100ms | Load test shows p99 query latency >= 100ms |
| Knowledge consolidation merges duplicate entities without data loss | Before/after comparison shows dropped facts or broken relationships |

## Scope

### In scope
- episteme crate: extraction, recall, consolidation, embeddings
- krites crate: embedded Datalog engine + HNSW vectors
- mneme crate: facade re-exporting eidos, graphe, episteme, krites

### Out of scope
- Multi-modal extraction (images, audio)
- Distributed knowledge graphs

## Requirements
- REQ-01: Extraction supports English; other languages are best-effort
- REQ-02: Embeddings use local candle by default; OpenAI-compatible fallback
- REQ-03: Datalog rules are hot-reloadable from instance directory
- REQ-04: Consolidation runs as a background task with progress tracking

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Embedding backend | candle (local) + OpenAI-compatible (cloud) | Operator choice between privacy and quality |
| Vector index | HNSW over IVF | Better recall at low latency for dynamic corpora |

## Open questions
- Should we support entity disambiguation? (Deferred to Phase 06)

## Dependencies
- Phase 02 complete
- LLM provider credentials configured
