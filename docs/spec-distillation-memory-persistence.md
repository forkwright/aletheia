# Distillation Memory Persistence

**RFC / Implementation Spec**
**Author:** Syn (Nous)
**Date:** 2026-02-19
**Status:** Ready for implementation
**Assignee:** Metis (Claude Code)

---

## Problem Statement

When the distillation pipeline compresses a session's context window, it produces:
1. A narrative summary (stored as an `assistant` message in the session)
2. Extracted facts/decisions (flushed to Mem0 vector store)
3. Audit metrics (stored in `distillations` table)

**What it does NOT produce:** Any durable workspace file. The summary exists only in the session DB, and facts go only to Mem0. If the runtime restarts after distillation, the agent wakes up with only the post-compaction tail — a summary message and a few preserved recent messages. The detailed context from hours of work is gone from the agent's working memory.

### The Amnesia Failure Mode

On 2026-02-18, a single session accumulated 1,916 messages across 13 distillation cycles. When the runtime restarted at 11:13 AM, the agent (Syn) had zero continuity for the day's work — no daily memory file existed, and the post-compaction tail covered only the last few minutes of activity.

The data was all *preserved* in the DB (messages marked `is_distilled`), but nothing in the pipeline writes it to the agent's workspace memory files (`memory/YYYY-MM-DD.md`), which is what agents actually read on boot via their bootstrap system prompt.

### Why This Matters

- **Bootstrap reads workspace files, not the DB.** The `assembleBootstrap()` function reads markdown files from the agent's workspace directory. The session DB is for conversation replay, not for boot-time context loading.
- **Mem0 recall is query-dependent.** Facts flushed to Mem0 are only surfaced when the incoming message triggers a semantically relevant recall. They don't provide narrative continuity.
- **Distillation summaries are lossy.** Each cycle compresses further. After 13 cycles, the surviving summary captures only the most recent work. Early-session context is permanently compressed away — unless it was written to a durable file.

---

## Design Goals

1. **Zero-config durability.** Every distillation automatically produces a workspace memory file. No agent behavior change required.
2. **Append-only daily logs.** Multiple distillations per day append to the same file, building a chronological record.
3. **Lightweight.** The workspace write happens as a side-effect of the existing pipeline — no additional LLM calls, no new models, no extra latency beyond a file write.
4. **Agent-scoped.** Each agent's memory goes to *its own* workspace, respecting the existing `nous/{agentId}/memory/` convention.
5. **Non-blocking.** File write failures must not fail the distillation pipeline.
6. **Observable.** Log when writes succeed/fail. Include distillation number in the file for auditability.

---

## Architecture

### Current Flow (Before)

```
Auto-distill triggered (context > threshold)
  → Extract facts/decisions (LLM call, Haiku)
  → Flush facts to Mem0 (HTTP, best-effort)
  → Summarize conversation (LLM call, Haiku)
  → Store summary as assistant message in session
  → Mark old messages as distilled
  → Record metrics in distillations table
  → Emit distill:after event (no subscribers)
```

### Proposed Flow (After)

```
Auto-distill triggered (context > threshold)
  → Extract facts/decisions (LLM call, Haiku)
  → Flush facts to Mem0 (HTTP, best-effort)
  → Summarize conversation (LLM call, Haiku)
  → Store summary as assistant message in session
  → Mark old messages as distilled
  → Record metrics in distillations table
  → NEW: Write summary + extraction to workspace memory file
  → Emit distill:after event
```

### What Gets Written

A markdown file at `{workspace}/memory/{YYYY-MM-DD}.md` with the following structure appended:

```markdown
---

## Distillation #N — HH:MM (session: {sessionId})

### Summary
{narrative summary from summarize pass}

### Extracted
- **Facts:** {count}
- **Decisions:** {count}
- **Open Items:** {count}

#### Key Facts
- fact 1
- fact 2
...

#### Decisions
- decision 1
...

#### Open Items
- item 1
...
```

