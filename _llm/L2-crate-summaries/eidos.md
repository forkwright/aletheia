# eidos

**Purpose:** Shared knowledge types for the memory layer. Zero workspace dependencies — pure data types.

## Key types

| Type | Purpose |
|------|---------|
| `Fact` | Memory fact: content, bi-temporal timestamps, provenance, lifecycle, access tracking |
| `Entity` | Knowledge graph node: name, type, aliases |
| `Relationship` | Directed edge between entities with weight |
| `EpistemicTier` | Confidence: Verified, Established, Inferred, Speculative |
| `KnowledgeStage` | Decay lifecycle: Active, Fading, Dormant, Forgotten |

## Public API surface

- `eidos::knowledge` — `Fact`, `Entity`, `Relationship`, `EmbeddedChunk`, `EpistemicTier`, `FactType`, `ForgetReason`
- `eidos::id` — `FactId`, `EntityId`, `EmbeddingId` validated newtype wrappers

## When to look here

- When defining or extending knowledge graph types (Fact, Entity, Relationship)
- When working with epistemic confidence tiers or knowledge lifecycle states
