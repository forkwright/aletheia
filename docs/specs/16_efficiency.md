# Spec: Efficiency — Parallel Execution & Token Economy

**Status:** Phases 1, 2a-2c done. Phase 2d next.
**Author:** Syn
**Date:** 2026-02-21

---

## Problem

Two categories of waste compound across every turn:

### 1. Sequential Tool Execution

The Anthropic API supports returning **multiple `tool_use` blocks** in a single response — the model can request several tools at once when the calls are independent. Our runtime executes them sequentially:

```typescript
// execute.ts — current behavior
for (let toolIdx = 0; toolIdx < toolUses.length; toolIdx++) {
  const toolUse = toolUses[toolIdx]!;
  // ... approval check, timeout, execute, store result
  // Each tool waits for the previous one to finish
}
```

When the model requests 5 independent `grep` calls (different patterns, different paths), they run one after another. Each takes 0.5-2s. Total: 2.5-10s sequential. Parallel: 2s max. This compounds on tool-heavy turns — a 28-tool turn that could finish in 8s takes 40s.

The model already signals parallelism intent by returning multiple `tool_use` blocks in a single response. We're ignoring that signal.

### 2. Token Waste

Token waste manifests at multiple layers:

**a) Prompt bloat — system prompt + bootstrap**

The system prompt includes SOUL.md, USER.md, AGENTS.md, TOOLS.md, MEMORY.md, PROSOCHE.md, CONTEXT.md, working state, agent notes, and thread summary. For Syn (the heaviest agent), this can exceed 15K tokens before a single message. Much of this content is static between turns — unchanged SOUL.md, unchanged USER.md — yet it's sent every time. Anthropic's prompt caching mitigates this (cache hit rate ~60-70%), but the content itself hasn't been audited for density.

Questions:
- How many tokens is each bootstrap section? What's the breakdown?
- Are there sections that could be summarized or trimmed without losing value?
- Is USER.md duplicated across agents (it's symlinked, so yes — but is the full content needed for every agent)?
- Are tool definitions bloated? Each tool carries a full JSON schema description.

**b) Tool result verbosity**

Tool results are stored verbatim. A `grep` that returns 50 matches stores all 50 lines. A `read` of a 500-line file stores all 500 lines. An `exec` running `npm run build` stores the entire build log. These accumulate in the message history and consume context window on every subsequent API call.

The runtime already has `clear_tool_uses` context management (clearing old tool results at 60% context window, keeping last 8). But this is reactive — it only kicks in when context is already bloated. Proactive truncation at storage time would reduce baseline context size.

**c) Thinking budget vs. value**

Extended thinking uses a budget (currently uncapped for Opus, ~10K for Sonnet). On simple turns ("what time is it?"), the model still thinks for 500+ tokens. On complex turns, thinking is genuinely valuable. There's no dynamic adjustment — every turn gets the same thinking budget regardless of complexity.

**d) Redundant recall / bootstrap**

The recall stage fetches memories from Mem0 on every turn. For rapid-fire tool loops (model responds, tools execute, model responds again), the recall runs once at the start but the same memories are injected into every loop iteration as part of the system prompt. The memories don't change between loops — they're wasted tokens on iterations 2+.

Similarly, the bootstrap (SOUL.md, etc.) is assembled once per turn but cached by Anthropic. The concern isn't the bootstrap itself but whether it's optimally dense.

### 3. Parallel Sub-Agent Dispatch

Spec 13 (Sub-Agent Workforce) defines delegation to sub-agents. Currently, sub-agents are spawned sequentially — even when tasks are independent. The dispatch framework should support parallel spawn with result aggregation.

---

## Design

### Phase 1: Parallel Tool Execution ✅

**Goal:** Execute independent tool calls concurrently when the model returns multiple `tool_use` blocks.

#### Safety model

Not all tools are safe to parallelize. Two tools writing to the same file must be sequential. Two `grep` calls are always safe. The key insight: **read-only tools are always parallelizable. Write tools need conflict detection.**

```typescript
type ToolParallelism = "always" | "never" | "conditional";

const TOOL_PARALLELISM: Record<string, ToolParallelism> = {
  // Always safe — read-only, no side effects
  read: "always",
  grep: "always",
  find: "always",
  ls: "always",
  mem0_search: "always",
  web_search: "always",
  web_fetch: "always",
  blackboard: "conditional",  // read is safe, write needs serialization
  note: "always",             // append-only
  enable_tool: "always",

  // Never parallel — side effects, ordering matters
  exec: "never",              // Commands can affect each other
  write: "conditional",       // Safe if different paths
  edit: "conditional",        // Safe if different paths
  sessions_send: "always",    // Fire-and-forget to different agents
  sessions_ask: "always",     // Independent queries
  sessions_spawn: "always",   // Independent workers
  message: "never",           // Message ordering matters
  voice_reply: "never",       // Same
};
```

**Conditional parallelism** means "parallel unless they conflict." For `write` and `edit`, conflict = same file path. For `blackboard`, conflict = same key with write action.

