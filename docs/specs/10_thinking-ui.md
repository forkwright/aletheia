# Spec: Thinking UI — Reasoning Visibility Without Noise

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

When an agent works through a complex problem, the reasoning and the conclusion arrive as one undifferentiated text stream. The user gets a wall of text where investigative work ("let me check the schema... ok the column is snake_case... now let me look at the pipeline...") is mixed with the actual answer ("The fix is to change two defaults in schema.ts").

This creates two problems:

1. **Noise drowns signal.** The user has to read through the entire working-through to find the answer. For long investigative turns (debugging, code review, multi-step tasks), this can be 80% reasoning and 20% conclusion.

2. **No feedback during thinking.** While the agent is reasoning (before tool calls or final output), the user sees nothing — just a spinner. They don't know if the agent is stuck, going down the wrong path, or making progress. The tool call pills help when tools are running, but pure reasoning is invisible.

### What exists today

- **Extended thinking** is supported end-to-end: Anthropic's `thinking` blocks → runtime `thinking_delta` stream events → UI `thinkingText` state → `<details>` block in messages.
- **But it's not enabled.** Sessions have `thinking_enabled=0` by default.
- **The UI rendering is basic.** A plain `<details>` with "Thought process" / "Thinking..." label and raw text dump. No summarization, no progress indication, no visual design matching the tool pills.

### What it should be

The thinking block should work like the tool status line: a compact pill that shows the *gist* of what's happening in real-time, expandable to full detail. The user feels involved without being buried.

```
🧠 Analyzing distillation pipeline for duplicate tool_result...
```

Click → full reasoning appears in the right panel (same as tool detail panel).

When complete, the final message shows only the conclusion. The thinking is preserved as an expandable section, not inline with the answer.

---

## Design

### Enable extended thinking

Turn on extended thinking for all Opus sessions. This is a config + session default change.

**Runtime:** Set `thinking_enabled=1` as the default for sessions using Opus models. The budget stays at 10K tokens (already the default in schema).

**Why Opus only:** Extended thinking adds latency and cost. Opus is where the complex reasoning happens. Haiku/Sonnet sessions (heartbeats, cron, sub-agents) don't need it.

```typescript
// In session creation or model resolution, when primary model is opus:
thinking_enabled: isOpusModel(model) ? 1 : 0,
thinking_budget: 10000,
```

### Live thinking summary pill

During streaming, replace the raw `<details>` block with a thinking pill that matches the tool status line design:

```
┌──────────────────────────────────────────────────┐
│ 🧠 Checking build-messages.ts for duplicate...   │
│    ↕ click to expand                             │
└──────────────────────────────────────────────────┘
```

**How the summary is generated:** The thinking text streams in as `thinking_delta` events. Every ~500 characters (or every 2 seconds, whichever comes first), extract the last complete sentence or phrase and display it as the pill text. This is pure client-side — no LLM call needed.

```typescript
function extractThinkingSummary(thinkingText: string): string {
  // Take the last 200 chars, find the last complete sentence
  const tail = thinkingText.slice(-200);
  const lastSentence = tail.match(/[.!?]\s+([^.!?]+[.!?])\s*$/);
  if (lastSentence) return lastSentence[1].trim();
  
  // Fallback: last line or phrase
  const lastLine = tail.split('\n').filter(Boolean).pop();
  if (lastLine && lastLine.length > 10) return lastLine.trim().slice(0, 80) + "...";
  
  return "Thinking...";
}
```

The pill updates every time new thinking text arrives, showing the most recent thought. This gives users a sense of progress — "oh, it's looking at the distillation pipeline now" — without dumping the full internal monologue.

### Expandable thinking detail

Clicking the thinking pill opens the same right-side panel used for tool call details. The thinking text renders with markdown formatting, auto-scrolling as new text arrives.

```
┌─ Thinking Panel ──────────────────────────┐
│                                           │
│ Let me check the build-messages.ts file   │
│ to understand how tool_use blocks are     │
│ paired with tool_result blocks...         │
│                                           │
│ The repair loop at the end scans for      │
│ orphaned tool_use blocks, but it only     │
│ checks the immediately-next message.      │
│ If the tool_result was placed elsewhere   │
│ (after distillation boundary), it creates │
│ a synthetic duplicate...                  │
│                                           │
│ ▼ (auto-scrolling)                        │
└───────────────────────────────────────────┘
```

