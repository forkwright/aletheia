# Spec: Tool Call Governance - Timeouts, Cancellation, and Visibility

**Author:** forkwright
**Date:** 2026-02-18  
**Status:** Draft  
**Mouseion:** #TBD  
**Depends on:** None (standalone, builds on existing loop-detector)

---

## Problem

When a tool call hangs or runs excessively long during a turn, both the human and the agent are locked out. The turn cannot complete, the human's "stop generation" button has no effect (it signals the LLM stream, not the tool executor), and there is no mechanism to cancel individual tool calls, timeout stale executions, or recover partial results from parallel calls.

### Current State

1. **No tool-level timeouts** - The `exec` tool has its own 30s default timeout, but the *framework* has no enforcement. If a tool implementation doesn't timeout internally (e.g., `sessions_ask` blocks for 120s, `web_fetch` could hang indefinitely), the turn blocks forever.

2. **No cancellation** - There is no `AbortController` or signal mechanism threaded through tool execution. The SSE stream in `pylon/ui.ts` detects client disconnect (`req.raw.signal.abort`) but only cleans up the event stream - it does NOT propagate cancellation to the running turn.

3. **No partial recovery** - Tools execute sequentially in the turn loop (`manager.ts` ~L540-610). If tool 3 of 5 hangs, tools 4 and 5 never execute. Even if they were parallel, there's no mechanism to return completed results while one is still pending.

4. **Limited visibility** - The SSE stream emits `tool_start` events but no elapsed-time updates. The webchat UI shows tool names but not duration. Neither human nor agent can see which specific call is the bottleneck.

5. **LoopDetector is post-hoc** - It catches *repetitive* patterns after execution, but can't help with a single call that simply never returns.

6. **"Stop generation" is a lie** - The webchat stop button (if implemented) can close the SSE connection, but the server-side turn continues to completion. The tool results get orphaned in history, the turn outcome gets swallowed, and the session state may become inconsistent.

### Impact

- Human loses control of the conversation mid-turn
- Agent context window fills with stale tool results from zombie turns  
- No graceful degradation - it's all-or-nothing
- The only recovery is runtime restart (which triggers amnesia per distillation spec)

---

## Design

### Principles

1. **Defense in depth** - Framework-level timeouts are a safety net; tool-level timeouts remain primary
2. **Cancellation is cooperative** - AbortSignal propagated; tools opt-in to checking it
3. **Partial results > no results** - If 2 of 3 parallel tools complete, return those
4. **Human authority** - Cancel button actually cancels. Period.
5. **Visibility is bidirectional** - Human sees tool status; agent gets timeout context

### Architecture Overview

```
┌─────────────┐     SSE events          ┌──────────────┐
│  Webchat UI  │◄──────────────────────── │  Pylon/UI    │
│              │                          │              │
│  [Cancel ✕]  │──── POST /abort-turn ──►│  abort(turn)  │
└─────────────┘                          └──────┬───────┘
                                                │ signal
                                         ┌──────▼───────┐
                                         │  NousManager  │
                                         │              │
                                         │  AbortCtrl ──┼──► ToolContext.signal
                                         │  Timeouts  ──┼──► per-tool enforcement
                                         │  Tracking  ──┼──► activeTurns registry
                                         └──────────────┘
```

---

## Implementation

### Phase 1: Framework Timeouts (Runtime)

**Goal:** No tool call can block a turn indefinitely.

#### 1.1 - Add `signal` to ToolContext

**File:** `infrastructure/runtime/src/organon/registry.ts`

```typescript
// Add to ToolContext interface:
export interface ToolContext {
  nousId: string;
  sessionId: string;
  workspace: string;
  allowedRoots: string[];
  depth: number;
  signal?: AbortSignal;  // NEW - cooperative cancellation
}
```

#### 1.2 - Add timeout wrapper to tool execution

**File:** `infrastructure/runtime/src/organon/registry.ts` (or new `infrastructure/runtime/src/organon/timeout.ts`)

