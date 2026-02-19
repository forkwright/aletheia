# Spec: Unified Thread Model

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

Aletheia's continuity model promises one persistent conversation between a human and each nous — no sessions to manage, no start/stop, no "new chat." The relationship just *is*.

The current implementation breaks this promise in two ways:

1. **Transport collision.** Signal and webchat share a session key (`signal:<hash>`), meaning they share a session lock, in-flight tool state, and message sequencing. A webchat tool loop blocks Signal responses. Orphaned tool_use blocks from one transport corrupt the other's history.

2. **Visible seams.** Distillation, session archival, and context limits are implementation details that leak into the experience. When context compacts, the nous loses thread. When a session archives, continuity breaks. The user shouldn't know or care about any of this.

---

## Design Principles

### The relationship is the unit, not the session

A human talks to a nous. That's the thread. It doesn't start or stop. It doesn't have a session ID the user ever sees. It's more like a friendship than a chat window — you don't "open" a conversation with a friend, you just talk.

### Transports are windows, not threads

Signal, webchat, API, cron — these are different ways the same conversation happens. Like texting someone vs. talking in person. The relationship is one; the channels are many. Each channel has its own in-flight execution state, but they all contribute to the same shared history.

### Infrastructure is invisible

Distillation, token budgets, session rotation — the user never sees these. The nous handles them internally. From the outside, the thread has perfect memory (via distilled summaries + recall) and infinite length.

---

## Architecture

### Thread

The **thread** is the primary abstraction. One thread per `(human, nousId)` pair.

```
Thread {
  id:        string           // stable, e.g. "thread:cody:syn"
  nousId:    string           // which nous
  identity:  string           // which human (derived from contact/auth)
  createdAt: string           // when the relationship started
}
```

A thread is **never archived or deleted** from the user's perspective. It may have internal segments (sessions) for memory management, but those are invisible.

### Segment

A **segment** is an internal session within a thread. Segments handle context window management — when a segment fills up and distills, a new segment continues the thread. The transition is seamless.

```
Segment {
  id:          string         // session ID (internal)
  threadId:    string         // parent thread
  ordinal:     number         // sequence within thread
  status:      'active' | 'distilled' | 'archived'
  summary:     string | null  // distillation summary (carried forward)
}
```

Only one segment per thread is `active` at a time. When distillation occurs:
1. Current segment gets summarized and marked `distilled`
2. New segment created with `ordinal + 1`
3. Distillation summary injected as preamble into new segment
4. From the user's perspective: nothing happened

### Transport Binding

Each transport channel gets its own **binding** to a thread. Bindings handle concurrency isolation.

```
TransportBinding {
  threadId:    string         // which thread
  transport:   string         // 'signal' | 'webchat' | 'api' | 'cron'
  channelKey:  string         // transport-specific identifier
  lockKey:     string         // concurrency lock (per-binding, not per-thread)
}
```

Key property: **bindings share the thread's history but have independent locks and in-flight state.**

### Turn Execution

When a message arrives on any transport:

```
1. Resolve thread     → (human, nousId) → Thread
2. Resolve binding    → (thread, transport, channelKey) → TransportBinding
3. Acquire lock       → binding.lockKey (NOT thread-level)
4. Read history       → thread's active segment, excluding other bindings' in-flight turns
5. Execute turn       → normal pipeline (context → history → execute → finalize)
6. Commit turn        → write completed messages to segment (visible to all bindings)
7. Release lock
```

Step 4 is critical: **history only includes committed turns.** If webchat has an in-flight tool loop, Signal doesn't see it. Each transport sees a clean, consistent history of completed exchanges.

### History Visibility Rules

| State | Visible to originating transport? | Visible to other transports? |
|-------|-----------------------------------|------------------------------|
| Committed turn (complete) | Yes | Yes |
| In-flight turn (tool calls executing) | Yes (own context) | No |
| Orphaned turn (transport disconnected mid-tool) | Repaired on next own turn | No (never leaks) |
| Distillation summary | Yes (as preamble) | Yes (as preamble) |

This eliminates the orphaned tool_use corruption problem entirely. A webchat tool loop is invisible to Signal because those messages haven't been committed yet.

---

## Thread Resolution

### From Signal
```
signal message from +1512... → 
  identity = resolve_contact(+1512...) → "cody"
  nousId = resolve_binding(signal, group_or_dm) → "main" (Syn)
  thread = find_or_create("cody", "main")
```

### From Webchat
```
webchat message to agent "syn" →
  identity = resolve_auth(session_cookie) → "cody"  
  nousId = "syn" (explicit in UI)
  thread = find_or_create("cody", "syn")
```

### From Cross-Agent
```
sessions_send from eiron to syn →
  identity = "eiron" (agent-to-agent, no human)
  nousId = "syn"
  thread = find_or_create("eiron", "syn")  // agent threads are separate
```

---

## Migration

Current state: sessions keyed by `signal:<hash>` or `signal:<group-uuid>`.

