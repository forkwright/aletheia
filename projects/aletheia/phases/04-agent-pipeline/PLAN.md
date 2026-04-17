# Phase 04: Agent pipeline

## Goal
A complete turn-processing pipeline with bootstrap, recall, reasoning, and finalize stages.

## Success criteria
- Pipeline processes a turn end-to-end in under 2s (excluding LLM latency)
- Bootstrap stage loads agent character, goals, and recent context
- Recall stage retrieves relevant facts with precision@5 >= 70%
- Finalize stage persists new facts and updates session state

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Pipeline processes a turn end-to-end in under 2s (excluding LLM latency) | Benchmark shows p99 turn latency >= 2s with local LLM |
| Bootstrap stage loads agent character, goals, and recent context | Agent responds out of character or without knowledge of stated goals |
| Recall stage retrieves relevant facts with precision@5 >= 70% | Eval dataset shows precision@5 < 70% |
| Finalize stage persists new facts and updates session state | Session restart shows missing facts from previous turn |

## Scope

### In scope
- nous crate: agent pipeline, bootstrap, recall, finalize, actor model
- Budget management for token limits
- Multi-agent coordination primitives

### Out of scope
- Fine-tuning or model training
- External agent marketplaces

## Requirements
- REQ-01: Pipeline stages are composable and testable in isolation
- REQ-02: Actor model uses Tokio mpsc channels with backpressure
- REQ-03: Token budget is enforced before LLM call
- REQ-04: Agent character is defined in SOUL.md and parsed at load time

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Actor framework | Tokio mpsc over actix | Lighter weight, no additional framework |
| Context window | 128k tokens default | Matches Claude 3.5 Sonnet capacity |

## Open questions
- Should pipeline support branching (multiple reasoning paths)? (Deferred)

## Dependencies
- Phase 03 complete
- LLM provider available (local or cloud)