```typescript
function canParallelize(tools: ToolUseBlock[]): ToolUseBlock[][] {
  if (tools.length <= 1) return [tools];

  const groups: ToolUseBlock[][] = [];
  const batch: ToolUseBlock[] = [];
  const touchedPaths = new Set<string>();

  for (const tool of tools) {
    const parallelism = TOOL_PARALLELISM[tool.name] ?? "never";

    if (parallelism === "never") {
      // Flush current batch, run this tool alone, then start new batch
      if (batch.length > 0) groups.push([...batch]);
      batch.length = 0;
      touchedPaths.clear();
      groups.push([tool]);
      continue;
    }

    if (parallelism === "conditional") {
      const path = extractPath(tool);
      if (path && touchedPaths.has(path)) {
        // Conflict — flush batch, start new one
        if (batch.length > 0) groups.push([...batch]);
        batch.length = 0;
        touchedPaths.clear();
      }
      if (path) touchedPaths.add(path);
    }

    batch.push(tool);
  }

  if (batch.length > 0) groups.push(batch);
  return groups;
}
```

**Execution:** Within each batch, tools run concurrently via `Promise.allSettled`. Batches execute sequentially:

```typescript
for (const batch of batches) {
  if (batch.length === 1) {
    // Single tool — execute normally (no overhead)
    await executeSingle(batch[0]);
  } else {
    // Parallel batch — all tools at once, collect results
    const results = await Promise.allSettled(
      batch.map(tool => executeSingle(tool))
    );
    // Process results in original order (preserve tool_result ordering)
    for (let i = 0; i < results.length; i++) {
      // yield tool_result events, store results, etc.
    }
  }
}
```

**SSE streaming:** Tool events (`tool_start`, `tool_result`) are yielded as they complete. Parallel tools will interleave their events — `tool_start A`, `tool_start B`, `tool_result B` (fast one finishes first), `tool_result A`. The UI already handles this since `ToolCallState` is tracked by ID.

**Approval gates:** If ANY tool in a parallel batch requires approval, extract it from the batch and run it separately. Don't block the whole batch waiting for user approval on one tool.

#### Metrics

Track parallel execution savings:

```typescript
interface ParallelMetrics {
  batchCount: number;        // How many parallel batches
  maxBatchSize: number;      // Largest batch
  sequentialMs: number;      // Sum of all tool durations
  parallelMs: number;        // Wall clock time for parallel execution
  savedMs: number;           // sequentialMs - parallelMs
}
```

Log these per-turn. This gives concrete data on how much time parallel execution saves.

### Phase 2: Token Audit

**Goal:** Measure, then reduce token waste at every layer.

#### 2a: Bootstrap measurement

Build a one-shot audit tool that measures each bootstrap section:

```bash
aletheia audit-tokens [agent-id]
```

Output:
```
Bootstrap Token Audit — Syn
────────────────────────────
SOUL.md           2,847 tokens
USER.md           1,923 tokens
AGENTS.md         3,412 tokens
TOOLS.md            891 tokens
MEMORY.md         1,256 tokens
PROSOCHE.md         234 tokens
CONTEXT.md          189 tokens
Working State       412 tokens
Agent Notes         567 tokens
Thread Summary      834 tokens
Tool Definitions  4,200 tokens  (42 tools)
────────────────────────────
Total Bootstrap  16,765 tokens
Cache hit rate      67%
Effective cost   5,532 tokens  (after cache)
```

This gives us the data to make trim decisions. No optimization without measurement.

#### 2b: Bootstrap density audit

With measurements in hand, evaluate each section:

- **SOUL.md:** The identity document. Should be dense but probably has redundancy with AGENTS.md. Audit overlap.
- **USER.md:** Symlinked to all agents. Does Akron need Cody's full reading list? Probably not. Consider agent-specific USER.md excerpts — each agent gets the sections relevant to their domain.
- **AGENTS.md:** The operations template. Already generated from sections. Audit each section for density.
- **Tool definitions:** Each tool has a JSON schema. Some descriptions are verbose. Trim descriptions to the minimum needed for correct usage.
- **MEMORY.md:** Only loaded in main sessions (already gated). Audit for stale entries.
- **Thread summary:** Generated from conversation history. If it's mostly boilerplate, reduce.

#### 2c: Tool result truncation

Implement proactive truncation at storage time:

```typescript
const TOOL_RESULT_LIMITS: Record<string, number> = {
  exec: 8000,       // Build logs, command output
  read: 10000,      // File contents
  grep: 5000,       // Search results
  find: 3000,       // File listings
  ls: 2000,         // Directory listings
  web_fetch: 8000,  // Web page content
  web_search: 4000, // Search results
  default: 5000,
};

function truncateResult(toolName: string, result: string): string {
  const limit = TOOL_RESULT_LIMITS[toolName] ?? TOOL_RESULT_LIMITS.default;
  if (result.length <= limit) return result;

  // Keep head and tail for context
  const headSize = Math.floor(limit * 0.7);
  const tailSize = limit - headSize - 100; // 100 chars for truncation notice
  const head = result.slice(0, headSize);
  const tail = result.slice(-tailSize);
  const omitted = result.length - headSize - tailSize;

  return `${head}\n\n[... ${omitted} characters omitted ...]\n\n${tail}`;
}
```

