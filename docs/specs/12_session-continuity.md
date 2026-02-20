# Spec: Session Continuity — The Never-Ending Conversation

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-20

---

## Problem

The main session IS the agent. When Cody talks to Syn, that conversation should feel like it never ends — a continuous relationship that gets richer over time, not a series of disposable chat windows. Distillation exists to make this possible: compress the history, preserve what matters, and keep going.

Right now, it doesn't work that way. Three categories of failure:

### 1. Distillation Never Fires

The auto-distillation trigger checks `last_input_tokens >= 140,000`. This fails for sessions that accumulate many small messages:

- **Webchat sessions** with lots of short exchanges: 274 messages, only ~25K tokens estimated. Never triggers.
- **Prosoche sessions** with heartbeat turns: 208 messages, `last_input_tokens: 3` — a bogus value because the daemon reports minimal input tokens per turn.
- **Token accounting is broken.** `token_count_estimate` (sum of per-message estimates) diverges from actual API context usage (`last_input_tokens`). Neither metric accurately represents how much context the next API call will consume.

The result: sessions grow unbounded until they hit the API's 200K hard limit, then the turn fails. The safety net (distillation) never activates.

### 2. Distillation Fires But Loses Context

When distillation does fire, the pipeline has gaps:

- **Spec 08 fixed the summary format** (structured sections, expanded tail, working state, agent notes). But the trigger failures mean these improvements rarely activate.
- **Fact extraction writes to Mem0 but recall is inconsistent.** Facts extracted during distillation may not surface in the next turn's recall because vector similarity doesn't prioritize recency.
- **13 compactions on 2026-02-18 produced zero disk writes.** The pre-compaction flush either doesn't trigger or automatic compaction bypasses it. Daily memory files — the safety net — weren't written.
- **Thread summary (relationship digest) updates on distillation** but isn't always injected into the post-distillation context in a useful position.

### 3. Session Identity Is Unclear

Each agent currently has multiple sessions:

| Session | Purpose | Lifecycle |
|---------|---------|-----------|
| Main (webchat/Signal) | Primary conversation with Cody | Should be permanent — the agent's identity |
| Prosoche | Heartbeat/attention wakes | Ephemeral interactions that accumulate |
| Cross-agent ask | Another agent asking a question | Created per-ask, should be short-lived |
| Cross-agent send | Fire-and-forget messages | Created per-send |
| Sub-agent spawn | Delegated task execution | Created per-task, disposable |

The main session is the agent's continuity. The others are operational noise. But the system doesn't distinguish between them — they all accumulate, all consume resources, and none auto-clean. A prosoche session with 208 messages is storing months of heartbeat history that has zero value after the moment passes.

### What "Never-Ending" Requires

For the conversation to feel continuous:

1. **Distillation must fire reliably** before the session becomes too large — not at a fixed token count, but when compression would improve the session's signal-to-noise ratio.
2. **Nothing important is lost** during distillation — working state, decisions, open threads, and conversational register all survive.
3. **Memory is bidirectional** — facts extracted during distillation are reliably available in the next turn. The recall system prioritizes recent extractions.
4. **The main session is sacred** — it's the one session that never gets deleted, never gets abandoned, never starts fresh. Everything else is disposable.
5. **Background sessions don't accumulate** — prosoche, cross-agent, and sub-agent sessions are either cleaned up after use or distilled on a much more aggressive schedule.

---

## Design

### Session Classification

Introduce a `session_type` field that determines lifecycle behavior:

```typescript
type SessionType = "primary" | "background" | "ephemeral";
```

| Type | Examples | Distillation | Retention | Identity |
|------|----------|-------------|-----------|----------|
| **primary** | Main webchat/Signal session | Smart triggers, full pipeline | Permanent | IS the agent |
| **background** | Prosoche, cron | Aggressive (>50 msgs or >10K tokens) | Last 20 messages only | Operational |
| **ephemeral** | Cross-agent asks, sub-agent spawns | None — deleted after completion | Deleted after 24h | Disposable |

