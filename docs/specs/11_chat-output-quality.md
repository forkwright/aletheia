# Spec: Chat Output Quality — Signal Over Noise

**Status:** Phase 1-2 done
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

### Phase 3: Narration Suppression (Runtime)

Post-processing filter that reclassifies common narration patterns from `text_delta` to `thinking_delta` events. Safety net for prompt regression.

```typescript
const NARRATION_PATTERNS = [
  /^(Let me|I'll|I need to)\s+(check|read|look|save|verify)/i,
  /context is (about to|going to) (clear|be cleared)/i,
  /critical (context|state|information) (is|has been) (already )?saved/i,
];
```

Not content filtering — reclassification. Information moves to thinking pane, not deleted.

### Phase 4: Rich Message Components (UI)

Status cards, diff views, progress checklists, cost badges. Pattern-match on common output structures and render upgraded components.

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** | Prompt — thinking vs chat routing | Small | High |
| **2** | Prompt — formatting standards | Small | Medium |
| **3** | Runtime narration filter | Medium | Medium (safety net) |
| **4** | Rich UI components | Large | High |

---

## Success Criteria

- Zero "context about to clear" or "let me save" in visible chat
- Status reports use tables/structure, not prose
- No repeated state blocks within a session
- Output skimmable from headers and bold text alone