### Completed message: thinking collapsed

When the turn completes, the thinking text is saved to the message (already happens via `state.thinkingText → message.thinking`). In the rendered message:

1. **The conclusion text is primary.** The agent's actual response renders normally.
2. **Thinking is a collapsible pill below the avatar, above the content:**

```
┌─ Completed Message ───────────────────────┐
│ 🌀 Syn                                    │
│                                           │
│ 🧠 Analyzed pipeline, found root cause    │
│    ↕ expand                               │
│                                           │
│ The duplicate tool_result happens because  │
│ the orphan repair loop only checks the    │
│ immediately-next message...               │
│                                           │
│ [code fix details]                        │
└───────────────────────────────────────────┘
```

The thinking summary for the collapsed pill is generated once at completion: take the first and last sentences of the thinking block, combine them into a ~80 char summary. Store this as `thinkingSummary` on the message.

```typescript
function generateThinkingSummary(thinking: string): string {
  const sentences = thinking.match(/[^.!?\n]+[.!?]+/g) ?? [];
  if (sentences.length === 0) return "Thought process";
  if (sentences.length === 1) return sentences[0].trim().slice(0, 80);
  const first = sentences[0].trim();
  const last = sentences[sentences.length - 1].trim();
  return `${first.slice(0, 40)}... → ${last.slice(0, 40)}`;
}
```

### Visual design

The thinking pill should be visually distinct from tool pills but in the same design family:

```css
/* Tool pill: blue accent */
.tool-status-line { border-left: 3px solid var(--accent-blue); }

/* Thinking pill: warm/amber accent */
.thinking-status-line { border-left: 3px solid var(--accent-amber); }
```

Both use the same compact layout: icon + summary text + expand indicator. Both open the same right-side detail panel. The user learns one interaction pattern for "stuff happening behind the scenes."

### Streaming sequence

During a turn, the UI shows events in this order:

```
1. 🧠 Reasoning about the problem...        ← thinking pill (live-updating)
2. 🔧 exec: grep -rn "tool_result"...       ← tool pill appears
3. 🔧 exec: completed (0.3s)                ← tool pill updates
4. 🧠 Found the pattern, now checking...    ← thinking resumes
5. 🔧 read: build-messages.ts               ← another tool call
6. [final response text streams in]          ← thinking pill fades/collapses
```

If thinking and tool calls interleave (which they do with extended thinking), both pills are visible simultaneously. Thinking at top, tool calls below, streaming text at bottom.

---

## Implementation Order

| Phase | Effort | Impact |
|-------|--------|--------|
| **1: Enable extended thinking for Opus** | Small | Runtime config change. Sessions with Opus get thinking enabled. |
| **2: Thinking summary pill** | Medium | Replace raw `<details>` with styled pill matching tool status line. Live-updating summary extraction. |
| **3: Thinking detail panel** | Small | Reuse existing tool detail panel for thinking text. Markdown rendering + auto-scroll. |
| **4: Completed message thinking** | Small | Collapsed pill with summary on completed messages. Expand shows full thinking. |

---

## Testing

- **Thinking enabled:** New Opus session has `thinking_enabled=1`. Haiku/Sonnet sessions have `thinking_enabled=0`.
- **Live pill:** During a thinking-heavy turn, the pill shows a readable summary that updates every few seconds. Not raw stream text.
- **Panel:** Clicking the pill opens the right panel with full thinking text, auto-scrolling during stream.
- **Interleaving:** When thinking and tool calls alternate, both pills are visible. Neither overwrites the other.
- **Completed:** After turn completes, thinking is collapsed with a summary. Clicking expands full text. Conclusion text is not buried.
- **No thinking:** Turns without thinking blocks (Haiku, Sonnet, or short Opus responses) render normally — no empty thinking pill.

---

## Success Criteria

- **Users feel involved.** During long reasoning, the pill shows what the agent is working on — not a blank spinner.
- **Signal over noise.** The conclusion is immediately visible. Reasoning is one click away, not inline.
- **Consistent UX.** Thinking pills and tool pills share the same interaction pattern. Learn one, know both.
