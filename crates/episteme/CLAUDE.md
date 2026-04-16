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
