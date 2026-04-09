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

## Observability

### Metrics (Prometheus)

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_pipeline_turns_total` | Counter | `nous_id` | Total pipeline turns processed |
| `aletheia_pipeline_stage_duration_seconds` | Histogram | `nous_id`, `stage` | Pipeline stage duration in seconds (buckets: 0.001s to 60s) |
| `aletheia_pipeline_errors_total` | Counter | `nous_id`, `stage`, `error_type` | Total pipeline errors by stage |
| `aletheia_cache_read_tokens_total` | Counter | `nous_id` | Tokens read from prompt cache (cache hits) |
| `aletheia_cache_creation_tokens_total` | Counter | `nous_id` | Tokens written to prompt cache (cache misses) |
| `aletheia_background_task_failures_total` | Counter | `nous_id`, `task_type` | Background task failures (extraction, distillation, skill analysis) |

### Spans

| Span | Location | Fields |
|------|----------|--------|
| `nous_actor` | `actor/spawn.rs` | `nous.id` |
| `handle_turn` | `actor/mod.rs` | - |
| `pipeline::run` | `pipeline/mod.rs` | `nous_id`, `session_id`, `task_hint` |
| `pipeline::resume` | `pipeline/mod.rs` | `nous_id` |
| `pipeline::finalize` | `pipeline/mod.rs` | `nous_id` |
| `execute_turn` | `execute/mod.rs` | `nous_id`, `session_id` |
| `execute_parallel` | `execute/mod.rs` | `nous_id`, `session_id` |
| `session_finalize` | `finalize.rs` | `session_id` |
| `recall` | `recall/mod.rs` | `nous_id` |
| `run_tool` | `instinct.rs` | `tool` |
| `run_tool_batch` | `instinct.rs` | `nous_id`, `tool_count` |
| `cross_router` | `cross/router.rs` | `msg_id`, `from`, `to` |
| `cross_router_deliver` | `cross/router.rs` | `msg_id`, `from`, `to` |
| `cross_router_handle_reply` | `cross/router.rs` | `in_reply_to`, `from` |

### Log Events

| Level | Event | When |
|-------|-------|------|
| `info` | `turn_completed` | Pipeline turn completes successfully |
| `info` | `nous_actor started` | Actor spawn with ID |
| `info` | `nous_actor stopped` | Actor shutdown complete |
| `warn` | `actor did not drain within timeout` | Actor restart with potential concurrent store access |
| `warn` | `recall stage failed, continuing without recalled knowledge` | Non-fatal recall failure |
| `warn` | `training capture initialization failed` | Metrics/training data capture error |
| `error` | `turn failed` | Pipeline execution error with details |
