---
scope: "crates/nous/src/compact/"
defers_to: ["../../../ARCHITECTURE.md", "../../../docs/lexicon.md"]
tightens: []
---

# Context Compaction: Two-Prompt Design

## Rationale for two prompts

Compaction has two distinct goals that demand different instructions to the model:

1. **Mid-session token-budget compaction** - We are still inside a live session. The
   only thing that matters is retaining *decisions* and *open questions* so the
   agent does not lose track of what it already figured out. Everything else
   (redundant reasoning, conversational filler, old tool output) can be discarded.
   Tone: terse, impersonal, directive.

2. **Session-boundary or dream-consolidation restoration** - The session is ending
   (or a background consolidation pass is running). The goal is to produce a
   *continuation note* that future-us can act on. That means preserving the
   *tool-call trail*, the *next intended action*, and the *working hypothesis*.
   Tone: first-person ("I"), tool-trail-preserving.

This distinction is drawn from the `huggingface/ml-intern` design in
`agent/context_manager/manager.py:85-102`, where compaction discards noise while
restoration preserves actionable continuation state.

## Prompt Routing

| `CompactReason`        | Selected prompt | Firing condition                         |
|------------------------|-----------------|------------------------------------------|
| `TokenBudget`          | `COMPACT_PROMPT` | Token usage crosses `full_compact_threshold` |
| `OperatorRequest`      | `COMPACT_PROMPT` | Operator explicitly requests compaction  |
| `SessionBoundary`      | `RESTORE_PROMPT` | Session end / checkpoint                 |
| `DreamConsolidation`   | `RESTORE_PROMPT` | Background consolidation pass            |

Routing is exhaustive (no `_` arm in `select_prompt`) so any future variant
forces an explicit decision.

## Open work

- **aletheia#95** (auto-dream consolidation): `TaskType::Consolidation` exists in
  `tasks/types.rs`, yet compaction has no call site wired from a dream or
  consolidation task. The `TODO(#2261)` in `pipeline/stages.rs` tracks the
  background summarization wiring. Once wired, the call site should use
  `select_prompt(CompactReason::DreamConsolidation)` to obtain `RESTORE_PROMPT`.
