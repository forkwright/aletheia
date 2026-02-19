# Spec: Memory Continuity — Surviving Distillation Without Losing the Thread

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

Distillation is necessary but destructive. When context hits ~70% of the 200K window, the pipeline summarizes the conversation, extracts facts, and replaces the raw history with a compressed summary + the last ~4 messages (~4000 tokens). The agent wakes up in a new context with:

1. A summary (good for "what happened" but lossy on nuance)
2. 4 recent messages (maybe 2 turns of conversation)
3. Recalled memories from Qdrant (hit-or-miss relevance)
4. Bootstrap files (SOUL.md, AGENTS.md, etc.)

What's lost:

- **Conversational register** — the tone, rhythm, and specificity of the ongoing dialogue
- **Working context** — which files were being edited, what the current task chain was, what was tried and failed
- **Implicit state** — "we agreed to do X before Y" lives in the flow of conversation, not in extractable facts
- **The last 10 messages before the preserved tail** — these often contain the context for why the tail messages exist. Preserving the last 4 messages without the preceding 6-10 is like reading the last paragraph of a chapter

The result: after distillation, the agent often repeats work, asks questions that were already answered, or loses the thread of a multi-step task. The user notices. Continuity breaks.

### What exists today

| Layer | What it captures | Limitations |
|-------|-----------------|-------------|
| **Distillation summary** | High-level narrative of what happened | Lossy. Misses nuance, working state, implicit agreements |
| **Fact extraction** | Durable facts, decisions, open items | Good for long-term knowledge, bad for "what are we doing right now" |
| **Preserved tail** | Last 4 messages (4000 tokens max) | Too small. Often misses the context for why those messages exist |
| **Thread summary** | Running relationship digest | Updated on distillation. Good for "who is this person" not "what were we doing" |
| **Mem0 recall** | Vector-similar memories from past sessions | Hit-or-miss relevance. Often surfaces old memories instead of recent working context |
| **Workspace files** | MEMORY.md, daily logs | Agent must remember to write to them. Often outdated |

### What the industry does

**Anthropic's approach (Claude Code):**
- Compaction with structured summary (Task Overview, Current State, Important Discoveries, Next Steps, Context to Preserve)
- Tool result clearing via API — old tool results are replaced with placeholders server-side, preserving tool call structure without the payload
- Memory tool — file-based persistent memory that survives compaction
- 5 most recently accessed files restored after compaction

**MemGPT/Letta:**
- Virtual context management inspired by OS memory hierarchy
- Main memory (in-context) + archival memory (out-of-context)
- The LLM itself manages what to move between tiers
- Self-editing memory — the agent can update its own memory entries

**Membox (2026):**
- Topic-continuity-aware memory — groups temporally adjacent messages into "memboxes" by topic
- Sliding-window topic classifier determines when topics shift
- Retrieves entire topic clusters rather than individual messages

**H-MEM (2025):**
- Hierarchical memory with positional index encoding
- Multi-level storage: episodic (raw), semantic (extracted), procedural (how-to)
- Structured retrieval by memory type

**Common pattern across all:** The best systems don't just summarize — they maintain multiple representations of the same information at different levels of abstraction, and they give the agent tools to actively manage its own memory.

---

## Design

### Core Insight

The problem isn't that distillation is too aggressive — it's that we rely on a single representation (the summary) to carry all the context. A single summary can't simultaneously be:
- Compact enough to save context
- Detailed enough for working state
- Structured enough for fact retrieval
- Narrative enough for conversational continuity

**Solution: Multiple memory tiers, each optimized for a different purpose, assembled into context dynamically based on what's needed.**

### Tier 1: Working State (Survives Within a Session)

**What:** A structured scratchpad that captures the current task chain, open files, recent decisions, and next steps. Updated continuously during a session, not just at distillation.

**Implementation:** A `working_state` field on the session/thread, updated after every turn by a lightweight post-turn hook:

```typescript
interface WorkingState {
  currentTask: string;           // "Reviewing PRs #30-32 and merging"
  taskChain: string[];           // ["Review PR #30", "Review PR #31", "Review PR #32", "Merge all"]
  completedSteps: string[];      // ["Reviewed #30 — clean", "Reviewed #31 — clean"]
  openFiles: string[];           // ["docs/specs/spec-turn-safety.md"]
  recentDecisions: string[];     // ["Squash merge, not regular merge"]
  blockers: string[];            // []
  updatedAt: string;
}
```

**How it's maintained:** After each turn, a lightweight extraction (cheaper model, simple prompt) updates the working state. This is NOT a full distillation — it's a ~500 token structured update that takes <1 second.