```typescript
export interface ToolTimeoutConfig {
  /** Default timeout for all tools (ms). 0 = no timeout. */
  defaultMs: number;
  /** Per-tool overrides */
  overrides: Record<string, number>;
}

const DEFAULT_TOOL_TIMEOUTS: ToolTimeoutConfig = {
  defaultMs: 120_000,  // 2 minutes - generous but finite
  overrides: {
    exec: 0,           // exec has its own timeout param; don't double-wrap
    sessions_ask: 0,   // sessions_ask has its own timeout; don't double-wrap
    sessions_spawn: 0, // long-running by design
    browser: 180_000,  // browser actions can be slow
    web_fetch: 60_000, // network requests
    web_search: 60_000,
  },
};

/**
 * Wrap a tool execution with a framework-level timeout.
 * Returns the tool result or throws on timeout.
 * Does NOT cancel the underlying execution (that requires AbortSignal).
 */
export async function executeWithTimeout(
  handler: ToolHandler,
  input: Record<string, unknown>,
  context: ToolContext,
  timeoutMs: number,
): Promise<string> {
  if (timeoutMs <= 0) {
    return handler.execute(input, context);
  }

  let timer: ReturnType<typeof setTimeout>;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new ToolTimeoutError(handler.definition.name, timeoutMs)),
      timeoutMs,
    );
  });

  try {
    const result = await Promise.race([
      handler.execute(input, context),
      timeoutPromise,
    ]);
    clearTimeout(timer!);
    return result;
  } catch (err) {
    clearTimeout(timer!);
    throw err;
  }
}

export class ToolTimeoutError extends Error {
  constructor(
    public readonly toolName: string,
    public readonly timeoutMs: number,
  ) {
    super(`Tool "${toolName}" timed out after ${Math.round(timeoutMs / 1000)}s`);
    this.name = "ToolTimeoutError";
  }
}
```

#### 1.3 - Integrate timeout into turn loop

**File:** `infrastructure/runtime/src/nous/manager.ts` (~L540-610, tool execution section)

Current:
```typescript
try {
  toolResult = await this.tools.execute(toolUse.name, toolUse.input, toolContext);
} catch (err) {
  isError = true;
  toolResult = err instanceof Error ? err.message : String(err);
}
```

Proposed:
```typescript
try {
  const timeoutMs = resolveToolTimeout(toolUse.name, this.config);
  toolResult = await executeWithTimeout(
    /* handler resolved internally */ 
    this.tools, toolUse.name, toolUse.input, 
    { ...toolContext, signal: turnAbortController.signal },
    timeoutMs,
  );
} catch (err) {
  isError = true;
  if (err instanceof ToolTimeoutError) {
    toolResult = `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round(err.timeoutMs / 1000)}s. The operation may still be running in the background.`;
    log.warn(`Tool timeout: ${toolUse.name} after ${err.timeoutMs}ms [${nousId}]`);
  } else {
    toolResult = err instanceof Error ? err.message : String(err);
  }
}
```

The key insight: even without cooperative cancellation in every tool, the framework timeout ensures the **turn** progresses. The timed-out tool may leave a zombie process (exec) or dangling connection, but the turn isn't blocked.

#### 1.4 - Configuration

**File:** `infrastructure/runtime/src/taxis/schema.ts`

Add to `agents.defaults`:
```typescript
toolTimeouts?: {
  defaultMs?: number;         // default: 120000
  overrides?: Record<string, number>;
};
```

---

### Phase 2: Turn Cancellation (Runtime + API)

**Goal:** Human can abort a turn, and it actually stops.

#### 2.1 - Turn abort controller registry

**File:** `infrastructure/runtime/src/nous/manager.ts`

```typescript
// Add to NousManager class:
private turnAbortControllers = new Map<string, AbortController>();

// In executeTurnStreaming, before the tool loop:
const turnAbortController = new AbortController();
const turnId = `${nousId}:${sessionId}:${Date.now()}`;
this.turnAbortControllers.set(turnId, turnAbortController);

// Thread signal into tool context:
const toolContext: ToolContext = {
  nousId, sessionId, workspace,
  allowedRoots: [paths.root],
  depth: msg.depth ?? 0,
  signal: turnAbortController.signal,
};

// At the start of each tool execution, check for abort:
if (turnAbortController.signal.aborted) {
  // Inject synthetic results for remaining tools
  for (const remaining of toolUses.slice(toolIndex)) {
    toolResults.push({
      type: "tool_result",
      tool_use_id: remaining.id,
      content: "[CANCELLED] Turn aborted by user.",
      is_error: true,
    });
  }
  currentMessages = [...currentMessages, { role: "user", content: toolResults }];
  yield { type: "error", message: "Turn cancelled by user" };
  return;
}

// Cleanup on turn end:
this.turnAbortControllers.delete(turnId);
```

