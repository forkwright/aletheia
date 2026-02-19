# Spec: Cost-Aware Orchestration & Interactive Turn Model

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-19

---

## Problem

Aletheia is hitting Anthropic's 20x Max usage cap weekly, with $300+ in overage. The root cause is structural: every turn — whether it's reading a file, running a grep, or writing a spec — runs on Opus at $5/MTok input, $25/MTok output. A 15-tool-call investigation that a Haiku agent could handle at $1/$5 instead costs 5-25x more on Opus.

Simultaneously, the interaction model is broken for sustained work sessions. When the nous is mid-tool-loop, the human cannot:
- Send course corrections without breaking the turn
- Queue follow-up messages
- Ask questions or get asked questions interactively
- See what the agent is planning before it executes

These are separate problems with a shared solution: a turn model that supports delegation, concurrency, and human-in-the-loop interaction.

---

## Cost Analysis

### Current Pricing (per million tokens)

| Model | Input | Cache Read | Output | Relative Cost |
|-------|-------|------------|--------|---------------|
| Opus 4.6 | $5.00 | $0.50 | $25.00 | 1x |
| Sonnet 4.6 | $3.00 | $0.30 | $15.00 | 0.6x |
| Haiku 4.5 | $1.00 | $0.10 | $5.00 | 0.2x |

### Where Tokens Go

A typical Syn session:
- Bootstrap (system prompt): ~6,500 tokens (cached after first turn)
- Tool definitions: ~8,000 tokens (cached)
- History: 10,000-50,000 tokens (grows per turn)
- Per-turn output: 500-2,000 tokens (text + tool calls)

A 20-tool-call investigation:
- ~15,000 cached input + ~35,000 fresh input + ~10,000 output per turn
- At Opus: ~$0.075 input + $0.25 output = ~$0.33 per turn
- At Haiku: ~$0.015 input + $0.05 output = ~$0.065 per turn
- **5x savings** on mechanical work

### What Should Run on Opus vs. Not

| Task Type | Model | Why |
|-----------|-------|-----|
| Direct conversation with Cody | Opus | Judgment, nuance, relationship |
| Architecture decisions | Opus | Synthesis, tradeoff reasoning |
| Spec writing | Opus | Quality of thought |
| Code review (critical) | Opus | Catches subtle bugs |
| File reading/searching | Haiku | Mechanical — read and summarize |
| Git operations | Haiku | Procedural — commit, push, branch |
| Test running | Haiku | Execute and report results |
| Bulk file edits | Sonnet | Needs accuracy but not judgment |
| Research/web search | Sonnet | Needs synthesis but not Opus-level |
| PR creation/merge | Haiku | Templated workflow |
| Status checks/health | Haiku | Read and format |

**Conservative estimate:** 40-60% of current Opus tokens could run on Haiku/Sonnet, saving $150-250/week.

---

## Design

### 1. Sub-Agent Architecture

The nous (Opus) acts as the orchestrator. It decomposes work into tasks and delegates to sub-agents that run on cheaper models with isolated context windows.

#### Sub-Agent Types

**Explore** — Read-only investigation
- Model: Haiku
- Tools: read, grep, find, ls, exec (read-only commands)
- No write/edit access
- Use: "Read these 5 files and summarize what you find"
- Returns: Structured summary to orchestrator

**Worker** — Execution with guidance
- Model: Sonnet
- Tools: All file tools + exec
- Write/edit access within workspace
- Use: "Create this PR with these changes", "Run tests and report"
- Returns: Outcome + artifacts to orchestrator

**Researcher** — Web + analysis
- Model: Sonnet
- Tools: web_search, web_fetch, read
- No write access
- Use: "Research how X works and summarize approaches"
- Returns: Structured research findings

**Planner** — Read-only reasoning
- Model: Sonnet (needs synthesis quality)
- Tools: read, grep, find, ls
- No write access
- Use: "Analyze this codebase and propose an approach"
- Returns: Plan for orchestrator to review/approve

#### Spawning Model

```
Orchestrator (Opus)
  │
  ├── spawn(explore, "Read the auth module and summarize the middleware flow")
  │     → Haiku, read-only, returns summary
  │
  ├── spawn(worker, "Create PR #30 with these changes: [details]")
  │     → Sonnet, full tools, returns outcome
  │
  └── spawn(researcher, "Find best practices for JWT refresh rotation")
        → Sonnet, web tools, returns findings
```

Each sub-agent:
- Gets its own context window (no history pollution)
- Receives a focused task description from the orchestrator
- Has tool access scoped to its role
- Returns a structured result that the orchestrator incorporates
- Cannot spawn further sub-agents (no recursion)
- Runs with a token budget cap (configurable per type)

#### Key Differences from Claude Code Sub-Agents

Claude Code's sub-agents are file-based definitions with YAML frontmatter. Ours are:
- **Runtime-configured** — defined in aletheia.json, not markdown files
- **Cost-aware** — model selection is the primary design axis
- **Result-oriented** — sub-agents return structured results, not stream to UI
- **Budget-capped** — each spawn has a max token spend
- **Orchestrator-mediated** — the nous decides what to delegate; the human sees the nous's work, not raw sub-agent output

