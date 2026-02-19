# Spec: Turn Safety — Error Propagation, Distillation Guards, and Orphan Prevention

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

Signal messages silently fail. The user sends a message, sees no error, gets no response. The runtime logs show context preparation starting but no execute stage, no error, no timeout — the turn vanishes.

Root cause analysis of the 2026-02-19 incident reveals three compounding failures:

1. **No error propagation in the pipeline runner.** The runner has zero try/catch around stages. When `prepareHistory()` or `buildContext()` throws, the error propagates up to `withSessionLock`, which resolves the lock promise via `.then(fn, fn)` — meaning the *next* queued turn runs immediately after the failure, inheriting whatever corrupted state exists.

2. **Distillation fires during or immediately after a failed turn.** The finalize stage triggers auto-distillation when token count exceeds threshold. If a turn partially completes (assistant message with tool_use blocks written to DB, but tool execution never happens), distillation marks those messages as distilled. The orphaned tool_use blocks persist in the post-distillation history because `preserveRecentMessages` keeps the tail, and the synthetic repair in `buildMessages` fires on every subsequent turn but produces a message sequence the API struggles with.

3. **Silent error swallowing in the Signal listener.** The `processTurn` catch handler sends a generic error message to the user, but `handleMessage` (buffered path) throws through `withSessionLock` which swallows via `.then(fn, fn)`. The error either never reaches `processTurn`'s catch, or the session lock's error-path resolution means the next queued turn starts before the error propagates.

### Observed Failure Chain (2026-02-19)

```
16:07:40  User sends "Ok. first, there are a number of prs..."
          → Pipeline starts, context stage succeeds
          → History stage: buildMessages repairs 4 orphaned tool_use blocks
          → Turn enters tool loop, executes gh commands, reviews PRs
          → Assistant responds at 16:08:25 with PR #31 review + starts PR #32 diff fetch

16:08:26  Tool result for PR #32 diff arrives (seq 538)
          → Turn is mid-execution, awaiting next LLM response

16:09:35  User sends "merged?" — NEW TURN ENTERS PIPELINE
          → withSessionLock queues behind active turn
          → Active turn completes (or errors) → queued turn starts
          → buildMessages finds orphaned tool_use blocks, repairs them
          → Something in the repaired history is malformed
          → Pipeline throws between history and execute stages
          → NO LOG EMITTED — error swallowed by lock chain
          → User gets no response

16:11:12  User sends "merged?" again — same pattern, same silent death

16:11:25  Distillation fires (seq 541) — marks messages through 537 as distilled
          → Post-distillation history still contains orphaned tail
          → All subsequent turns hit the same orphan repair → malformed history → silent death

16:18:47  Runtime restarts — clears in-memory state
16:19:18  Next message succeeds (post-restart history is clean enough)
```

---

## Design Principles

### Every turn must produce a visible outcome

A turn either returns a response, returns an explicit error message, or logs a categorized failure. "Nothing happened" is never acceptable. If the pipeline cannot produce a response, it must tell the user and log why.

### Distillation is a maintenance operation, not a combat operation

Distillation must never fire while a turn is in-flight. It must never fire when the most recent message sequence is incomplete (assistant tool_use without matching tool_result). It should fire between turns, never during.

### Orphan repair is a safety net, not a feature

The current orphan repair in `buildMessages` silently patches broken history on every turn. This masks bugs instead of fixing them. Orphans should be prevented, not repaired. When orphans exist, they should be logged as warnings with context about *how* they occurred.

### Errors propagate, always

No stage should swallow errors. No lock mechanism should eat rejections. Every error from every stage should reach the caller with enough context to diagnose.

---

## Changes

### Phase 1: Pipeline Error Boundaries

**File:** `infrastructure/runtime/src/nous/pipeline/runner.ts`

Wrap each stage in try/catch with stage identification. Emit structured error events.