#### 2.2 - Expose active turns with abort capability

```typescript
// Add to NousManager:
getActiveTurnDetails(): Array<{
  turnId: string;
  nousId: string;
  sessionId: string;
  startedAt: number;
  currentTool?: string;
  toolCallCount: number;
}>;

abortTurn(turnId: string): boolean {
  const controller = this.turnAbortControllers.get(turnId);
  if (!controller) return false;
  controller.abort();
  return true;
}
```

#### 2.3 - API endpoint

**File:** `infrastructure/runtime/src/pylon/server.ts`

```typescript
app.get("/api/turns/active", (c) => {
  return c.json({ turns: manager.getActiveTurnDetails() });
});

app.post("/api/turns/:id/abort", (c) => {
  const id = c.req.param("id");
  const aborted = manager.abortTurn(id);
  if (!aborted) return c.json({ error: "Turn not found or already completed" }, 404);
  return c.json({ ok: true, turnId: id });
});
```

#### 2.4 - SSE abort signal propagation

When the webchat client disconnects mid-stream (SSE close), propagate abort to the turn:

**File:** `infrastructure/runtime/src/pylon/server.ts` (streaming endpoint)

```typescript
// In the /api/sessions/stream handler:
const turnAbortController = new AbortController();

c.req.raw.signal?.addEventListener("abort", () => {
  // Client disconnected - abort the turn
  turnAbortController.abort();
  log.info(`Client disconnected mid-turn [${agentId}:${resolvedSessionKey}]`);
});

// Pass abort signal through to manager (requires threading)
```

This is the trickiest part - the current streaming flow creates a `ReadableStream` that wraps the manager's async generator. We need to either:
- Pass an `AbortSignal` into `handleMessageStreaming()` and thread it to `executeTurnStreaming()`
- Or use the turn registry and abort by turnId after the `turn_start` event fires

The turn registry approach is cleaner (no API changes to `handleMessageStreaming`):

```typescript
// After receiving turn_start event in stream handler:
let activeTurnId: string | null = null;

for await (const event of manager.handleMessageStreaming({...})) {
  if (event.type === "turn_start") {
    activeTurnId = event.turnId; // NEW field
  }
  // ... enqueue to SSE
}

// On client abort:
c.req.raw.signal?.addEventListener("abort", () => {
  if (activeTurnId) manager.abortTurn(activeTurnId);
});
```

---

### Phase 3: Parallel Tool Execution with Partial Recovery (Runtime)

**Goal:** Execute independent tool calls in parallel; don't let one slow call block the others.

This is a larger change and should be a separate phase. Current behavior is sequential execution of all tool_use blocks in a single response. Anthropic's API allows the model to request multiple tool calls in one response, expecting all results.

#### 3.1 - Parallel execution with settled results

```typescript
// Replace sequential loop with Promise.allSettled:
const toolPromises = toolUses.map(async (toolUse, index) => {
  if (turnAbortController.signal.aborted) {
    return { toolUse, result: "[CANCELLED]", isError: true, durationMs: 0 };
  }
  
  const toolStart = Date.now();
  try {
    const timeoutMs = resolveToolTimeout(toolUse.name, this.config);
    const result = await executeWithTimeout(
      this.tools, toolUse.name, toolUse.input,
      { ...toolContext, signal: turnAbortController.signal },
      timeoutMs,
    );
    return { toolUse, result, isError: false, durationMs: Date.now() - toolStart };
  } catch (err) {
    return {
      toolUse,
      result: err instanceof ToolTimeoutError 
        ? `[TIMEOUT after ${Math.round(err.timeoutMs / 1000)}s]`
        : (err instanceof Error ? err.message : String(err)),
      isError: true,
      durationMs: Date.now() - toolStart,
    };
  }
});

const settled = await Promise.allSettled(toolPromises);
```

