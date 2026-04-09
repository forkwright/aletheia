# episteme

Knowledge pipeline: extraction, recall, conflict detection, consolidation, embeddings, skills. 34K lines.

## Read first

1. `src/lib.rs`: Module structure and re-exports from eidos/graphe
2. `src/extract/mod.rs`: ExtractionEngine, ExtractionProvider trait, extracted types
3. `src/recall.rs`: RecallEngine 6-factor scoring (recency, relevance, confidence, access, tier, graph)
4. `src/knowledge_store/mod.rs`: CozoDB-backed knowledge store (Datalog schema, HNSW index)
5. `src/conflict.rs`: Conflict detection pipeline for fact insertion
6. `src/embedding.rs`: EmbeddingProvider trait, CandelProvider, MockEmbeddingProvider

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `ExtractionEngine` | `extract/engine.rs` | LLM-driven entity/relationship/fact extraction from conversations |
| `ExtractionProvider` | `extract/provider.rs` | Trait for LLM calls during extraction |
| `RecallEngine` | `recall.rs` | 6-factor scoring engine for knowledge retrieval ranking |
| `RecallWeights` | `recall.rs` | Per-factor weight configuration for recall scoring |
| `EmbeddingProvider` | `embedding.rs` | Trait: `embed(text) -> Vec<f32>` for vector embeddings |
| `ConflictClassification` | `conflict.rs` | Enum: Contradiction, Supersession, Elaboration, Independent |
| `ConsolidationProvider` | `consolidation/mod.rs` | Trait for LLM-driven fact consolidation decisions |
| `QueryBuilder` | `query/builders.rs` | Typed Datalog query builder (requires `mneme-engine`) |
| `HnswIndex` | `hnsw_index.rs` | In-memory HNSW vector index (requires `hnsw_rs`) |
| `RelationType` | `vocab.rs` | Normalized relationship types for knowledge graph edges |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `graph-algo` | yes | Graph algorithms for recall scoring |
| `mneme-engine` | no | CozoDB knowledge store + typed query builder |
| `embed-candle` | no | Local ML embeddings via candle |
| `hnsw_rs` | no | Alternative HNSW vector index backend |
| `test-support` | no | MockEmbeddingProvider for tests |

## Patterns

- **6-factor recall**: recency, relevance, confidence, access frequency, knowledge tier, graph intelligence. Weighted sum produces final score.
- **Conflict pipeline**: new facts checked against existing via embedding similarity. Classified as contradiction, supersession, elaboration, or independent.
- **Extraction refinement**: turn classification, correction detection, quality filters, and fact type classification in `extract/refinement`.
- **Serendipity engine**: cross-domain discovery and surprise scoring in `serendipity/mod.rs`.
- **Ecological succession**: domain volatility tracking and adaptive decay rates in `succession.rs`.

## Common tasks

| Task | Where |
|------|-------|
| Add extraction type | `src/extract/types.rs` (new struct) + `src/extract/engine.rs` (extraction logic) |
| Modify recall scoring | `src/recall.rs` (RecallEngine, FactorScores, RecallWeights) |
| Add embedding provider | `src/embedding.rs` (implement EmbeddingProvider trait) |
| Add conflict type | `src/conflict.rs` (ConflictClassification enum) |
| Add knowledge store relation | `src/knowledge_store/mod.rs` (Datalog schema) + `src/query/schema.rs` |
| Add consolidation logic | `src/consolidation/mod.rs` |
| Add skill parser | `src/skill.rs` (SkillContent, SKILL.md parsing) |

## Dependencies

Uses: eidos, graphe, koina, krites (optional)
Used by: mneme (facade re-export)

## Observability

### Metrics (Prometheus)

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_knowledge_facts_total` | Counter | `nous_id` | Total facts inserted into knowledge store |
| `aletheia_knowledge_extractions_total` | Counter | `nous_id`, `status` | Knowledge extraction operations (ok/error) |
| `aletheia_recall_duration_seconds` | Histogram | `nous_id` | Knowledge recall latency (buckets: 0.001s to 5s) |
| `aletheia_embedding_duration_seconds` | Histogram | `provider` | Embedding computation latency (buckets: 0.001s to 5s) |

### Spans

| Span | Location | Fields |
|------|----------|--------|
| `ExtractionEngine::extract` | `extract/engine.rs` | `msg_count`, `turn_type` |
| `ExtractionEngine::analyze_turn` | `extract/engine.rs` | `msg_count` |
| `ExtractionEngine::extract_facts` | `extract/engine.rs` | - |
| `RecallEngine::recall` | `recall.rs` | - |
| `RecallEngine::rank` | `recall.rs` | `count` (candidates) |
| `HnswIndex::new` | `hnsw_index.rs` | `dim`, `max_conn` |
| `HnswIndex::load` | `hnsw_index.rs` | `dir` |
| `KnowledgeStore::search` | `knowledge_store/search.rs` | `limit`, `ef` |
| `KnowledgeStore::insert_fact` | `knowledge_store/facts.rs` | `fact_id` |

### Log Events

| Level | Event | When |
|-------|-------|------|
| `info` | `migrating knowledge schema v{N} -> v{M}` | Schema migration starting |
| `info` | `knowledge schema migration v{N} -> v{M} complete` | Schema migration finished |
| `info` | `loading existing HNSW index` | Vector index restore from disk |
| `warn` | `query rewrite failed, falling back to original` | LLM query rewrite error |
| `warn` | `fact extraction parse error` | JSON parsing failure from LLM response |
| `warn` | `failed to increment access counts` | Concurrent access tracking error |
| `warn` | `HNSW read lock was poisoned, recovering` | Lock poison recovery |
| `warn` | `unknown epistemic tier in stored fact` | Data integrity issue in stored fact |