```
Given the last assistant response and tool calls, update the working state:
- What is the current task?
- What steps have been completed?
- What's next?
- What files are open/relevant?
- Any new decisions or blockers?
```

**How it survives distillation:** The working state is injected into the system prompt after distillation, not into the message history. It's a separate block:

```
## Working State (auto-maintained)
Current task: Reviewing PRs #30-32 and merging
Completed: #30 reviewed (clean), #31 reviewed (clean)
Next: Review #32 diff, then merge all three
Recent decisions: Squash merge, not regular merge
```

**Cost:** ~500 tokens in system prompt + 1 cheap LLM call per turn for maintenance. The per-turn maintenance call can be batched with other post-turn work and runs on Haiku.

### Tier 2: Enhanced Distillation Summary

**What:** Replace the current single-pass summary with a structured, multi-section summary that mirrors Claude Code's compaction format but adapted for multi-session agents.

**New summary format:**

```markdown
# Conversation Summary (Distillation #N)

## Task Context
What the user is working on, what they asked for, what the goal is.

## Completed Work
What was accomplished, with specifics. Not "discussed PRs" but "reviewed and merged PRs #30-32, all clean, squash merged."

## Key Decisions & Rationale
Decisions made and WHY — the rationale matters as much as the decision.

## Current State
Where we left off. What's in progress. What's half-done.

## Open Threads
Things mentioned but not yet addressed. Questions asked but not answered. Tasks deferred.

## Corrections & Failed Approaches
What was tried and didn't work. What was wrong and corrected. Prevents repeating mistakes.

## Tone & Register
Brief note on conversational dynamics — is the user in rapid-fire mode? Deep discussion? Frustrated? This helps the agent match the register after distillation.
```

**How it differs from current:** The current summary is a single narrative blob. The new format is sectioned so the agent can scan for "what's in progress" vs. "what decisions were made" vs. "what failed." Each section serves a different retrieval need.

### Tier 3: Expanded Preserved Tail

**What:** Increase the preserved message window from 4 messages / 4000 tokens to **10 messages / 12000 tokens**.

**Rationale:** 4 messages is approximately 2 turns (user + assistant). That's not enough to maintain conversational flow. 10 messages captures ~5 turns, which typically includes:
- The current exchange
- The context-setting exchange before it
- The transition from the previous topic

**Token budget:** At 12K tokens, this is ~6% of the 200K context window. After distillation, the total context looks like:
- Bootstrap: ~7K tokens
- Working state: ~500 tokens  
- Summary: ~3-5K tokens
- Preserved tail: ~12K tokens
- Recalled memories: ~1.5K tokens
- Tool definitions: ~5K tokens
- **Total: ~30-35K tokens** — plenty of headroom for the next conversation segment

**Configurable:** `preserveRecentMessages: 10` and `preserveRecentMaxTokens: 12000` in compaction config.

### Tier 4: Anthropic Context Editing API

**What:** Use Anthropic's server-side context management API to clear old tool results *without* full distillation. This extends the useful life of the context window significantly.

**How:** The `clear_tool_uses_20250919` strategy automatically replaces old tool results with placeholders when context exceeds a threshold. The tool *call* structure is preserved (so the agent knows what was done) but the verbose result payload is removed.

**Configuration:**

```typescript
context_management: {
  edits: [
    {
      type: "clear_tool_uses_20250919",
      trigger: { type: "input_tokens", value: 120_000 },  // 60% of 200K
      keep: { type: "tool_uses", value: 8 },               // keep last 8 tool results
      clear_at_least: { type: "input_tokens", value: 20_000 },
    },
    {
      type: "clear_thinking_20251015",
      keep: { type: "thinking_turns", value: 2 },
    },
  ],
}
```

**Impact:** Tool results are the biggest context consumers (a single `exec` result can be 2-5K tokens). Clearing old ones at 60% context means distillation doesn't trigger until much later — potentially doubling the useful conversation length before summary is needed.

**Interaction with distillation:** Context editing happens at 60%. Distillation triggers at 70% (current threshold). With context editing active, distillation fires less frequently because old tool results are already cleared. When distillation does fire, the remaining context is cleaner (no verbose tool payloads to summarize).

### Tier 5: Agent-Managed Memory Notes

**What:** Give agents an explicit mechanism to write notes that survive distillation. Not workspace files (which require the agent to remember to write them) — an in-system note-taking tool integrated into the turn flow.

**Implementation:** A `note` tool that writes to a structured notes table:

