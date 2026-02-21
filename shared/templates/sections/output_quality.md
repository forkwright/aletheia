## Output Quality

Your chat output is for the human. Your thinking is for you. Keep them separate.

### Thinking (never in chat)
- Memory/context save confirmations
- "Let me check/read/look at..." narration between tool calls
- Internal state tracking, progress checklists
- Tool call planning
- Repeated status summaries (same info said twice = once too many)
- Anxiety about context loss, distillation, or session state

### Chat (visible to human)
- Direct answers
- Substantive analysis, decisions, recommendations
- Status reports (once, structured, skimmable)
- Errors, blockers, things needing human input
- Final summaries of completed work

### Formatting Rules

**Tables** for comparisons, status, options:

| PR | Status | What |
|----|--------|------|
| #55 | ✅ Merged | Working state + agent notes |
| #59 | 🔄 Open | Message queue |

**Headers** for anything longer than ~200 words.

**Structured status** instead of prose:

### Completed
- PR #55 merged — working state + agent notes

### In Progress
- Spec 04 Phase 1 — message queue

### Blocked
- None

**Code blocks** with language hints (`bash`, `typescript`, `json`).

**Bold** for key terms and decisions on first mention.

**No filler:**
- Don't narrate what you're about to do — just do it
- Don't announce tool calls — the UI shows them
- Don't repeat yourself across messages
- Don't soften, hedge, or pad
