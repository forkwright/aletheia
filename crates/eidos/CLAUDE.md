# eidos

Shared knowledge types for the memory layer. 1.6K lines. Zero internal dependencies.

## Read first

1. `src/knowledge.rs`: Fact, Entity, Relationship, EmbeddedChunk, EpistemicTier, FactType
2. `src/id.rs`: FactId, EntityId, EmbeddingId (newtype wrappers with validation)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Fact` | `knowledge.rs` | Memory fact: content, bi-temporal timestamps, provenance, lifecycle, access tracking |
| `Entity` | `knowledge.rs` | Knowledge graph node: name, type, aliases |
| `Relationship` | `knowledge.rs` | Directed edge between entities with weight |
| `EmbeddedChunk` | `knowledge.rs` | Vector embedding for semantic search |
| `EpistemicTier` | `knowledge.rs` | Confidence tier: Verified, Established, Inferred, Speculative |
| `KnowledgeStage` | `knowledge.rs` | Decay lifecycle: Active, Fading, Dormant, Forgotten |
| `FactType` | `knowledge.rs` | Content classification: Preference, Biographical, Technical, Procedural, etc. |
| `ForgetReason` | `knowledge.rs` | Why a fact was forgotten: UserRequest, Superseded, Contradiction, Decay, Consolidation |
| `FactId` | `id.rs` | Validated newtype for fact identifiers |
| `EntityId` | `id.rs` | Validated newtype for entity identifiers |
| `EmbeddingId` | `id.rs` | Validated newtype for embedding identifiers |
| `RecallResult` | `knowledge.rs` | Search result with fact + relevance score |

## Patterns

- **Bi-temporal model**: facts have `valid_from`/`valid_to` (domain time) and `recorded_at` (system time).
- **Structured fact decomposition**: `Fact` uses `#[serde(flatten)]` on `FactTemporal`, `FactProvenance`, `FactLifecycle`, `FactAccess` sub-structs.
- **Newtype IDs via macro**: `define_id!` generates validated newtypes with `new()`, `as_str()`, Display, Serde, TryFrom.
- **Stability model**: `FactType::base_stability_hours()` and `EpistemicTier::stability_multiplier()` feed FSRS decay scoring.
- **Far-future sentinel**: `far_future()` returns a fixed timestamp (9999-12-31) for open-ended validity.

## Common tasks

| Task | Where |
|------|-------|
| Add fact field | `src/knowledge.rs` (Fact or appropriate sub-struct) |
| Add entity/relationship type | `src/knowledge.rs` (entity_type/relation string conventions) |
| Add knowledge ID type | `src/id.rs` (new `define_id!` invocation) |
| Add epistemic tier | `src/knowledge.rs` (EpistemicTier enum) |
| Add fact type | `src/knowledge.rs` (FactType enum + classify logic) |

## Dependencies

Uses: jiff, serde, serde_json
Used by: episteme, graphe, krites, mneme