```typescript
// Streaming pipeline
export async function* runStreamingPipeline(msg, services, opts) {
  const state = resolveStage(msg, services, opts?.abortSignal);
  if (!state) { /* existing handling */ }

  const turnId = opts?.turnId ?? `${state.nousId}:${state.sessionId}:${Date.now()}`;
  yield { type: "turn_start", sessionId: state.sessionId, nousId: state.nousId, turnId };

  try {
    const refusal = checkGuards(state, services);
    if (refusal) { /* existing handling */ }

    await buildContext(state, services);
    await prepareHistory(state, services);

    const finalState = yield* executeStreaming(state, services);

    if (finalState.outcome) {
      await finalize(finalState, services);
      yield { type: "turn_complete", outcome: finalState.outcome };
    }
    return finalState.outcome;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    const stage = identifyFailedStage(state);
    log.error(`Pipeline failed at ${stage}: ${message}`, { nousId: state.nousId, sessionId: state.sessionId, stage });
    if (err instanceof Error && err.stack) log.error(err.stack);

    yield { type: "error", message: `Turn failed at ${stage}: ${message}` };
    return undefined;
  }
}

// Buffered pipeline — same pattern
export async function runBufferedPipeline(msg, services) {
  const state = resolveStage(msg, services);
  if (!state) throw new Error(`Unknown nous: ${msg.nousId ?? "default"}`);

  try {
    const refusal = checkGuards(state, services);
    if (refusal) return refusal.outcome;

    await buildContext(state, services);
    await prepareHistory(state, services);

    const finalState = await executeBuffered(state, services);

    if (finalState.outcome) {
      await finalize(finalState, services);
    }
    if (!finalState.outcome) throw new Error("Turn produced no outcome");
    return finalState.outcome;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    const stage = identifyFailedStage(state);
    log.error(`Pipeline failed at ${stage}: ${message}`, { nousId: state.nousId, sessionId: state.sessionId, stage });
    if (err instanceof Error && err.stack) log.error(err.stack);

    // Return a synthetic error outcome so the caller always gets something
    return {
      text: "",
      nousId: state.nousId,
      sessionId: state.sessionId,
      toolCalls: state.totalToolCalls,
      inputTokens: state.totalInputTokens,
      outputTokens: state.totalOutputTokens,
      cacheReadTokens: state.totalCacheReadTokens,
      cacheWriteTokens: state.totalCacheWriteTokens,
      error: message,
    } satisfies TurnOutcome;
  }
}
```

**Stage identification** uses markers set on state as each stage completes:

```typescript
function identifyFailedStage(state: Partial<TurnState>): string {
  if (!state.systemPrompt) return "context";
  if (!state.messages) return "history";
  if (!state.outcome) return "execute";
  return "finalize";
}
```

**TurnOutcome gains an optional `error` field:**

```typescript
export interface TurnOutcome {
  text: string;
  nousId: string;
  sessionId: string;
  toolCalls: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  error?: string;  // NEW: set when pipeline fails
}
```

**Impact:** Every pipeline failure produces a log line with stage identification. Buffered callers (Signal) always get a TurnOutcome, never an unhandled throw that disappears into the lock chain.

### Phase 2: Session Lock Fix

**File:** `infrastructure/runtime/src/nous/manager.ts`

The current lock implementation:

```typescript
function withSessionLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const previous = sessionLocks.get(key) ?? Promise.resolve();
  const current = previous.then(fn, fn);  // ← fn runs on BOTH resolve and reject
  // ...
}
```

The `.then(fn, fn)` pattern means if the previous turn rejected, `fn` (the next turn) still runs immediately. This is by design — it prevents deadlocks. But it means errors from turn N are lost; turn N+1 starts on the rejection path and its result replaces the error.

**Fix:** Keep the deadlock prevention but capture and log rejected turns:

```typescript
function withSessionLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const previous = sessionLocks.get(key) ?? Promise.resolve();
  const current = previous.then(
    () => fn(),
    (prevErr) => {
      log.warn(`Previous turn on lock ${key} failed: ${prevErr instanceof Error ? prevErr.message : prevErr}`);
      return fn();
    },
  );
  sessionLocks.set(key, current.catch(() => {})); // prevent unhandled rejection, store settled promise
  current.finally(() => {
    if (sessionLocks.get(key) === current) sessionLocks.delete(key);
  }).catch(() => {});
  return current;
}
```

**Impact:** Failed turns are logged instead of silently replaced by the next turn. The lock chain still prevents deadlocks.

### Phase 3: Distillation Guards

**File:** `infrastructure/runtime/src/nous/pipeline/stages/finalize.ts`

#### Guard 1: No distillation during active turns

The finalize stage runs *after* the current turn completes, so the current turn itself isn't the problem. The problem is when **another session on the same nous** triggers distillation, or when the session receives a new message during distillation. Add a turn-in-flight check:

```typescript
// Before distillation trigger
if (services.manager?.hasActiveTurn(sessionId)) {
  log.info(`Skipping distillation for ${sessionId} — turn in progress`);
  return;
}
```

**NousManager exposes:**

```typescript
hasActiveTurn(sessionId: string): boolean {
  return [...this.turnMeta.values()].some(m => m.sessionId === sessionId);
}
```