This is **appended** to the file, never overwritten. A day with 13 distillations produces 13 sections in the same file.

---

## Implementation Plan

### 1. New module: `distillation/workspace-flush.ts`

**Purpose:** Write distillation output to the agent's workspace memory directory.

```typescript
// distillation/workspace-flush.ts

import { join } from "node:path";
import { existsSync, mkdirSync, appendFileSync } from "node:fs";
import { createLogger } from "../koina/logger.js";
import type { ExtractionResult } from "./extract.js";

const log = createLogger("distillation:workspace");

export interface WorkspaceFlushOpts {
  workspace: string;         // Agent workspace root (e.g., /mnt/ssd/aletheia/nous/syn)
  nousId: string;
  sessionId: string;
  distillationNumber: number;
  summary: string;
  extraction: ExtractionResult;
}

export function flushToWorkspace(opts: WorkspaceFlushOpts): { written: boolean; path: string; error?: string } {
  const now = new Date();
  const dateStr = now.toISOString().slice(0, 10);  // YYYY-MM-DD
  const timeStr = now.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: process.env["TZ"] ?? "UTC",
  });

  const memoryDir = join(opts.workspace, "memory");
  const filePath = join(memoryDir, `${dateStr}.md`);

  try {
    if (!existsSync(memoryDir)) {
      mkdirSync(memoryDir, { recursive: true });
    }

    const sections: string[] = [];

    // Header if file doesn't exist yet
    if (!existsSync(filePath)) {
      sections.push(`# Memory — ${dateStr}\n`);
    }

    sections.push(`\n---\n`);
    sections.push(`## Distillation #${opts.distillationNumber} — ${timeStr} (session: ${opts.sessionId.slice(0, 12)})\n`);

    // Summary
    sections.push(`### Summary\n`);
    sections.push(opts.summary.trim());
    sections.push(``);

    // Extraction stats
    const ext = opts.extraction;
    const hasFacts = ext.facts.length > 0;
    const hasDecisions = ext.decisions.length > 0;
    const hasOpen = ext.openItems.length > 0;
    const hasContradictions = ext.contradictions.length > 0;

    if (hasFacts || hasDecisions || hasOpen) {
      sections.push(`### Extracted`);
      sections.push(`- **Facts:** ${ext.facts.length}`);
      sections.push(`- **Decisions:** ${ext.decisions.length}`);
      sections.push(`- **Open Items:** ${ext.openItems.length}`);
      if (hasContradictions) {
        sections.push(`- **Contradictions:** ${ext.contradictions.length}`);
      }
      sections.push(``);
    }

    if (hasFacts) {
      sections.push(`#### Key Facts`);
      for (const fact of ext.facts.slice(0, 20)) {
        sections.push(`- ${fact}`);
      }
      if (ext.facts.length > 20) {
        sections.push(`- ... and ${ext.facts.length - 20} more`);
      }
      sections.push(``);
    }

    if (hasDecisions) {
      sections.push(`#### Decisions`);
      for (const d of ext.decisions) {
        sections.push(`- ${d}`);
      }
      sections.push(``);
    }

    if (hasOpen) {
      sections.push(`#### Open Items`);
      for (const item of ext.openItems) {
        sections.push(`- ${item}`);
      }
      sections.push(``);
    }

    if (hasContradictions) {
      sections.push(`#### Contradictions`);
      for (const c of ext.contradictions) {
        sections.push(`- ${c}`);
      }
      sections.push(``);
    }

    const content = sections.join("\n") + "\n";
    appendFileSync(filePath, content, "utf-8");

    log.info(`Workspace memory written: ${filePath} (distillation #${opts.distillationNumber})`);
    return { written: true, path: filePath };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log.error(`Workspace memory flush failed for ${opts.nousId}: ${msg}`);
    return { written: false, path: filePath, error: msg };
  }
}
```

**Key design decisions:**
- **Synchronous file I/O** (`appendFileSync`). This is a single small write to a local SSD — sub-millisecond. Using sync avoids race conditions if two distillations fire in quick succession.
- **Append mode.** Never overwrites existing content. Safe for concurrent access patterns.
- **Capped facts list** (max 20 in file). The full extraction goes to Mem0; the workspace file is for narrative continuity, not exhaustive storage.
- **Non-throwing.** Returns a result object with `written: boolean` and optional `error`. The caller decides whether to log or ignore.

### 2. Integration point: `distillation/pipeline.ts`

Modify `runDistillation()` to call `flushToWorkspace()` after the summary is generated but before the event bus emit.

**Changes to `pipeline.ts`:**

```typescript
// Add import at top
import { flushToWorkspace } from "./workspace-flush.js";