Migration path:
1. Create `threads` table mapping `(identity, nousId)` → thread
2. Create `transport_bindings` table mapping `(threadId, transport, channelKey)` → binding
3. Existing sessions become segments within their respective threads
4. Session keys are decomposed: `signal:LAP385oF/...` → thread `cody:main`, binding `signal:LAP385oF/...`
5. Webchat gets its own binding: `webchat:cody` → same thread `cody:main`, different lock

### Backward Compatibility

The existing session key system continues to work internally. The thread/binding layer sits *above* it:
- `SessionStore.findOrCreateSession(nousId, sessionKey)` still works
- Session keys are now derived from `binding.channelKey` rather than passed directly
- Thread resolution happens before session resolution

---

## Distillation & Continuity

When a segment distills:

1. **Extract** facts, decisions, open items from the segment
2. **Summarize** the segment into a narrative summary
3. **Persist** extractions to long-term memory (Mem0) and workspace files
4. **Create** new segment with the summary as preamble
5. **Archive** old segment (queryable but not loaded into context)

The user experiences this as: nothing. The nous just keeps talking. If the user references something from 3 distillations ago, the recall system (Mem0 + workspace memory) fills the gap.

### Thread-Level Memory

Each thread accumulates a **thread summary** — a running digest updated after each distillation. This is the nous's persistent understanding of this specific relationship:

```
ThreadSummary {
  threadId:     string
  summary:      string      // "Cody and I have been working on..."
  keyFacts:     string[]    // extracted facts relevant to this thread
  lastUpdated:  string
}
```

This thread summary is always included in the bootstrap, giving the nous relationship context even after multiple distillations.

---

## UI Implications

### Webchat
- No session list. One thread per nous, always visible.
- Scroll up loads older messages (from current + distilled segments).
- Distillation boundaries are invisible (or at most a subtle visual divider).
- `/new` command creates a **topic** within the thread, not a new session.

### Signal
- No change in UX. Messages just work. Continuity across days/weeks.
- The thread survives device changes, number changes (contact-based identity).

### Cross-Transport Continuity
- User sends "look at this" via Signal with an image, then switches to webchat.
- Webchat sees the image message in history (committed turn).
- Nous can reference it seamlessly.

---

## Concurrency Model

```
Thread: cody <-> syn
|-- Segment #7 (active, ~30K tokens)
|   |-- [committed] turn 1: Signal "hey" -> "hey, what's up"
|   |-- [committed] turn 2: Webchat "review PR #26" -> [tools...] -> "merged"
|   |-- [in-flight] Webchat: executing tool calls (invisible to Signal)
|   +-- [committed] turn 3: Signal "you there?" -> "yeah, I'm here"
|
|-- Segment #6 (distilled)
|   +-- summary: "Merged PRs #4-#14, #24, #26. Launched public repo..."
|
+-- Transport Bindings
    |-- signal:LAP385oF/... -> lock: signal-cody-syn
    +-- webchat:cody        -> lock: webchat-cody-syn
```

Turn 3 can execute immediately because Signal has its own lock. It sees turns 1 and 2 (committed) but not the in-flight webchat turn. When the webchat turn completes and commits, the *next* Signal turn will see it.

---

## Open Questions

1. **Topic branching.** Should `/new` in webchat create a sub-thread (parallel context) or just a soft boundary within the same thread? Soft boundary is simpler; sub-thread enables focused work.

2. **Multi-human threads.** Group chats involve multiple humans + one nous. The thread model extends naturally: `thread:(group_id, nousId)`, but identity resolution gets more complex.

3. **Segment size tuning.** Current distillation triggers at ~75% context fill. With the unified model, should this be more aggressive (smaller segments, more frequent distillation) to keep history lean across transports?

4. **Conflict resolution.** If Signal and webchat both send a message in the same second, they execute independently (different locks). But the committed history order matters. Use wall-clock ordering? Sequence number from segment?

5. **Thread summary quality.** The running thread summary is critical for continuity across distillations. How do we ensure it doesn't drift or lose important context over time? Human-in-the-loop corrections?

---

## Implementation Phases

### Phase 1: Transport Isolation (fixes current bug)
- Derive separate session keys for Signal vs. webchat
- Signal: `signal:<contact_hash>:<nousId>`  
- Webchat: `web:<identity>:<nousId>`
- Both still point to the same logical conversation, but separate locks
- History sharing via cross-session message forwarding (lightweight)
- **This unblocks Signal immediately without full thread model**

### Phase 2: Thread Abstraction
- New `threads` and `transport_bindings` tables
- Thread resolution layer above session resolution
- Segment lifecycle management
- Migration of existing sessions into threads

### Phase 3: Seamless Continuity
- Thread summaries (running relationship digest)
- Cross-segment history loading (scroll-back across distillation boundaries)
- Distillation boundary hiding in UI
- Recall integration (thread context in memory search)

### Phase 4: Advanced
- Topic branching within threads
- Multi-human thread support (groups)
- Thread-level analytics (relationship health, topic tracking)