Note: Since finalize runs at the end of a turn, `hasActiveTurn` should return false for the current turn (which is completing). If another concurrent turn on the same session exists (shouldn't happen with session locks, but defensive), this catches it.

#### Guard 2: No distillation with incomplete message sequences

**File:** `infrastructure/runtime/src/distillation/pipeline.ts`

Before running distillation, validate the message tail:

```typescript
function validateHistoryForDistillation(messages: Message[]): { valid: boolean; reason?: string } {
  // Check that the last assistant message doesn't have unresolved tool_use blocks
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!;
    if (msg.role === "assistant") {
      try {
        const content = JSON.parse(msg.content);
        if (Array.isArray(content)) {
          const toolUses = content.filter((b: { type: string }) => b.type === "tool_use");
          if (toolUses.length > 0) {
            // Check if the next message(s) contain matching tool_results
            const answeredIds = new Set<string>();
            for (let j = i + 1; j < messages.length; j++) {
              const next = messages[j]!;
              if (next.role === "tool_result" && next.toolCallId) {
                answeredIds.add(next.toolCallId);
              }
            }
            const unanswered = toolUses.filter((b: { id: string }) => !answeredIds.has(b.id));
            if (unanswered.length > 0) {
              return {
                valid: false,
                reason: `${unanswered.length} unanswered tool_use blocks in recent history (last assistant msg at seq ${msg.seq})`,
              };
            }
          }
        }
      } catch {
        // Not JSON — text-only assistant message, fine
      }
      break; // Only check the most recent assistant message
    }
  }
  return { valid: true };
}
```

**Usage in `runDistillation`:**

```typescript
const history = store.getHistory(sessionId, { excludeDistilled: true });
const validation = validateHistoryForDistillation(history);
if (!validation.valid) {
  log.warn(`Distillation blocked for ${sessionId}: ${validation.reason}`);
  return { skipped: true, reason: validation.reason };
}
```

**Impact:** Distillation never runs on a session with orphaned tool_use blocks. The orphans must be resolved (by the next turn completing, or by manual cleanup) before distillation proceeds.

#### Guard 3: Distillation acquires the session lock

Distillation should go through the same session lock as turns. This prevents the race where a new message arrives during distillation:

**File:** `infrastructure/runtime/src/nous/pipeline/stages/finalize.ts`

```typescript
// Instead of directly calling distillSession:
const lockKey = `${nousId}:${sessionKey}`;
withSessionLock(lockKey, async () => {
  await distillSession(services.store, services.router, sessionId, nousId, { ... });
}).catch((err) => {
  log.warn(`Distillation failed: ${err instanceof Error ? err.message : err}`);
});
```

Wait — finalize itself runs inside a session lock (the turn holds it). Distillation must run *after* the lock is released. Change to a deferred approach:

```typescript
// In finalize: schedule, don't execute
if (shouldDistill) {
  services.manager?.scheduleDistillation(sessionId, nousId, distillOpts);
}
```

**NousManager adds:**

```typescript
scheduleDistillation(sessionId: string, nousId: string, opts: DistillationOpts): void {
  const lockKey = /* derive from session */;
  // Run after current lock releases by chaining onto the lock
  withSessionLock(lockKey, async () => {
    const validation = validateHistoryForDistillation(this.store.getHistory(sessionId));
    if (!validation.valid) {
      log.warn(`Deferred distillation skipped: ${validation.reason}`);
      return;
    }
    await distillSession(this.store, this.router, sessionId, nousId, opts);
  }).catch((err) => {
    log.warn(`Deferred distillation failed: ${err instanceof Error ? err.message : err}`);
  });
}
```

**Impact:** Distillation waits for the current turn to fully release the session lock, then acquires it before modifying history. No new turn can start during distillation. No distillation can start during a turn.

### Phase 4: Orphan Prevention and Improved Repair

**File:** `infrastructure/runtime/src/nous/pipeline/utils/build-messages.ts`

#### 4a: Log orphan context

When orphans are detected, log which tool_use blocks are orphaned and their position in history:

```typescript
if (orphaned.length > 0) {
  const details = orphaned.map(b => `${b.name}(${b.id})`).join(", ");
  log.warn(
    `Repairing ${orphaned.length} orphaned tool_use block(s) in history: ${details}. ` +
    `This indicates a turn was interrupted without completing tool execution.`
  );
}
```

#### 4b: Track orphan frequency per session

Add an event emission so monitoring can detect sessions that repeatedly hit orphan repair:

```typescript
if (orphaned.length > 0) {
  eventBus.emit("history:orphan_repair", {
    count: orphaned.length,
    tools: orphaned.map(b => b.name),
  });
}
```

#### 4c: Ensure synthetic tool_results produce valid Anthropic message structure

The current repair creates synthetic results with the content `"Error: Tool execution interrupted — service restarted mid-turn."`. Verify this produces a valid message sequence:

- If the orphaned tool_use is in the last assistant message, the synthetic result must create a valid user message *before* the current user message (not merged into it if the content types conflict).
- After repair, validate alternating roles: assistant → user → assistant → user.

```typescript
// After all repairs, validate alternating roles
for (let k = 1; k < messages.length; k++) {
  if (messages[k]!.role === messages[k - 1]!.role) {
    log.error(
      `Message sequence violation after orphan repair: ` +
      `messages[${k-1}].role=${messages[k-1]!.role}, messages[${k}].role=${messages[k]!.role}`
    );
    // Force merge if both are user messages
    if (messages[k]!.role === "user") {
      // Merge logic (already exists below, but may not catch post-repair violations)
    }
  }
}
```

### Phase 5: Signal Listener Error Visibility

**File:** `infrastructure/runtime/src/semeion/listener.ts`

The `processTurn` catch handler already sends an error message to the user. But the error may never reach it if `handleMessage` fails inside the session lock. Fix `handleMessage` to always return a TurnOutcome (Phase 1 handles this), and ensure `processTurn` handles the `error` field:

```typescript
async function processTurn(manager, msg, client, target) {
  try {
    const outcome = await manager.handleMessage(msg);

    if (outcome.error) {
      log.error(`Turn completed with error: ${outcome.error}`, {
        nousId: outcome.nousId,
        sessionId: outcome.sessionId,
      });
      await sendMessage(client, target, "Something went wrong processing that. Try again?", { markdown: false });
      return;
    }

    sendTyping(client, target, true).catch(() => {});
    if (outcome.text) {
      await sendMessage(client, target, outcome.text);
    }
    // ...
  } catch (err) {
    // This should now be rare — Phase 1 catches most errors
    log.error(`Turn failed (uncaught): ${err instanceof Error ? err.message : err}`);
    // ...
  }
}
```

---

## Implementation Order

| Phase | Effort | Risk | Impact |
|-------|--------|------|--------|
| **1: Pipeline error boundaries** | Small | Low | Eliminates silent failures — every turn produces a log |
| **2: Session lock fix** | Tiny | Low | Previous-turn errors are logged, not swallowed |
| **3: Distillation guards** | Medium | Medium | Prevents the orphan → distillation → permanent corruption chain |
| **4: Orphan repair improvements** | Small | Low | Better diagnostics, correct message sequences |
| **5: Signal error visibility** | Small | Low | Users see errors instead of silence |

**Recommended:** Implement 1 → 2 → 5 → 4 → 3. Phases 1-2 are the most critical (eliminate silent failures and error swallowing). Phase 5 ensures Signal users see something. Phase 4 improves diagnostics. Phase 3 is the most complex and prevents the specific distillation-corruption chain.

---

## Testing

### Unit Tests

- **Pipeline error boundaries:** Mock a stage to throw, verify error is caught, logged, and returned as TurnOutcome with `error` field.
- **Session lock:** Queue two turns, make the first throw, verify the second still runs and the first's error is logged.
- **Distillation validation:** Create a session with orphaned tool_use, verify `validateHistoryForDistillation` returns invalid.
- **Distillation scheduling:** Verify distillation waits for session lock release and runs after.
- **Orphan repair:** Verify repaired message sequences pass Anthropic's alternating-role validation.

### Integration Tests

- **Signal round-trip with error:** Send a message that triggers a pipeline error, verify the user receives an error message (not silence).
- **Rapid-fire messages:** Send 3 messages in quick succession to the same session, verify all produce responses (possibly queued).
- **Distillation under load:** Fill a session to distillation threshold, send a message simultaneously, verify the turn completes before distillation runs.

### Regression Test (the 2026-02-19 scenario)

1. Create a session with high token count (near distillation threshold)
2. Start a turn with multi-tool execution
3. Mid-turn, inject a second message to the same session key
4. Verify: first turn completes, second turn queues and completes, distillation fires after both, no orphaned blocks, no silent failures

---

## Success Criteria

- **Zero silent failures.** Every turn produces either a response, an error message to the user, or a categorized error log. `grep -c "Pipeline failed at" logs` should equal the number of failed turns.
- **No distillation on dirty history.** Distillation never runs when the most recent assistant message has unanswered tool_use blocks.
- **Orphan rate trends to zero.** The `history:orphan_repair` event count should decrease over time as the prevention mechanisms work. Target: zero orphan repairs on sessions that haven't experienced a crash.
- **Signal response rate ≥ 99%.** Every Signal message that passes the mention/contact gate should produce a visible response within 120 seconds.