This happens when storing the tool result in the message history, not when returning to the model in the current turn. The model sees the full result for its current decision, but future turns see the truncated version.

#### 2d: Dynamic thinking budget

Adjust thinking budget based on turn complexity signals:

```typescript
function computeThinkingBudget(
  messageContent: string,
  toolCount: number,
  sessionContext: { messageCount: number; lastThinkingTokens?: number },
): number {
  const baseLength = messageContent.length;

  // Short simple messages → minimal thinking
  if (baseLength < 100 && toolCount === 0) return 2000;

  // Medium messages → moderate thinking
  if (baseLength < 500) return 6000;

  // Complex / multi-part → full budget
  if (baseLength > 1000 || toolCount > 5) return 16000;

  return 8000; // Default
}
```

This is a rough heuristic. The better signal is the model's own behavior — if it consistently uses <1K tokens of thinking on simple turns, we could observe and adapt. But a static budget floor prevents waste on trivial turns.

### Phase 3: Parallel Sub-Agent Dispatch

**Goal:** When dispatching multiple independent sub-agent tasks, run them concurrently.

This builds on Spec 13's `sessions_spawn`. Currently, each spawn is a separate tool call that the model makes sequentially. The model CAN request multiple spawns in one response (multiple `tool_use` blocks), and Phase 1's parallel execution would handle this automatically.

But there's also the orchestrator pattern: the model explicitly decomposes a task into sub-tasks and dispatches them. This should be a first-class operation:

```typescript
// New tool: sessions_dispatch (batch spawn)
{
  name: "sessions_dispatch",
  description: "Spawn multiple sub-agents in parallel. Returns when all complete.",
  input: {
    tasks: [{
      role: "coder" | "reviewer" | "researcher" | "explorer" | "runner",
      message: string,
      context?: string,
    }]
  }
}
```

This is syntactic sugar over parallel `sessions_spawn`, but it makes the intent explicit and allows the runtime to optimize (shared context loading, result aggregation, budget distribution).

### Phase 4: Cost Visibility

**Goal:** Make token costs visible per-turn and per-session so waste is noticed.

This overlaps with Spec 04 (Cost-Aware Orchestration) Phase 1. The key addition here is per-tool-call cost attribution:

```typescript
interface ToolCostAttribution {
  toolName: string;
  inputTokens: number;    // Context tokens consumed by this tool's result in subsequent API calls
  directCost: number;     // API cost of the turn that included this tool
}
```

Surface this in the tool panel (Spec 15 Phase 3 adds the UI hooks — this adds the data):

```
⚡ Run command: npm run build    ✓  3.2s   ~2,400 tokens
```

When users see that a single `exec` result consumed 2,400 tokens of context, they understand why truncation matters.

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** ✅ | Parallel tool execution | Medium | High — 2-5x faster tool-heavy turns |
| **2a** | Token audit tooling | Small | ✅ Done — `audit-tokens` CLI with per-file, cache group, cost estimates |
| **2b** | Bootstrap density audit | Small-Medium | ✅ Done — Syn 55%, domain agents ~45% trimmed. Measurement drives ongoing trims |
| **2c** | Tool result truncation | Small | ✅ Done — per-tool char limits, head+tail preservation, both exec paths |
| **2d** | Dynamic thinking budget | Small | Low-Medium — saves tokens on simple turns |
| **3** | Parallel sub-agent dispatch | Medium | Medium — faster complex delegation |
| **4** | Per-tool cost visibility | Small | Low — awareness drives behavior change |

---

## Testing

- **Parallel execution:** Send a message that triggers 4 independent `grep` calls. Verify they execute concurrently (wall clock < sum of individual durations). Verify results are returned in the correct order.
- **Parallel safety:** Send a message that triggers `edit` on the same file twice. Verify they execute sequentially (conflict detection works).
- **Mixed batches:** Send a message with 3 `grep` (parallel), 1 `exec` (sequential), 2 `read` (parallel). Verify batch grouping: [grep, grep, grep], [exec], [read, read].
- **Token audit:** Run `aletheia audit-tokens syn`. Verify output includes per-section token counts and total.
- **Truncation:** Execute a tool that produces 20K chars of output. Verify the stored result in message history is truncated to the configured limit. Verify the model's current-turn result is NOT truncated.
- **Thinking budget:** Send "hi" to an agent with dynamic thinking. Verify thinking tokens < 2000. Send a complex multi-part question. Verify thinking tokens > 6000.
- **Parallel dispatch:** Use `sessions_dispatch` with 3 independent tasks. Verify all 3 sub-agents run concurrently and results are aggregated.

---

## Success Criteria

- Tool-heavy turns (10+ tools) complete 2-5x faster due to parallel execution
- Bootstrap token count is documented and actively managed
- Tool result truncation reduces average context growth by 30%+
- Every turn's token cost is visible in the UI
- No correctness regressions from parallelization (results are correct and ordered)
