# mneme

Session store (SQLite) and knowledge engine (CozoDB Datalog + HNSW vectors). 110K lines. The memory layer.

## Read first

1. `src/types.rs`: Session, Message, UsageRecord (core domain)
2. `src/knowledge.rs`: Fact, Entity, Relationship, EpistemicTier
3. `src/store/mod.rs`: SessionStore (SQLite WAL)
4. `src/knowledge_store/mod.rs`: KnowledgeStore (CozoDB, feature-gated)
5. `src/recall.rs`: 6-factor recall scoring engine

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `SessionStore` | `store/mod.rs` | SQLite session/message persistence |
| `KnowledgeStore` | `knowledge_store/mod.rs` | CozoDB Datalog + HNSW (feature: `mneme-engine`) |
| `Fact` | `knowledge.rs` | Bi-temporal memory with confidence, tier, decay |
| `Entity` | `knowledge.rs` | Named entity with aliases and type |
| `RecallEngine` | `recall.rs` | Weighted 6-factor scoring for memory retrieval |
| `ExtractionEngine` | `extract/mod.rs` | LLM-driven fact/entity extraction from conversations |
| `SkillExtractor` | `skills/mod.rs` | Auto-capture of recurring tool patterns as skills |

## Patterns

- **Bi-temporal facts**: `valid_from`/`valid_to` windows. Supersession chains via `superseded_by`.
- **Epistemic tiers**: Verified > Inferred > Assumed. Tier affects decay rate and consolidation eligibility.
- **FSRS decay**: `stability_hours` + `access_count` drive spaced-repetition-style recall scoring.
- **Datalog queries**: typed builder in `query/builders.rs` or raw via `KnowledgeStore::run_query()`.
- **SQLite recovery**: integrity check on open, auto-repair (backup + re-init), read-only fallback.
- **Feature flags**: `mneme-engine` (knowledge store), `storage-fjall` (persistent), `embed-candle` (local embeddings).

## Common tasks

| Task | Where |
|------|-------|
| Add session field | `src/types.rs` (struct) + `src/schema.rs` (DDL) + `src/migration.rs` + `src/store/session.rs` |
| Add knowledge query | `src/query/builders.rs` (typed) or `src/knowledge_store/mod.rs` (raw Datalog) |
| Add recall signal | `src/recall.rs` (new field in FactorScores + weight in RecallWeights) |
| Add extraction type | `src/extract/types.rs` + `src/extract/engine.rs` |
| Add skill heuristic | `src/skills/heuristics.rs` (scoring function) |

## Dependencies

Uses: koina, serde, jiff, ulid, rusqlite, snafu, tracing, candle-*, fjall
Used by: nous, pylon, melete, aletheia (binary)