```typescript
// Tool definition
{
  name: "note",
  description: "Write a note to your persistent memory. Notes survive context distillation and are automatically included in your next session. Use for: important context you don't want to lose, task progress, things to remember.",
  input: {
    content: "string",
    category: "task" | "decision" | "preference" | "correction" | "context",
  }
}
```

Notes are stored per-thread and injected into the system prompt (capped at ~2K tokens, most recent first). The agent is prompted to write notes when:
- A significant decision is made
- The user expresses a preference
- A correction happens
- Work-in-progress state changes

**Difference from workspace files:** Notes are automatic, structured, and injected into context. Workspace files require the agent to read them manually and are not guaranteed to be loaded.

**Difference from Mem0 memories:** Mem0 memories are extracted automatically and retrieved by vector similarity. Notes are explicitly written by the agent and always present in context. They're the agent's "sticky notes" vs. Mem0's "long-term knowledge base."

---

## How It All Fits Together

### Before distillation (normal turn):

```
System prompt:
  - Bootstrap (SOUL.md, AGENTS.md, etc.)        ~7K tokens
  - Working State (auto-maintained)              ~500 tokens
  - Agent Notes (per-thread)                     ~2K tokens
  - Recalled Memories (Qdrant)                   ~1.5K tokens
  - Tool definitions                             ~5K tokens
                                                 ────────
                                                 ~16K tokens

Message history:
  - Full conversation                            grows to ~140K
  - (Tool results auto-cleared at 120K)
                                                 ────────
Total approaching 160K → context editing clears tool results → back to ~120K
Total approaching 140K again → distillation fires
```

### After distillation:

```
System prompt:
  - Bootstrap                                    ~7K tokens
  - Working State (preserved from before)        ~500 tokens
  - Agent Notes (preserved, always present)      ~2K tokens
  - Recalled Memories                            ~1.5K tokens
  - Tool definitions                             ~5K tokens
                                                 ────────
                                                 ~16K tokens

Message history:
  - Structured summary                           ~4K tokens
  - Preserved tail (10 messages)                 ~12K tokens
                                                 ────────
Total: ~32K tokens — 84% of context available for new conversation
```

### Continuity chain:

1. **Working state** tells the agent what's in progress (immediate context)
2. **Preserved tail** gives the recent conversation flow (conversational continuity)
3. **Structured summary** provides the narrative arc (what happened and why)
4. **Agent notes** carry explicit "remember this" signals (high-signal context)
5. **Recalled memories** surface long-term knowledge (cross-session continuity)
6. **Thread summary** provides the relationship context (who is this person, what do we usually discuss)

Each tier answers a different question: "What am I doing?" / "What just happened?" / "What happened before?" / "What's important?" / "What do I know?" / "Who am I talking to?"

---

## Implementation Order

| Phase | Effort | Impact |
|-------|--------|--------|
| **1: Expanded preserved tail** (10 msgs / 12K tokens) | Tiny | Immediate continuity improvement — config change only |
| **2: Structured summary format** | Small | Better post-distillation comprehension |
| **3: Anthropic context editing API** | Medium | Delays distillation, cleaner context |
| **4: Working state maintenance** | Medium | Task continuity across distillations |
| **5: Agent notes tool** | Medium | Explicit memory management by agents |

**Phase 1 is a config change.** Do it immediately. Phases 2-3 are the highest-impact architectural changes. Phases 4-5 add sophistication.

---

## Testing

- **Continuity test:** Run a 50-turn conversation that triggers distillation. After distillation, ask "what were we just working on?" — the answer should be specific and correct.
- **Working state accuracy:** After 10 turns of task work, verify the working state correctly reflects current task, completed steps, and next actions.
- **Context editing:** Fill a session to 120K tokens with tool-heavy work. Verify tool results are cleared, context drops, and the conversation continues without interruption.
- **Notes persistence:** Write 3 notes during a session. Trigger distillation. Verify notes appear in the next turn's system prompt.
- **Summary quality:** Compare old-format summary vs. new structured summary on 5 real distillation traces. Score each on: task recall, decision recall, tone preservation, working state capture.

---

## Success Criteria

- **Post-distillation continuity:** Agent correctly answers "what were we doing?" in >95% of cases (currently ~60-70%)
- **No repeated work:** Agent doesn't re-run investigations or ask questions already answered in the pre-distillation conversation
- **Distillation frequency:** With context editing, distillation fires ~50% less often (extending useful conversation length)
- **User perception:** The user shouldn't notice distillation happened. The conversation should feel continuous.