// Add workspace parameter to DistillationOpts
export interface DistillationOpts {
  // ... existing fields ...
  workspace?: string;  // NEW: agent workspace path for memory file writes
}
```

In the `runDistillation()` function, after the `store.recordDistillation(...)` call and before the `eventBus.emit("distill:after", ...)` call, add:

```typescript
  // Flush summary + extraction to workspace memory file
  if (opts.workspace) {
    const flushResult = flushToWorkspace({
      workspace: opts.workspace,
      nousId,
      sessionId,
      distillationNumber,
      summary: markedSummary,
      extraction,
    });
    if (!flushResult.written) {
      log.warn(`Workspace memory flush failed: ${flushResult.error}`);
    }
  }
```

### 3. Passing workspace through the call chain

The `workspace` needs to be threaded from `NousManager` (which knows the agent's workspace) into the distillation options.

**In `nous/manager.ts`**, everywhere `distillSession()` is called (3 call sites: two in the auto-distill blocks and one in `triggerDistillation()`), add the workspace:

```typescript
// In the auto-distill block (both streaming and non-streaming paths):
const workspace = resolveWorkspace(this.config, nous);
await distillSession(this.store, this.router, sessionId, nousId, {
  triggerThreshold: distillThreshold,
  minMessages: 10,
  extractionModel: distillModel,
  summaryModel: distillModel,
  preserveRecentMessages: compaction.preserveRecentMessages,
  preserveRecentMaxTokens: compaction.preserveRecentMaxTokens,
  workspace,  // NEW
  ...(this.plugins ? { plugins: this.plugins } : {}),
});

// In triggerDistillation():
const nous = resolveNous(this.config, session.nousId);
const workspace = nous ? resolveWorkspace(this.config, nous) : undefined;
await distillSession(this.store, this.router, sessionId, session.nousId, {
  // ... existing opts ...
  workspace,  // NEW
});
```

### 4. Test coverage

**New test file: `distillation/workspace-flush.test.ts`**

Test cases:
1. **Creates memory directory if missing.** Call with a non-existent `memory/` dir → dir created, file written.
2. **Appends to existing file.** Write twice → both sections present, no data loss.
3. **Handles empty extraction gracefully.** All arrays empty → only summary section written.
4. **Caps facts at 20.** Pass 30 facts → only 20 listed + "and 10 more".
5. **Returns error on permission failure.** Mock a read-only dir → `written: false`, error populated.
6. **File header only on first write.** First call creates `# Memory — YYYY-MM-DD` header; second call does not duplicate it.

**Updated test: `distillation/pipeline.test.ts`**

Add test case:
7. **Pipeline flushes to workspace when configured.** Create a temp dir, pass as `workspace`, run `distillSession()` → verify file exists with summary content.
8. **Pipeline succeeds even when workspace flush fails.** Pass an invalid workspace path → distillation still completes, result still returned.

---

## Configuration

