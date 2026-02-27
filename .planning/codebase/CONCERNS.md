# Codebase Concerns

**Analysis Date:** 2025-02-24

## Tech Debt

### Single-Threaded Distillation Locking

**Issue:** Distillation uses an in-memory `Set<string>` to prevent concurrent distillations
- Files: `infrastructure/runtime/src/distillation/pipeline.ts` (line 19)
- Impact: If the runtime crashes mid-distillation, the lock never clears. On restart, that session is permanently blocked from distillation. Sessions will accumulate tokens indefinitely.
- Current implementation:
  ```typescript
  const activeDistillations = new Set<string>();
  // Lock is only cleared via .delete() on success/error
  // No recovery mechanism on process restart
  ```
- Fix approach: Migrate to distributed locking (Redis/Etcd) OR add startup recovery that clears stale locks older than 10 minutes. For now, add a warning in startup diagnostics if locks are found at runtime start.

### MCP Session Memory Leak

**Issue:** Unbounded `mcpSessions` Map grows without automatic cleanup
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (line 175-180)
- Impact: Long-running systems with many MCP connections will accumulate orphaned session objects. Each contains a `ReadableStreamDefaultController` and `setInterval` handle that won't be garbage collected if cleanup fails.
- Scenario: Client connects but network drops before SSE `cancel()` is called → session stays in map forever
- Fix approach: Add periodic cleanup task that purifies stale sessions (no activity > 5 minutes). Implement heartbeat validation on `/messages` endpoint to detect dead connections.

### Manual Transaction Management in Store

**Issue:** Database transactions managed via `.transaction()` wrapper without automatic rollback on uncaught errors
- Files: `infrastructure/runtime/src/mneme/store.ts` (migrations at lines 218-226)
- Impact: If a migration function throws an exception after database modifications but before commit, state may be partially written. Future migrations see partial state.
- Current: `const migrate = this.db.transaction(() => { ... }); migrate();` relies on better-sqlite3's implicit rollback
- Concern: Pattern is fragile if exception escapes the transaction callback
- Fix approach: Wrap all transaction callbacks in try-catch that logs state. Add pre-flight validation that schema version matches before applying next migration.

### Tool Timeout Configuration Duplication

**Issue:** Tool timeout config duplicated across codebase
- Files: `infrastructure/runtime/src/organon/timeout.ts` (line 10-16)
- Impact: Hardcoded timeouts for different tool types makes it impossible to reconfigure without code changes. If a tool legitimately needs 30s but default is 15s, it will timeout in production and require redeployment.
- Current: Only `exec` and `sessions_ask` disable timeout (0 = disabled). Others get default 30s with no per-environment override.
- Fix approach: Move all timeout values to `aletheia.json` config under `agents.defaults.toolTimeouts`. Validate at startup that all registered tools have timeout definitions.

## Known Bugs

### Empty Try-Catch in MCP Token Loading

**Issue:** Silent failure when loading MCP tokens
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 39-45)
- Impact: If `mcp-tokens.json` is malformed (truncated, invalid JSON), the system silently returns empty array and boots with auth disabled (or in open mode). No warning to operator.
- Current:
  ```typescript
  try {
    const raw = readFileSync(tokensPath, "utf-8");
    return JSON.parse(raw) as McpToken[];
  } catch { /* MCP init failed */
    log.warn("Failed to load MCP tokens");
    return [];
  }
  ```
- Symptoms: MCP accessible to anyone, no indication why
- Fix approach: Log error details including file path and parse error. In diagnostics, flag if auth is disabled unintentionally. Require explicit `auth.allowUnauthenticated: true` in config to proceed.

### Signal-CLI Daemon Restart Loop Without Jitter

