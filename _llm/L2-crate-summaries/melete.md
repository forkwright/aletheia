# melete

**Purpose:** Context distillation engine: compresses conversation history via LLM-driven summarization with token budgeting and verbatim tail preservation.

## Key types

| Type | Purpose |
|------|---------|
| `DistillEngine` | Orchestrates LLM summarization with token budget and tail preservation |
| `DistillConfig` | Knobs: token budget, section list, similarity threshold, model |
| `DistillResult` | Output: structured summary, memory flush, contradiction log, pruning stats |
| `MemoryFlush` | Decisions, corrections, facts to persist before distillation boundaries |
| `ContradictionLog` | Cross-chunk contradictions detected during distillation |

## Public API surface

- `melete::distill` - `DistillEngine`, `DistillConfig`, `DistillResult`
- `melete::flush` - `MemoryFlush`, `FlushItem` for pre-distillation persistence
- `melete::similarity` - Jaccard-based near-duplicate pruning

## When to look here

- When configuring or extending the context compression pipeline
- When adding new distillation sections or adjusting flush behavior