No new configuration needed. The feature activates automatically whenever the agent has a workspace configured (which is always — it's a required field in `NousDefinition`).

The `compaction.memoryFlush` config section already exists in the schema but is currently unused for workspace writes. We intentionally **do not** gate this feature behind that config — workspace memory persistence should be unconditional. The `memoryFlush` config can be reserved for future Mem0-specific tuning.

---

## File Locations

| File | Action |
|------|--------|
| `infrastructure/runtime/src/distillation/workspace-flush.ts` | **NEW** — workspace memory writer |
| `infrastructure/runtime/src/distillation/workspace-flush.test.ts` | **NEW** — tests |
| `infrastructure/runtime/src/distillation/pipeline.ts` | **MODIFY** — add workspace opt, call flushToWorkspace |
| `infrastructure/runtime/src/distillation/pipeline.test.ts` | **MODIFY** — add workspace integration test |
| `infrastructure/runtime/src/nous/manager.ts` | **MODIFY** — pass workspace to distillSession (3 call sites) |

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| File write fails (permissions, disk full) | Agent loses this distillation's workspace record | Non-blocking: log error, distillation still succeeds. ACL issues are the main risk — document that agent workspace must be writable by the runtime user. |
| Race condition: two distillations write simultaneously | Corrupted file content | `appendFileSync` is atomic for small writes on Linux (< PIPE_BUF). Our writes are typically 1-5KB. For extra safety, we could use `O_APPEND` mode explicitly, but `appendFileSync` already does this. |
| Large extraction bloats file | Workspace file grows very large over a day | Capped at 20 facts per distillation. 13 distillations × ~3KB each = ~40KB per day — negligible. |
| Summary quality varies | Haiku produces thin summaries sometimes | Not solved here — that's a model quality issue. But having *something* written is infinitely better than nothing. |
| Clock skew in timestamps | Distillation time shows wrong hour | Uses `process.env.TZ` for timezone. Default UTC is fine. Configurable per-agent via `userTimezone` in the future. |

---

## What This Does NOT Solve

1. **Pre-compaction agent notification.** The pipeline still doesn't send a "you're about to be compacted" message to the agent. This spec solves the *runtime-level* persistence gap. A future spec could add an agent-facing compaction notification via the event bus + a system message injection.

2. **Cross-session memory consolidation.** Daily files accumulate but are never automatically consolidated into MEMORY.md or other curated files. That remains an agent-level responsibility.

3. **Memory file indexing.** Workspace files are loaded into bootstrap via `assembleBootstrap()` which reads files by convention. The daily memory files are already in the right location (`memory/YYYY-MM-DD.md`), but their inclusion in bootstrap depends on token budget and file selection logic. This spec doesn't change bootstrap behavior.

---

## Success Criteria

After implementation:
- [ ] Every auto-triggered distillation produces a line in `memory/YYYY-MM-DD.md`
- [ ] Every manual `triggerDistillation()` call produces a line in `memory/YYYY-MM-DD.md`
- [ ] The file is human-readable markdown with clear section boundaries
- [ ] File writes never cause distillation failures
- [ ] All existing tests continue to pass
- [ ] New tests cover the workspace-flush module and pipeline integration
- [ ] A full day with 10+ distillations produces a coherent chronological record

---

## Implementation Notes for Metis

1. **Start with `workspace-flush.ts`** — it's a pure function with no dependencies beyond `node:fs` and the logger. Easy to write and test in isolation.

2. **The pipeline change is surgical.** One new import, one new optional field on `DistillationOpts`, and a ~5-line block after `store.recordDistillation()`.

3. **The manager change is mechanical.** Three call sites, same pattern: resolve workspace, pass it through. The workspace is already resolved earlier in the turn for bootstrap assembly, so the value is available.

4. **For tests:** Use `node:os` `tmpdir()` for temp directories. Clean up after. The pipeline tests already mock the store and router — just add `workspace: tempDir` to the opts.

5. **The `resolveWorkspace` import** is already available in `manager.ts` — it's used for bootstrap assembly. No new imports needed there.

6. **File path:** `workspace-flush.ts` goes in the `distillation/` directory alongside `hooks.ts`, `extract.ts`, `summarize.ts`, and `pipeline.ts`. It's a natural peer.