**Issue:** Signal-CLI restart backoff is linear but uses same delay calculation
- Files: `infrastructure/runtime/src/semeion/daemon.ts` (lines 101-105)
- Impact: If startup requires fixed sequence (e.g., service account auth), all retry attempts fire at: 2s, 4s, 6s, 8s, 10s. If root cause is temporary (e.g., resource contention), you waste 30s on 5 attempts when 2s jitter could have saved it.
- Current: `const delay = RESTART_BACKOFF_MS * restartAttempts;` with fixed 2000ms base
- Fix approach: Add exponential backoff with ±10% jitter: `Math.random() * 0.2 * base + base * Math.pow(1.5, attempt)`. Cap at 30s.

### Unvalidated Query Limit in MCP Memory Search

**Issue:** User-controlled limit parameter in memory search not validated against backend constraints
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 390-396)
- Impact: Frontend clamps limit to [1, 50], but if memory sidecar has different constraints or if sidecar is replaced with custom implementation, request could succeed then timeout or OOM.
- Current: `const limit = Math.min(Math.max(Number(args["limit"]) || 10, 1), 50);`
- Fix approach: Query sidecar for its max limit on startup, store in config, use that instead of hardcoded 50.

## Security Considerations

### Timing-Safe Token Comparison Present

**Status:** Secure
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 60-66)
- Implementation correctly uses `timingSafeEqual` to prevent timing attacks on bearer token validation
- No immediate concern

### MCP Scope Validation Adequate

**Status:** Adequate but incomplete
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 69-74)
- Current: Scope supports wildcards (`*`, `category:*`). Tool access is checked per-method.
- Gap: No audit log of tool executions. If token is leaked, no way to detect abuse retroactively.
- Recommendation: Add `tool_executed` events to event bus including `{tool, client, result_tokens}`. Periodically review for anomalies (tools executed by unexpected clients, unusually high token usage).

### Credentials File Permissions

**Status:** Correctly hardcoded
- Files: `infrastructure/runtime/src/entry.ts` (line 102)
- Anthropic API key written with `mode: 0o600` (read/write owner only). Correct.
- No issue

### MCP Body Size Limits Enforced

**Status:** Secure
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 142-145, 348-349, 393)
- JSON-RPC body size checked against `maxBody` config (default 1MB). Messages capped at 100KB.
- Query length capped at 2000 chars. Protects against unbounded memory allocation.

## Performance Bottlenecks

### Store Initialization Full Table Scan on Schema Migration

**Issue:** Schema migration loads entire version history unnecessarily
- Files: `infrastructure/runtime/src/mneme/store.ts` (lines 207-212)
- Impact: On startup, if 10+ migrations exist, all are queried and sorted even if DB is already at latest version
- Current: Always `SELECT ... ORDER BY version DESC LIMIT 1`, then filter migrations
- Concern: With thousands of agents, database initialization could stall
- Improvement: Query only latest version first, use fallback query if not found

### Distillation Pipeline No Streaming Feedback

**Issue:** Distillation blocks until complete, no progress indication
- Files: `infrastructure/runtime/src/distillation/pipeline.ts`
- Impact: Long distillations (10K+ messages → 500 tokens) give no feedback. UI shows hung. User may kill process thinking it's stuck.
- Current: Synchronous pipeline, events only at start/end
- Improvement: Add mid-pipeline events: `distill:extracting`, `distill:summarizing`, `distill:flushing` with `progressPct`. Allow cancellation via token.

### Token Estimation Cached But Never Invalidated

**Issue:** Session `tokenCountEstimate` updated on new messages but distillation doesn't refresh until next message
- Files: `infrastructure/runtime/src/mneme/store.ts` interface (line 18), usage in distillation pipeline
- Impact: After distillation reduces 5000 tokens to 500, the session's `tokenCountEstimate` is updated. But if no new messages arrive for 24 hours, `shouldDistill()` will return false even if concurrent messages spike it above threshold.
- Current: `shouldDistill()` checks `session.tokenCountEstimate >= threshold` with stale estimate
- Improvement: Recompute estimate when check is requested (or background task refreshes it hourly)