#### 3.2 - Stream tool results as they complete

For real-time feedback, we want to emit `tool_result` events as each tool completes, not batch them:

```typescript
// Use Promise.race in a drain loop:
const pending = new Map(toolPromises.map((p, i) => [i, p]));

while (pending.size > 0) {
  const { index, result } = await raceWithIndex(pending);
  pending.delete(index);
  
  // Emit SSE event immediately
  yield {
    type: "tool_result",
    toolName: result.toolUse.name,
    toolId: result.toolUse.id,
    result: result.result.slice(0, 2000),
    isError: result.isError,
    durationMs: result.durationMs,
  };
}
```

**Caution:** This changes the order of tool_result messages in the conversation history. The Anthropic API requires tool_results to be in a single user message following the assistant's tool_use response, but order within that message shouldn't matter. Needs testing.

#### 3.3 - Concurrency limits

Not all tools should run in parallel. Shell commands that modify the filesystem could conflict. Add a `concurrency` property to tool definitions:

```typescript
// In ToolDefinition:
concurrency?: "parallel" | "exclusive";  // default: "parallel"

// Tools that should be exclusive:
// - exec (filesystem side effects)
// - write/edit (file mutation)
// - workspace-git operations
```

---

### Phase 4: UI Visibility (Webchat)

**Goal:** Human sees real-time tool call status and can cancel individual calls or the entire turn.

#### 4.1 - Enhanced SSE events

Add new event types to the streaming protocol:

```typescript
// New events:
type: "tool_progress"   // Periodic elapsed-time update for running tools
  { toolId, toolName, elapsedMs, status: "running" }

type: "tool_timeout"    // Tool hit its timeout limit
  { toolId, toolName, timeoutMs }

type: "turn_abort"      // Turn was cancelled
  { reason: "user" | "timeout" | "error" }
```

The `tool_progress` events can be emitted on a 5-second interval by the turn loop for any tool that's been running > 5s. Lightweight - just a timestamp check.

#### 4.2 - Webchat tool status display

The existing `ToolStatusLine` component needs:
- Elapsed time counter (client-side, started on `tool_start`, stopped on `tool_result`)
- Color coding: green (< 10s), yellow (10-30s), red (> 30s)
- Individual cancel button per tool (POST `/api/turns/:id/abort-tool/:toolId` - future)
- Turn-level cancel button (POST `/api/turns/:id/abort`)

#### 4.3 - Turn cancel button (replaces stop generation)

```svelte
{#if activeTurn}
  <button class="cancel-turn" on:click={abortTurn}>
    ✕ Cancel Turn
  </button>
{/if}
```

This button:
1. Sends POST `/api/turns/{turnId}/abort`
2. Closes the SSE stream
3. Shows "Turn cancelled" in the chat

---

### Phase 5: Agent Awareness (Runtime)

**Goal:** Agents can reason about tool call duration and act accordingly.

#### 5.1 - Timeout context in tool results

When a tool times out, the error message should be descriptive enough for the model to reason about it:

```
[TIMEOUT] Tool "exec" did not respond within 120s. 
The command may still be running. Consider:
- Checking if the process completed: `ps aux | grep ...`
- Using a shorter timeout
- Breaking the operation into smaller steps
```

#### 5.2 - `session_status` tool enhancement

The existing `session_status` tool could be enhanced to show:
- Current turn's tool call count
- Any timed-out tools in the current session
- Overall session health

#### 5.3 - Pre-flight timeout hints

Allow agents to request custom timeouts on a per-call basis:

```typescript
// In exec tool, this already exists (timeout param).
// For other tools, add optional _timeout field:
{
  "command": "npm run build",
  "timeout": 300000,  // 5 minutes - agent knows this is slow
  "_meta": { "timeout": 300000 }  // Alternative: meta field for any tool
}
```

---

## Implementation Order

