# episteme

**Purpose:** Knowledge pipeline: LLM-driven extraction, 6-factor recall scoring, conflict detection, consolidation, and embedding management.

## Key types

| Type | Purpose |
|------|---------|
| `ExtractionEngine` | LLM-driven entity/relationship/fact extraction from conversations |
| `RecallEngine` | 6-factor scoring for knowledge retrieval (recency, relevance, confidence, access, tier, graph) |
| `EmbeddingProvider` | Trait: `embed(text) -> Vec<f32>` for vector embeddings |
| `ConflictClassification` | Contradiction, Supersession, Elaboration, Independent |
| `ConsolidationProvider` | Trait for LLM-driven fact consolidation decisions |

## Public API surface

- `episteme::extract` — `ExtractionEngine`, `ExtractionProvider` trait
- `episteme::recall` — `RecallEngine`, `RecallWeights` for configurable scoring
- `episteme::embedding` — `EmbeddingProvider` trait, `CandelProvider`, `MockEmbeddingProvider`

## When to look here

- When adding or modifying knowledge extraction, recall scoring, or embedding logic
- When implementing a new `EmbeddingProvider` (e.g., a different embedding model)