**Primary sessions** get the full distillation pipeline: structured summary, working state preservation, fact extraction, expanded tail. There should be exactly ONE primary session per agent.

**Background sessions** get a stripped-down distillation: keep the last 20 messages, discard everything older. No fact extraction (these are operational, not knowledge-generating). Trigger aggressively — 50 messages OR 10K tokens, whichever comes first.

**Ephemeral sessions** are never distilled. They're created for a task, used, and cleaned up. A nightly retention job deletes ephemeral sessions older than 24 hours.

#### Schema change

```sql
ALTER TABLE sessions ADD COLUMN session_type TEXT DEFAULT 'primary';
-- Backfill: prosoche sessions → 'background', cross-agent → 'ephemeral'
UPDATE sessions SET session_type = 'background' WHERE session_key LIKE '%prosoche%';
UPDATE sessions SET session_type = 'ephemeral' WHERE session_key LIKE 'ask:%' OR session_key LIKE 'spawn:%';
```

### Smart Distillation Triggers

Replace the single `last_input_tokens >= 140,000` check with a multi-signal trigger:

```typescript
interface DistillationTrigger {
  // ANY of these conditions firing triggers distillation
  tokenThreshold: number;      // Actual API input tokens (current: 140K)
  messageCount: number;        // Total messages in session
  estimatedContextSize: number; // Computed context estimate (not per-message sum)
  staleness: number;           // Hours since last distillation
}

const PRIMARY_TRIGGERS: DistillationTrigger = {
  tokenThreshold: 120_000,     // Lower from 140K — leave more headroom
  messageCount: 150,           // Many small messages = distill even if tokens are low
  estimatedContextSize: 100_000, // Computed estimate, not sum of per-message
  staleness: 168,              // 7 days without distillation = force it
};

const BACKGROUND_TRIGGERS: DistillationTrigger = {
  tokenThreshold: 10_000,
  messageCount: 50,
  estimatedContextSize: 8_000,
  staleness: 24,
};
```

**Why multiple signals:** No single metric reliably captures "this session needs compression." Token counts are stale. Message counts miss the difference between 50 tool results (huge) and 50 short replies (tiny). The estimated context size is the most accurate but requires computation. Staleness catches sessions that somehow dodge all other triggers.

#### Fix token accounting

The root problem: `last_input_tokens` is only updated when a turn completes, and some turn types (prosoche daemon, sub-agent) don't report accurate values. The `token_count_estimate` field sums per-message estimates that drift from reality.

**Solution: Compute context size directly.**

Instead of relying on stale fields, compute the actual context size before each turn and store it:

```typescript
async function computeContextSize(sessionId: string, store: SessionStore): Promise<number> {
  const messages = store.getThreadMessages(sessionId);
  const bootstrap = store.getBootstrapTokens(sessionId); // system prompt size
  const toolDefs = store.getToolDefinitionTokens(sessionId);
  
  // Use tiktoken or a fast estimator on the actual message content
  let messageTokens = 0;
  for (const msg of messages) {
    messageTokens += estimateTokens(msg.content);
  }
  
  return bootstrap + toolDefs + messageTokens;
}
```