#### Configuration

```json
{
  "agents": {
    "defaults": {
      "subagents": {
        "types": {
          "explore": {
            "model": "claude-haiku-4-5-20251001",
            "tools": ["read", "grep", "find", "ls", "exec"],
            "toolRestrictions": { "exec": "readOnly" },
            "maxOutputTokens": 4096,
            "maxTurns": 10,
            "budgetTokens": 50000
          },
          "worker": {
            "model": "claude-sonnet-4-6",
            "tools": "inherit",
            "maxOutputTokens": 8192,
            "maxTurns": 30,
            "budgetTokens": 100000
          },
          "researcher": {
            "model": "claude-sonnet-4-6",
            "tools": ["web_search", "web_fetch", "read"],
            "maxOutputTokens": 8192,
            "maxTurns": 15,
            "budgetTokens": 80000
          },
          "planner": {
            "model": "claude-sonnet-4-6",
            "tools": ["read", "grep", "find", "ls"],
            "maxOutputTokens": 8192,
            "maxTurns": 10,
            "budgetTokens": 60000
          }
        },
        "maxConcurrent": 3,
        "defaultBudgetTokens": 50000
      }
    }
  }
}
```

### 2. Interactive Turn Model

The current model is synchronous: human sends message → agent processes entire turn (possibly 20+ tool calls) → agent responds. During processing, the human is locked out.

#### Message Queue

Human messages sent during an active turn are **queued**, not dropped or errored.

```
Human: "Review PR #26 and merge if good"
  [Agent starts: reading diff, checking tests, building...]
Human: "Actually skip the tests, CI will handle those"
  [Queued — agent sees this after current tool call completes]
Human: "Also check if there are other open PRs"
  [Queued]
```

After each tool call result, the agent checks the queue. If messages are waiting:
- They're injected into the conversation as a **mid-turn user message**
- The agent can adjust course, acknowledge, or incorporate the new information
- The turn continues (not restarted)

This is not interruption (which would abort the turn). It's **course correction** — the human steers while the agent works.

#### Implementation

```
Turn Lifecycle:
  1. Human message arrives → start turn
  2. Agent produces response (text or tool_use)
  3. If tool_use:
     a. Execute tool
     b. Check message queue
     c. If queued messages: inject as user content before tool_result
     d. Continue turn with tool_result + any queued messages
  4. If end_turn: deliver response, check queue for next turn
```

The message queue lives on the session, not the transport. Both Signal and webchat can queue messages into the same turn.

#### UI Indicators

- **Queued message badge**: Shows "Message queued — will be seen after current tool completes"
- **Agent activity**: Compact status line already exists (ToolStatusLine). Add "📨 2 queued" indicator
- **Delivered confirmation**: When queued message is consumed, show "✓ Seen by agent"

### 3. Plan Mode (Interactive Approval)

For expensive or risky operations, the agent can present a plan and wait for approval before executing.

#### When to Use Plan Mode

- Agent is about to spawn multiple sub-agents (cost implication)
- Agent is about to make destructive changes (delete, overwrite)
- Agent is uncertain about approach and wants human input
- Human explicitly requested plan mode ("plan this out first")

#### How It Works

```
Agent: "Here's my plan for this PR:

1. 🔍 Explore: Read the auth module (Haiku, ~5K tokens, ~$0.03)
2. 🔍 Explore: Read the test suite (Haiku, ~5K tokens, ~$0.03)
3. ✏️ Worker: Apply the 3 changes listed below (Sonnet, ~20K tokens, ~$0.35)
4. 🧪 Worker: Run affected tests (Sonnet, ~10K tokens, ~$0.15)
5. 📦 Worker: Create and push PR (Haiku, ~5K tokens, ~$0.03)

Estimated cost: ~$0.59 | Estimated time: 2-3 min

Proceed? [Yes / Edit / Skip steps]"
```

The human can:
- **Approve**: Agent executes the plan
- **Edit**: "Skip step 2, I know the tests pass"
- **Reject**: "Actually, different approach..."
- **Partial approve**: "Do steps 1-2 first, then let's decide"

#### UI Implementation

**Webchat**: Plan appears as a structured card with action buttons (Approve / Edit / Reject). Not a modal — inline in the conversation, so the human can scroll back to see context.

**Signal**: Plan appears as formatted text. Human replies "yes", "skip 2", "no", etc. Agent parses intent.

### 4. Complexity-Based Model Routing (Automatic)

Beyond explicit sub-agent spawning, the orchestrator's own turns can route to cheaper models when the task is simple.

We already have `scoreComplexity` in `hermeneus/complexity.ts`. Extend it:

| Complexity | Model | When |
|-----------|-------|------|
| Routine | Haiku | Simple questions, status checks, file reads |
| Standard | Sonnet | Multi-step tasks, code changes, analysis |
| Complex | Opus | Architecture, judgment, relationship, nuance |

The routing happens per-turn, not per-session. A conversation that starts with an architecture discussion (Opus) can route a follow-up "what time is it" to Haiku.

