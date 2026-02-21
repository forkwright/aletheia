# Spec: Chat Output Quality — Signal Over Noise

**Status:** Complete — All 5 phases implemented. (PR #86)
**Author:** Syn
**Date:** 2026-02-20

---

## Problem

Agent chat output has three categories of waste:

### 1. Internal Narration in Chat

Process narration that belongs in extended thinking leaks into the visible chat:

```
"All critical context is already saved in session notes and memory files."
"Let me save my progress immediately."
"Context is about to clear. Let me finish the manager changes."
```

This is the agent talking to itself, not to the human. It should be in the thinking pane or not said at all.

### 2. Repetitive Status Blocks

The same state gets restated across multiple messages — one session had the same "critical context saved" block **12 times**.

### 3. Poor Formatting

- Walls of prose where tables scan faster
- Missing section headers for long outputs
- Inconsistent formatting between similar outputs
- Code blocks without language hints

---

## Design

### Phase 1: Thinking vs Chat Routing (Prompt) ✅

Update agent operations template with explicit rules for what belongs in thinking vs chat. See `shared/templates/sections/output_quality.md`.

### Phase 2: Formatting Standards (Prompt) ✅

Define formatting conventions: tables for comparisons, headers for long output, structured status reports, bold for key terms, no filler phrases. See `shared/templates/sections/output_quality.md`.

### Phase 3: Narration Suppression — Start of Response (Runtime) ✅

Post-processing filter that reclassifies common narration patterns from `text_delta` to `thinking_delta` events at the start of a response. Safety net for prompt regression.

Not content filtering — reclassification. Information moves to thinking pane, not deleted.

**Current limitation:** The `NarrationFilter` deactivates after the first non-narration sentence. Process narration that follows substantive text passes through uncaught:

```
"Good call on both. Let me check the spec landscape and the message-queue branch state."
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                    This narration passes through because "Good call" deactivated the filter.
```

During tool-heavy turns (28 tool calls), the user sees a wall of process narration streaming in the text pane. After the turn completes, the thinking pane collapses and only the final clean summary remains — but during the work, the experience is noisy.

### Phase 4: Full-Response Narration Suppression (Runtime) ✅

Extend the `NarrationFilter` to classify narration throughout the entire response, not just at the start.

**Design change:** Remove the `active = false` early exit. Every sentence passes through `isNarration()` for the full response. Non-narration sentences emit as `text_delta`, narration sentences emit as `thinking_delta`, regardless of position.

```typescript
// Current behavior (Phase 3):
// "Good call. Let me check the logs. Here are the results."
//  ^^^^^^^^^^                         ^^^^^^^^^^^^^^^^^^^^^^^
//  text_delta (deactivates filter)    text_delta (filter off)
//              ^^^^^^^^^^^^^^^^^^^^^^^
//              text_delta (filter already off — WRONG)

// Phase 4 behavior:
// "Good call. Let me check the logs. Here are the results."
//  ^^^^^^^^^^                         ^^^^^^^^^^^^^^^^^^^^^^^
//  text_delta                         text_delta
//              ^^^^^^^^^^^^^^^^^^^^^^^
//              thinking_delta (caught regardless of position)
```

**Performance:** The sentence-boundary buffering adds ~0.5ms per chunk. `isNarration()` is 6 regex tests per sentence — negligible vs. the LLM latency. No early exit means this cost applies to every sentence, but on a response with 20 sentences that's 10ms total. Acceptable.

**Edge cases:**
- Mixed sentences ("The file has 200 lines, let me check the first 50") — these are substantive enough to pass through. The verb patterns only match when narration IS the entire sentence.
- Short narration ("Checking now.") — already caught by patterns. Min length 10 chars prevents false positives on fragments.
- Long process descriptions (>200 chars) — pass through as text. These are likely substantive explanations, not throwaway narration.

**Additional patterns to add:**
```typescript
// Process narration that currently slips through:
/^(?:Good (?:call|point|idea|question)[.,]?\s+)?(?:Let me|I'll|I need to)/i,
/^(?:Now I (?:have|need|can|should|want))/i,
/^(?:Let me (?:also|now|first|quickly))/i,
/^(?:Time to|Going to|About to)\s+/i,
```

### Phase 5: Rich Message Components (UI)

Status cards, diff views, progress checklists, cost badges. Pattern-match on common output structures and render upgraded components.

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** ✅ | Prompt — thinking vs chat routing | Small | High |
| **2** ✅ | Prompt — formatting standards | Small | Medium |
| **3** ✅ | Runtime narration filter (response start) | Medium | Medium (safety net) |
| **4** ✅ | Runtime narration filter (full response) | Small | High — eliminates mid-response process noise |
| **5** | Rich UI components | Large | High |

---

## Success Criteria

- Zero "context about to clear" or "let me save" in visible chat
- Zero mid-response process narration ("Let me check...", "Now let me...") in visible chat — reclassified to thinking pane
- Status reports use tables/structure, not prose
- No repeated state blocks within a session
- Output skimmable from headers and bold text alone
- During tool-heavy turns, visible text contains only substantive observations and the final summary — process narration streams in the thinking pane