## Fragile Areas

### Execute Stage Tool Loop Complexity

**Issue:** Tool execution loop has multiple state transitions with nested conditionals
- Files: `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (lines 94-300+)
- Why fragile: 790 lines of async generator with streaming, tool calling, narration filtering, thinking blocks, plan proposals, approvals, all interleaved. Single off-by-one in state management breaks entire turn.
- Changes to add new state (e.g., "await user input") require careful threading through all branches
- Test coverage: Tool loop has tests but many branches untested (e.g., thinking block clearing with >8 tool calls)
- Safe modification:
  1. Extract streaming loop logic into separate pure function with signature `(event: StreamEvent) => TurnState | null`
  2. Add tests for each state transition: thinking→text, text→tool_call, tool_call→tool_result, etc.
  3. Use exhaustive type checking in switch statements

### Session Store Query Patterns

**Issue:** No query plan validation or index hit confirmation
- Files: `infrastructure/runtime/src/mneme/store.ts`
- Why fragile: Database has indexes on `nous_id`, `session_key`, `status`. Queries should use them but no verification. If someone adds a new query without index, performance silently degrades.
- Scenario: `findRecentSessions(nousId, limit: 1000)` without `ORDER BY created_at` returns rows in arbitrary order
- Safe modification:
  1. Add `EXPLAIN QUERY PLAN` assertions in tests for hot queries
  2. Maintain index list in code comment with rationale
  3. Review new store methods in code review for full-table scan risk

### Distillation Plugin Hook Execution

**Issue:** Plugins can throw or hang without affecting distillation outcome
- Files: `infrastructure/runtime/src/distillation/pipeline.ts`
- Why fragile: Plugin hooks (`onThreadSummaryUpdate`, memory flush plugins) can fail. Current: distillation succeeds if core logic succeeds, plugins are non-blocking.
- Concern: If plugin registers facts to memory but fetch fails (network timeout), memory gets inconsistent state
- Safe modification:
  1. Make plugin failures non-fatal but logged at `WARN` level with context
  2. Add retry mechanism: plugin errors trigger exponential backoff (1s, 2s, 4s) up to 3 attempts
  3. Store plugin execution state in database so failed plugins can be retried by background task

## Scaling Limits

### In-Memory Session Metadata

**Current capacity:** Estimated 10-100K active sessions before memory pressure
- Files: Core session tracking in `infrastructure/runtime/src/nous/manager.ts`
- Limit: Each session stores metadata in memory. With 100K agents each with 10 sessions, that's 1M session objects, ~500MB RAM
- Scaling path: Implement lazy-load session metadata. Only keep active session handles in memory. Query store for dormant sessions on demand.

### SQLite WAL File Growth

**Current capacity:** WAL files can grow unbounded if checkpoints don't occur
- Files: `infrastructure/runtime/src/mneme/store.ts` (line 181: `journal_mode = WAL`)
- Impact: After 24h of continuous writes, WAL may be 100MB+. Backup/sync becomes slow.
- Current: Relies on automatic checkpoints. Under high concurrency, checkpoints may be deferred.
- Scaling path: Add background task that forces checkpoint every hour: `PRAGMA wal_checkpoint(RESTART)`. Monitor WAL file size, alert if >500MB.

### MCP Session Connection Limits

**Current capacity:** 1000s of concurrent MCP sessions before event channel saturation
- Issue: Each session has its own `ReadableStreamDefaultController`. No pooling or multiplexing.
- Scaling path: Implement connection pool with max 100 concurrent SSE streams per auth client. Queue additional connections.

### Event Bus Unbounded Memory

**Issue:** Event listeners accumulate without cleanup
- Files: `infrastructure/runtime/src/koina/event-bus.ts`
- Current: `eventBus.on("event", handler)` adds listener, never removed
- Risk: If runtime starts/stops tools many times, listeners accumulate
- Scaling concern: With 100 agents each spawning 10 tools, 1000 listeners could be registered
- Fix: Add `.off()` method, ensure tool registration/cleanup unregisters listeners

## Dependencies at Risk

### anthropic-ai/sdk ^0.78.0

**Risk:** Loose semver allows breaking changes in minor versions
- Impact: SDK v0.80 could change streaming event types, breaking entire pipeline
- Current: No integration tests pinned to SDK version
- Migration plan: Pin to exact version in production (`0.78.0` not `^0.78.0`). Upgrade explicitly in development, run full test suite.

### better-sqlite3 ^12.6.2

**Risk:** Native module requires recompilation for each platform
- Impact: `npm ci` may fail on ARM64 or glibc mismatches. Binary artifacts must be committed or rebuild on deploy.
- Current: No prebuilt binary caching
- Migration plan: Use `better-sqlite3` with prebuilt binaries or migrate to `sql.js` (pure JS) for cloud deployments. For self-hosted, commit `.node` file.

## Missing Critical Features

### Distillation Cancellation

**Problem:** No way to stop distillation in progress. User must wait or restart process.
- Blocks: Can't implement user-cancellable operations (e.g., "cancel extraction step")
- Current: All distillation operations are synchronous, no AbortSignal support
- Fix: Thread AbortSignal through distillation pipeline. Interrupt LLM streams on cancellation.

### Distributed Session Lock

**Problem:** Session locking is in-memory. Multi-instance deployments see race conditions.
- Blocks: Can't scale to multiple runtime instances without data corruption
- Current: `activeDistillations` Set is global to process, not shared
- Impact: Two instances could distill same session simultaneously, overwriting each other's results
- Fix: Implement distributed lock via database (unique constraint on `session_id, lock_token`) or Redis

### Session Rollback Capability

**Problem:** If distillation fails mid-way, session state is partially updated with no rollback.
- Blocks: Can't guarantee data consistency on infrastructure failures
- Current: Distillation updates `messages`, `distillations`, `sessions` table without coordinated transaction
- Impact: If process crashes after updating messages but before updating session status, session is inconsistent
- Fix: Wrap entire distillation in database transaction. Test failure scenarios (network timeouts, OOM) to verify rollback.

## Test Coverage Gaps

### MCP Token Validation Edge Cases

**What's not tested:**
- `timingSafeEqual` with mismatched buffer lengths
- Token validation when `tokens` array is empty but `requireAuth=false`
- Multiple bearer tokens in Authorization header

**Files:** `infrastructure/runtime/src/pylon/mcp.ts`

**Risk:** Authentication bypass or denial of service

**Priority:** High

### Distillation Concurrent Execution

**What's not tested:**
- Two calls to `distillSession()` for same session simultaneously (should reject second)
- Distillation lock not released on exception
- Signal-CLI restart mid-distillation

**Files:** `infrastructure/runtime/src/distillation/pipeline.ts`

**Risk:** Data corruption, session lock deadlock

**Priority:** High

### Session Store Migration Failures

**What's not tested:**
- Schema version query throws exception (corrupted `schema_version` table)
- Migration SQL contains syntax error (caught at runtime, not load time)
- Database is corrupted when store opens (PRAGMA foreign_keys fails)

**Files:** `infrastructure/runtime/src/mneme/store.ts`

**Risk:** Startup failure, unrecoverable state

**Priority:** Medium

### Tool Timeout Boundary Conditions

**What's not tested:**
- Tool completes exactly at timeout boundary (1ms before timeout)
- Timeout fires, then tool completion arrives (race condition)
- Timeout for tool with 0ms timeout config (should disable timeout)

**Files:** `infrastructure/runtime/src/organon/timeout.ts`

**Risk:** Flaky test failures, tools incorrectly aborted

**Priority:** Medium

---

*Concerns audit: 2025-02-24*
