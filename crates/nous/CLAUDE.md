# nous

Agent session pipeline: bootstrap, recall, execute, finalize. 22K lines. The agent runtime.

## Read first

1. `src/actor/mod.rs`: NousActor run loop (tokio::select! inbox pattern)
2. `src/pipeline/mod.rs`: PipelineInput, PipelineContext, TurnResult, guard logic
3. `src/bootstrap/mod.rs`: System prompt assembly from workspace cascade
4. `src/execute/mod.rs`: LLM call + tool dispatch loop
5. `src/manager.rs`: NousManager lifecycle, health polling, restart

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `NousActor` | `actor/mod.rs` | Tokio actor processing turns sequentially |
| `NousHandle` | `handle.rs` | Cloneable sender for invoking turns |
| `NousManager` | `manager.rs` | Spawns actors, monitors health, routes messages |
| `PipelineContext` | `pipeline/mod.rs` | Assembled context flowing through pipeline stages |
| `BootstrapAssembler` | `bootstrap/mod.rs` | Priority-based system prompt packer |
| `CrossNousRouter` | `cross/router.rs` | Inter-agent message routing with delivery audit |
| `SessionState` | `session.rs` | In-memory session tracking (turn count, token estimate) |

## Pipeline stages (in order)

1. **Guard**: rate limits, session token cap, loop detection
2. **Bootstrap**: assemble system prompt from workspace files (SOUL.md, etc.)
3. **Skills**: inject task-relevant skills from knowledge store
4. **Recall**: vector/BM25 search for related memories
5. **History**: load recent messages within token budget
6. **Execute**: LLM call, tool dispatch loop (max_tool_iterations)
7. **Finalize**: persist messages, record usage, emit events

## Patterns

- **Actor model**: sequential message processing, panic boundary (degrades after 5 panics/10min)
- **Bootstrap packing**: Required > Important > Flexible > Optional. Truncate flexible, drop optional.
- **Token budget**: `CharEstimator` (chars_per_token=4). History gets 60% of remaining budget.
- **Distillation triggers**: context >= 120K tokens, messages >= 150, 7+ day stale sessions.
- **Session types**: primary (long-lived), ephemeral (`ask:`, `spawn:` prefix), background (`prosoche`).

## Common tasks

| Task | Where |
|------|-------|
| Add pipeline stage | New module in `src/`, wire into `src/actor/mod.rs::handle_turn()` |
| Modify bootstrap | `src/bootstrap/mod.rs` (WorkspaceFileSpec list, priorities) |
| Modify recall | `src/recall.rs` (weights, search strategy, reranking) |
| Add session hook | `src/session.rs` (SessionManager or SessionState) |
| Add cross-nous message type | `src/cross/mod.rs` (CrossNousMessage enum) |

## Dependencies

Uses: koina, taxis, mneme, hermeneus, organon, melete, thesauros, tokio, snafu
Used by: pylon, aletheia (binary)