| Phase | Scope | Effort | Risk | Priority |
|-------|-------|--------|------|----------|
| **1: Framework Timeouts** | Runtime only | Small | Low - additive, no behavior change for tools that return quickly | **P0** - fixes the core hang |
| **2: Turn Cancellation** | Runtime + API | Medium | Medium - abort signal threading touches the hot path | **P0** - restores human control |
| **3: Parallel Execution** | Runtime | Large | High - changes tool execution order, history format | **P1** - optimization |
| **4: UI Visibility** | Webchat + Runtime | Medium | Low - purely additive | **P1** - usability |
| **5: Agent Awareness** | Runtime | Small | Low - message formatting only | **P2** - nice-to-have |

**Recommended:** Ship Phases 1+2 together. They're complementary and address both sides of the problem (automatic recovery + manual intervention).

---

## Risk Analysis

### Zombie processes
Framework timeout doesn't kill the underlying operation. A timed-out `exec` call leaves the child process running. Mitigation: exec tool already uses `child_process` with its own timeout that sends SIGTERM. For other tools, the AbortSignal allows cooperative cleanup.

### History consistency
Aborting mid-turn could leave the conversation history in a weird state (assistant message with tool_use blocks, but no corresponding tool_results). The existing orphan repair code in `manager.ts` (~L1301) handles this by injecting synthetic results. Verify this covers the abort case.

### Parallel execution ordering
Anthropic expects tool_results in a single user message. Parallel execution changes the order but not the structure. Test that the API accepts results in any order within the message.

### Race conditions on abort
Between checking `signal.aborted` and starting tool execution, the signal could fire. Use `signal.throwIfAborted()` at the start of execution for immediate detection.

### Double-abort
User clicks cancel multiple times. AbortController handles this gracefully - calling `abort()` on an already-aborted controller is a no-op.

---

## Test Cases

### Phase 1: Timeouts
1. Tool exceeding default timeout → returns timeout error, turn continues
2. Tool with custom timeout override → uses override, not default
3. Tool with timeout=0 → no framework timeout applied
4. Timeout error message includes tool name and duration
5. LoopDetector still fires for repetitive calls (no conflict with timeouts)

### Phase 2: Cancellation
1. `POST /api/turns/:id/abort` → turn stops, synthetic results injected
2. Client SSE disconnect → turn aborted automatically
3. Abort during first tool of multi-tool response → remaining tools skipped
4. Abort when no turn active → 404
5. Double-abort → idempotent
6. History has valid tool_result for every tool_use after abort

### Phase 3: Parallel
1. Three independent tools → all execute concurrently
2. One of three times out → other two results preserved
3. Exclusive tool (exec) → runs alone, not parallelized
4. Tool results arrive in correct order in history
5. Stream events emitted as each tool completes

---

## Open Questions

1. **Should abort kill exec child processes?** The framework timeout just stops waiting. Should we also send SIGTERM to any running child process? Probably yes - track PIDs in the exec tool and clean up on abort signal.

2. **Per-tool cancel vs. turn cancel?** Phase 2 only implements turn-level abort. Individual tool cancellation is more complex (need to continue the turn with partial results). Worth doing?

3. **Configurable via UI?** Should timeout values be adjustable from the webchat settings panel? Or config-file only?

4. **What about cross-agent calls?** If Syn asks Eiron something and the human cancels Syn's turn, should Eiron's in-progress turn also abort? Probably yes - the parent abort should propagate to child turns.

---

## Files Modified

| File | Change |
|------|--------|
| `organon/registry.ts` | Add `signal` to ToolContext interface |
| `organon/timeout.ts` | **NEW** - executeWithTimeout, ToolTimeoutError, config |
| `nous/manager.ts` | AbortController registry, timeout wrapping, abort check in tool loop |
| `pylon/server.ts` | `/api/turns/active`, `/api/turns/:id/abort` endpoints, SSE abort propagation |
| `pylon/ui.ts` | New SSE event types (tool_progress, tool_timeout, turn_abort) |
| `taxis/schema.ts` | toolTimeouts config section |
| `taxis/loader.ts` | Default timeout config |

---

*This spec addresses Mouseion TBD. Implementation should start with Phases 1+2 as a single PR.*