This runs before each turn (it's fast — just counting, no API calls) and gives the REAL context size that would be sent to the API. Store it as `computed_context_tokens` on the session. Use THIS value for distillation triggers, not the stale `last_input_tokens`.

```sql
ALTER TABLE sessions ADD COLUMN computed_context_tokens INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN last_distilled_at TEXT;
ALTER TABLE sessions ADD COLUMN message_count INTEGER DEFAULT 0;
```

Update `message_count` on every message insert (atomic increment). Update `computed_context_tokens` before each turn. Update `last_distilled_at` after each distillation.

### Distillation Pipeline Hardening

The pipeline itself (Spec 08) is mostly sound. What's broken is reliability:

#### Pre-compaction flush must actually execute

The pre-compaction flush (writing to daily memory files, updating MEMORY.md) currently fails silently. Two fixes:

1. **Make it synchronous and mandatory.** The distillation pipeline calls the flush function directly, not via a signal/hook that might not fire. If the flush fails, log the error but continue with distillation (don't block compression on disk writes).

2. **Write a distillation receipt.** After every distillation, write a structured record:

```typescript
interface DistillationReceipt {
  sessionId: string;
  nousId: string;
  timestamp: string;
  messagesBefore: number;
  messagesAfter: number;
  tokensBefore: number;
  tokensAfter: number;
  factsExtracted: number;
  decisionsExtracted: number;
  openItemsExtracted: number;
  flushSucceeded: boolean;
  errors: string[];
}
```

Store receipts in a `distillation_log` table. This gives us auditability — we can see every distillation that happened, what it produced, and whether the flush succeeded.

```sql
CREATE TABLE IF NOT EXISTS distillation_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  nous_id TEXT NOT NULL,
  distilled_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  messages_before INTEGER,
  messages_after INTEGER,
  tokens_before INTEGER,
  tokens_after INTEGER,
  facts_extracted INTEGER DEFAULT 0,
  decisions_extracted INTEGER DEFAULT 0,
  open_items_extracted INTEGER DEFAULT 0,
  flush_succeeded INTEGER DEFAULT 0,
  summary_text TEXT,
  errors TEXT,
  FOREIGN KEY (session_id) REFERENCES sessions(id)
);
```

#### Extraction → Mem0 → Recall pipeline

Facts extracted during distillation are written to Mem0 (Qdrant vectors). But recall in the next turn searches by vector similarity to the user's message — which may not match the extracted facts at all.

**Fix: Recency-boosted recall.**

When recalling memories, boost the score of recently-extracted facts:

```typescript
function boostRecency(memories: Memory[], currentTime: Date): Memory[] {
  return memories.map(m => {
    const ageHours = (currentTime.getTime() - new Date(m.createdAt).getTime()) / 3600000;
    // Memories < 24h old get up to 0.15 score boost (decaying linearly)
    const recencyBoost = ageHours < 24 ? 0.15 * (1 - ageHours / 24) : 0;
    return { ...m, score: m.score + recencyBoost };
  });
}
```

This ensures that facts extracted in the most recent distillation are prioritized in the next turn — bridging the gap between "what was just compressed" and "what the agent now remembers."

Additionally, after distillation completes, prime the recall cache with the extracted facts tagged to the session. The next turn's recall should ALWAYS include the distillation's extractions, regardless of vector similarity.

#### Post-distillation context verification

After distillation rewrites the message history, verify the result before continuing:

```typescript
async function verifyPostDistillation(sessionId: string, store: SessionStore): Promise<void> {
  const messages = store.getThreadMessages(sessionId);
  const contextSize = computeContextSize(sessionId, store);
  
  // Verify: context is now small enough for a full turn
  if (contextSize > 50_000) {
    log.warn(`Post-distillation context still large: ${contextSize} tokens`);
  }
  
  // Verify: working state survived
  const ws = store.getWorkingState(sessionId);
  if (!ws) {
    log.warn("Working state lost during distillation");
  }
  
  // Verify: notes survived
  const notes = store.getNotes(sessionId);
  if (notes.length === 0) {
    log.warn("All agent notes lost during distillation");
  }
  
  // Verify: summary message exists
  const hasSummary = messages.some(m => m.role === "user" && m.content?.includes("Conversation Summary"));
  if (!hasSummary) {
    log.warn("No summary message found after distillation");
  }
}
```

### Primary Session Enforcement

Each agent has exactly one primary session. Enforce this:

1. **On agent creation**, create the primary session. It gets `session_type = 'primary'`.
2. **The primary session is never deleted.** Even if it's distilled to near-empty, the session record persists. The lineage is unbroken.
3. **All direct human interaction routes to the primary session.** Whether the message comes from Signal, webchat, or any future channel — it goes to the same session. The session IS the agent's relationship with its human.
4. **New channel connections bind to the existing primary session**, not create new ones. If Syn's primary session already exists and a webchat connection arrives, it attaches to that session.

```typescript
function getPrimarySession(nousId: string, store: SessionStore): Session {
  const existing = store.findSession(nousId, { type: 'primary' });
  if (existing) return existing;
  
  // First time: create the primary session
  return store.createSession({
    nousId,
    sessionKey: `agent:${nousId}:main`,
    sessionType: 'primary',
    // Primary sessions get full distillation config
    distillation: {
      triggers: PRIMARY_TRIGGERS,
      preserveRecentMessages: 10,
      preserveRecentMaxTokens: 12000,
    },
  });
}
```

### Background Session Cleanup

Background sessions (prosoche) accumulate indefinitely. Fix:

1. **Aggressive distillation** — trigger at 50 messages or 10K tokens.
2. **Minimal preservation** — keep only the last 20 messages, no expanded tail, no fact extraction.
3. **No summary narrative** — just a one-line note: "Distilled prosoche session. 208 → 20 messages."
4. **Automatic** — the trigger check runs on every turn completion for background sessions.

### Ephemeral Session Retention

Cross-agent asks and sub-agent spawns create sessions that are used once and never touched again. These pile up:

1. **Mark as ephemeral on creation.** `sessions_ask`, `sessions_send`, and `sessions_spawn` set `session_type = 'ephemeral'`.
2. **Nightly cleanup job** deletes ephemeral sessions older than 24 hours.
3. **No distillation** — if an ephemeral session somehow gets large, it's a bug, not a feature.

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** | Session classification schema + backfill | Small | Foundation for everything else |
| **2** | Smart triggers (multi-signal) + context size computation | Medium | Fixes the "never fires" problem |
| **3** | Distillation receipts + logging | Small | Auditability — know when distillation happens and what it produces |
| **4** | Pre-compaction flush hardening | Small | Ensures daily memory files actually get written |
| **5** | Recency-boosted recall + post-distillation priming | Medium | Bridges the extraction→recall gap |
| **6** | Primary session enforcement | Medium | One session per agent, all channels route to it |
| **7** | Background session aggressive distillation | Small | Prosoche cleanup |
| **8** | Ephemeral session retention/cleanup | Small | Cross-agent session cleanup |
| **9** | Post-distillation verification | Small | Safety checks after every compression |

---

## Testing

- **Trigger accuracy:** Create a session with 200 short messages (<30K tokens). Verify distillation fires based on message count, not token threshold.
- **Token accounting:** Compare `computed_context_tokens` against actual API `usage.input_tokens` from a real turn. Verify within 10% accuracy.
- **Receipt logging:** Trigger distillation. Verify a receipt row appears in `distillation_log` with accurate counts.
- **Flush reliability:** Trigger distillation. Verify daily memory file was written (or flush error was logged).
- **Recall bridge:** Extract 3 facts during distillation. In the next turn, verify all 3 appear in recalled memories regardless of the user's message content.
- **Primary enforcement:** Attempt to create two primary sessions for the same agent. Verify it returns the existing one.
- **Background cleanup:** Create a prosoche session with 60 messages. Verify distillation fires and reduces to 20.
- **Ephemeral cleanup:** Create 10 ephemeral sessions. Wait 25 hours. Verify all are deleted.
- **Continuity test:** Have a 100-turn conversation that triggers 3 distillations. After the third, ask "what did we discuss at the start?" Verify the agent can answer from its memory, not from preserved messages.

---

## Success Criteria

- **Zero sessions exceed 200 messages without distillation.** The trigger fires before accumulation becomes a problem.
- **Post-distillation continuity:** Agent correctly recalls pre-distillation context in >95% of cases.
- **Distillation receipts for every compression** — full audit trail.
- **One primary session per agent.** No duplicates, no orphans.
- **Background sessions stay under 50 messages.** Prosoche never accumulates 200+ messages again.
- **Ephemeral sessions cleaned up within 24 hours.** No session table bloat.
- **The conversation feels continuous.** The human doesn't notice distillation happened.