**Important constraint**: Routing only affects the model, not the identity. The nous is always Syn. Haiku-routed turns still get the full bootstrap and respond as Syn. The human should never notice the model switch.

#### Routing Override

The human can force a model:
- "think hard about this" → Opus regardless of complexity score
- "quick question" → Haiku regardless

The agent can also self-escalate: if a Haiku-routed turn realizes it needs deeper reasoning, it requests re-routing to a higher tier.

### 5. Prompt Caching Optimization

We already use Anthropic's prompt caching (cache breakpoints on system prompt and tools). Current cache hit rates from logs show ~347,000% (the metric is bugged but caching is working). Optimize further:

- **Bootstrap as static cache block**: SOUL.md, AGENTS.md, IDENTITY.md, TOOLS.md rarely change. Pin them as the first cache breakpoint (5-minute TTL by default, consider 1-hour for $2x write but $0.10 reads)
- **Tool definitions as second cache block**: Tool schemas are stable within a session
- **Sub-agent bootstrap caching**: Sub-agents that share the same type can reuse cached system prompts across spawns
- **History-aware cache placement**: Place the third breakpoint at the distillation boundary (where history becomes stable)

At Opus rates, cache reads are $0.50/MTok vs $5.00/MTok base — **90% savings on cached content**. Maximizing cache hits on the ~15K token bootstrap is worth ~$0.07/turn saved.

### 6. Token Budget Tracking

Surface cost awareness to both the agent and the human.

#### Agent-Visible

Each turn's trace already tracks input/output tokens. Add:
- Estimated cost per turn (using model pricing)
- Running session cost
- Sub-agent cost breakdown
- Budget remaining (if configured)

Inject into working state (every 8th turn):
```
## Cost — Session $2.34 (12 turns)
Last turn: $0.18 (Opus) | Sub-agents: $0.42 (3 Haiku, 1 Sonnet)
Budget: $10.00 remaining today
```

#### Human-Visible (UI)

- Per-message cost indicator (subtle, hover-to-see)
- Session cost in status bar
- Daily/weekly usage chart in settings view
- Sub-agent cost breakdown in tool panel

---

## Migration Path

### Phase 1: Message Queue (unblocks interaction)
- Implement message queue on session
- Check queue after each tool call
- Inject queued messages as mid-turn user content
- UI: queued message indicator + delivered confirmation
- **No model changes, no sub-agents. Pure UX improvement.**

### Phase 2: Sub-Agent Infrastructure
- Implement sub-agent spawn/collect lifecycle
- Sub-agent types: explore, worker, researcher, planner
- Token budget caps per spawn
- Result aggregation back to orchestrator
- Tool for orchestrator: `sessions_spawn` enhanced with type/model/budget params

### Phase 3: Plan Mode
- Plan presentation format (structured, with cost estimates)
- Approval flow: webchat buttons, Signal text parsing
- Partial approval / step editing
- Plan execution engine (sequential or parallel steps)

### Phase 4: Automatic Routing
- Enhance complexity scorer with more signals
- Per-turn model selection (not per-session)
- Self-escalation mechanism
- Override commands ("think hard", "quick question")
- Ensure identity consistency across model tiers

### Phase 5: Cost Visibility
- Token cost calculation per model
- Turn-level cost tracking in trace
- UI cost indicators
- Budget enforcement (soft warnings, hard caps optional)
- Weekly usage reports

---

## Open Questions

1. **Sub-agent context**: Should sub-agents get any session history, or purely the task description? History adds cost but improves quality. Recommendation: task description + relevant file contents only, no conversation history.

2. **Parallel vs. sequential spawning**: Claude Code supports both foreground (blocking) and background (parallel) sub-agents. Should we default to parallel when tasks are independent? Recommendation: yes, with maxConcurrent=3.

3. **Sub-agent failure**: If a sub-agent hits its budget cap or fails, does the orchestrator retry on a higher model, report failure, or try itself? Recommendation: report failure to orchestrator, let it decide.

4. **Routing transparency**: Should the human see which model handled each turn? Recommendation: available on hover/detail but not prominently displayed. The experience should be seamless.

5. **Cache TTL strategy**: 5-minute default cache is free-ish (1.25x write). 1-hour cache is 2x write but better hit rates for long sessions. Worth it for bootstrap content that's read 20+ times per session? Recommendation: 1-hour for bootstrap, 5-minute for history.

6. **Signal compatibility**: Plan mode approval works naturally in webchat (buttons) but awkwardly in Signal (text parsing). Is text parsing sufficient or do we need structured Signal messages? Recommendation: text parsing is fine — "yes", "no", "skip 2" covers 95% of cases.

---

## Success Metrics

- **Cost reduction**: 40-60% reduction in weekly Anthropic spend
- **Interaction quality**: Human can send messages during agent work 100% of the time
- **Turn latency**: Sub-agent spawns complete faster than orchestrator doing it (parallel + cheaper model = faster)
- **Context clarity**: Orchestrator context stays focused on decisions, not mechanical output
- **No quality regression**: Opus handles all judgment; delegation is for mechanical work only
