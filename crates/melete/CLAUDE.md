# melete

Context distillation engine: compresses conversation history via LLM-driven summarization. 5.2K lines.

## Read first

1. `src/distill.rs`: DistillEngine, DistillConfig, DistillResult, DistillSection (core distillation pipeline)
2. `src/flush.rs`: MemoryFlush, FlushItem (persist critical context before distillation boundaries)
3. `src/contradiction.rs`: Contradiction, ContradictionLog (cross-chunk contradiction detection)
4. `src/prompt.rs`: System prompt construction and message formatting for the distillation LLM
5. `src/similarity.rs`: Jaccard similarity pruning to remove near-duplicate content

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `DistillEngine` | `distill.rs` | Orchestrates LLM-driven summarization with token budgeting and verbatim tail preservation |
| `DistillConfig` | `distill.rs` | Knobs: token budget, section list, similarity threshold, model |
| `DistillResult` | `distill.rs` | Output: structured summary, memory flush, contradiction log, pruning stats |
| `DistillSection` | `distill.rs` | Enum of summary sections (Summary, TaskContext, CompletedWork, KeyDecisions, etc.) |
| `MemoryFlush` | `flush.rs` | Decisions, corrections, facts, and task state to persist before distillation |
| `FlushItem` | `flush.rs` | Single item to persist with content, timestamp, and source |
| `ContradictionLog` | `contradiction.rs` | Cross-chunk contradictions detected during distillation |
| `PruningStats` | `similarity.rs` | Metrics from Jaccard similarity deduplication pass |

## Patterns

- **Structured sections**: Distillation output uses fixed sections (Summary, TaskContext, CompletedWork, etc.) for predictable downstream parsing.
- **Similarity pruning**: Jaccard similarity removes near-duplicate messages before LLM summarization to reduce token waste.
- **Memory flush**: Critical context (decisions, corrections, facts) is extracted and persisted to the knowledge store before the distillation boundary erases conversation history.
- **Retry backoff**: Exponential backoff (1, 2, 4, 8 turns) on consecutive distillation failures prevents retry storms.
- **Prompt engineering**: First-person voice, specific identifiers, 400-600 word target, traceable facts.

## Common tasks

| Task | Where |
|------|-------|
| Add distillation section | `src/distill.rs` (DistillSection enum + heading/description methods) |
| Modify distillation prompt | `src/prompt.rs` (build_system_prompt, format_messages) |
| Tune similarity threshold | `src/similarity.rs` (DEFAULT_SIMILARITY_THRESHOLD) |
| Add flush source type | `src/flush.rs` (FlushSource enum) |
| Add contradiction strategy | `src/contradiction.rs` (ResolutionStrategy enum) |

## Dependencies

Uses: hermeneus
Used by: nous
