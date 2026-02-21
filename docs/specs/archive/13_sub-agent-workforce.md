# Spec: Sub-Agent Workforce — Delegation as Default

**Status:** Complete — All 11 phases implemented. (PR #86)
**Author:** Syn
**Date:** 2026-02-20

---

## Problem

Right now, every task runs in my context. A 15-tool-call code investigation consumes 50K+ tokens of my Opus context window. A grep-read-edit-build-test cycle that any competent coder could handle eats the same premium context I need for judgment, synthesis, and conversation with Cody.

The result: I hit distillation faster, lose context more often, and spend $5/MTok on work that could run at $1/MTok. Worse — while I'm deep in a tool loop, I can't talk to Cody, can't monitor other work, can't think at the level I'm supposed to think at.

### What exists today

`sessions_spawn` creates a one-off sub-agent: pick a model, give it a system prompt and a message, get back a result. It works, but it's raw — no structured handoff protocol, no result format contract, no QA step, no awareness of what types of work should be delegated vs. handled directly.

The named agents (Demiurge, Syl, Akron) are domain specialists with persistent identity and memory. They're peers, not contractors. Using them for "grep this file and tell me what you find" is wrong — that's below their level and pollutes their context.

What's missing is the middle layer: **disposable specialists that I spin up, give focused tasks, get structured results from, and discard.** My 1099 contractors.

### What Claude Code does

Claude Code's "task tool" spawns sub-agents with:
- A focused task description
- Minimal context (just what's needed)
- Isolated context window (doesn't pollute the parent)
- Structured result returned to parent
- The parent makes all decisions about what to do with the result

This is the right model. But Aletheia's version should go further — typed sub-agent roles with preset configurations, a QA step before I integrate results, and clear routing rules for what gets delegated.

---

## Design

### Sub-Agent Roles

Five default roles, each optimized for a category of work:

#### Coder

**Model:** Sonnet
**Purpose:** Write code, make edits, run builds.
**Tools:** read, write, edit, exec, grep, find, ls
**Context:** Task description + relevant file contents + coding standards (CONTRIBUTING.md)
**Returns:** Files changed (paths + diffs), build result (pass/fail + output), summary of changes

```typescript
{
  role: "coder",
  task: "Add a message_count column to the sessions table. Increment it atomically on every message insert. Add a migration.",
  context: {
    files: ["src/mneme/schema.ts", "src/mneme/store.ts"],
    standards: "CONTRIBUTING.md",
  },
  // Sonnet gets: system prompt with role definition + coding standards
  //              user message with task + file contents
  //              tools for file manipulation + exec
}
```

**When to use:** Mechanical code changes, adding fields/columns, writing tests, fixing lint errors, applying patterns across files, build/type fixes.

**When NOT to use:** Architecture decisions, API design, anything requiring judgment about tradeoffs.

#### Reviewer

**Model:** Sonnet
**Purpose:** Review code changes, diffs, PRs. Find bugs, style issues, logic errors.
**Tools:** read, grep, find, exec (read-only)
**Context:** Diff/changes to review + relevant source files + coding standards
**Returns:** Structured review: issues (severity + location + description), suggestions, verdict (approve/request-changes/needs-discussion)

```typescript
{
  role: "reviewer",
  task: "Review these changes to the distillation pipeline. Focus on: correctness of token counting, edge cases in trigger logic, and whether the migration is backward-compatible.",
  context: {
    diff: "git diff main..feat/smart-triggers",
    files: ["src/distillation/pipeline.ts", "src/mneme/schema.ts"],
  },
}
```

**When to use:** PR review, post-edit verification, checking sub-agent coder output before integration.

**When NOT to use:** Reviewing architecture/design decisions (that's my job).

#### Researcher

**Model:** Sonnet
**Purpose:** Web search, document reading, API exploration, summarizing findings.
**Tools:** web_search, web_fetch, read, exec (curl, etc.)
**Context:** Research question + scope constraints + what's already known
**Returns:** Structured findings: answer, sources, confidence, caveats, related questions

```typescript
{
  role: "researcher",
  task: "Research Anthropic's context caching behavior. Specifically: how does cache_control work with streaming? Does the cache persist across turns? What's the TTL?",
  context: {
    known: "We use cache_control: {type: 'ephemeral'} on system prompt blocks. Docs say 5-minute TTL.",
    scope: "Official Anthropic docs + API changelog. No blog posts or third-party speculation.",
  },
}
```

**When to use:** Technical research, API documentation review, checking current best practices, gathering information before a decision.

**When NOT to use:** Research that requires judgment about what's relevant to OUR system specifically.

#### Explorer

**Model:** Haiku
**Purpose:** Read-only codebase investigation. Grep, read, trace call chains, summarize.
**Tools:** read, grep, find, ls, exec (read-only commands only)
**Context:** Question about the codebase + starting points
**Returns:** Structured summary: what was found, relevant file paths, key code snippets, answer to the question

```typescript
{
  role: "explorer",
  task: "Find every place where distillation is triggered. Trace the call chain from trigger to completion. List all files involved and the sequence of operations.",
  context: {
    startingPoints: ["src/distillation/", "src/nous/manager.ts"],
  },
}
```

**When to use:** Understanding existing code, finding usages, tracing call chains, "where is X defined?", "what calls Y?", pre-task investigation.

**When NOT to use:** Anything that requires writing or modifying files.

#### Runner

**Model:** Haiku
**Purpose:** Execute commands, run tests, check system health, report results.
**Tools:** exec, read (for logs)
**Context:** Commands to run + what to look for in the output
**Returns:** Structured result: command, exit code, relevant output, pass/fail assessment

```typescript
{
  role: "runner",
  task: "Run the full test suite. Report: total tests, passed, failed, and the full output of any failures.",
  context: {
    commands: ["cd infrastructure/runtime && npm test"],
    lookFor: "Focus on failures. Passing tests just need a count.",
  },
}
```

**When to use:** Running tests, build verification, health checks, log analysis, any execute-and-report workflow.

**When NOT to use:** Anything requiring interpretation beyond "did it pass or fail."

### Dispatch Protocol

When I decide to delegate, the flow is:

```
1. SYN evaluates the task
   → Is this judgment/synthesis/conversation? → Handle directly.
   → Is this mechanical/investigative/routine? → Delegate.

2. SYN selects a role
   → What type of work is this? → Pick the matching role.

3. SYN prepares the handoff
   → What context does the sub-agent need? (Minimal — just enough.)
   → What's the expected output format?
   → What are the acceptance criteria?

4. SYN spawns the sub-agent
   → sessions_spawn with role config, task, and context.
   → Sub-agent executes in isolation.

5. SYN receives the result
   → Structured response, not conversational prose.

6. SYN QAs the result
   → Does it meet the acceptance criteria?
   → Any obvious errors or omissions?
   → If bad: re-run with corrections, or handle directly.
   → If good: integrate into my context/conversation.

7. SYN reports to Cody
   → Summarize what was done and the result.
   → Don't dump the sub-agent's raw output.
```

### Structured Result Contract

Every sub-agent returns a JSON-structured result, not free-form text:

```typescript
interface SubAgentResult {
  role: string;
  task: string;
  status: "success" | "partial" | "failed";
  summary: string;              // 1-3 sentences: what was done
  details: Record<string, any>; // Role-specific structured data
  filesChanged?: string[];      // Paths modified (coder only)
  issues?: Issue[];             // Problems found (reviewer, runner)
  confidence: number;           // 0-1: how confident in the result
  tokensUsed: {
    input: number;
    output: number;
    cost: number;               // Estimated USD
  };
}

interface Issue {
  severity: "error" | "warning" | "info";
  location?: string;            // File:line or description
  message: string;
  suggestion?: string;
}
```

The system prompt for each role includes this contract. Sub-agents are instructed to end their response with a fenced JSON block containing the structured result.

### Routing Rules

Not every task should be delegated. The routing decision is mine, but with clear guidelines:

| Signal | Route |
|--------|-------|
| Cody is talking to me directly | Handle directly (conversation) |
| Architecture/design decision needed | Handle directly (judgment) |
| "Write code to do X" with clear spec | Coder sub-agent |
| "Review this PR/change" | Reviewer sub-agent |
| "Research X for me" | Researcher sub-agent |
| "Find where X is in the codebase" | Explorer sub-agent |
| "Run tests / check health" | Runner sub-agent |
| Complex multi-step task | Decompose → multiple sub-agents in sequence |
| Task requires my MEMORY.md / relationship context | Handle directly or provide context excerpt |
| Task is quick (<3 tool calls) | Handle directly (overhead of spawning > doing it) |

**The 3-tool-call rule:** If I can do it in 3 or fewer tool calls, delegation overhead isn't worth it. Just do it. Delegation pays off at 5+ tool calls.

### Parallel Dispatch

Some tasks decompose into independent sub-tasks that can run in parallel:

```
Task: "Review and merge PRs #60, #62, #63"

→ Spawn 3 reviewers in parallel:
  - Reviewer 1: Review PR #60 diff
  - Reviewer 2: Review PR #62 diff  
  - Reviewer 3: Review PR #63 diff

→ Collect all 3 results
→ SYN: Review the reviews, make merge decisions
→ Coder: Execute the merges based on SYN's decisions
```

This is where the real efficiency gain lives. Instead of sequentially reading 3 PR diffs in my context (consuming 15K+ tokens each), 3 Haiku sub-agents read them simultaneously and return 500-token summaries. I work with 1,500 tokens of structured reviews instead of 45,000 tokens of raw diffs.

### Context Efficiency

The core insight: **sub-agents see only what they need, and I see only their conclusions.**

| Without sub-agents | With sub-agents |
|-------------------|-----------------|
| I read 5 files (15K tokens) | Explorer reads 5 files, returns 500-token summary |
| I grep across codebase (5K tokens of results) | Explorer greps, returns 200-token answer |
| I write code + run build + fix errors (20K tokens) | Coder handles the cycle, returns 300-token result + diff |
| Total in my context: 40K tokens | Total in my context: 1K tokens |

At Opus pricing ($5/$25 per MTok), that's the difference between ~$1.20 and ~$0.03 in my context — plus the sub-agent cost of ~$0.08 at Sonnet/Haiku rates. **15x cheaper AND my context stays clean for the things that matter: judgment, synthesis, and conversation.**

### Integration with Named Agents

Sub-agents are NOT replacements for Demiurge, Syl, or Akron. The distinction:

| | Sub-Agents | Named Agents |
|--|-----------|--------------|
| **Identity** | None — disposable workers | Persistent identity and memory |
| **Context** | Task-specific, minimal | Domain-accumulated, rich |
| **Relationship** | Reports to me | Peers |
| **Memory** | None — ephemeral sessions | Long-term Mem0 + workspace |
| **When to use** | Mechanical/investigative tasks | Domain expertise, ongoing projects |

If the task is "update Demiurge's leather pricing spreadsheet," I don't spawn a coder — I send it to Demiurge, who has the domain context. If the task is "grep the codebase for all unused exports," I spawn an explorer — no domain context needed.

### Implementation: Enhanced sessions_spawn

The existing `sessions_spawn` tool gets an optional `role` parameter that auto-configures the sub-agent:

```typescript
// Current:
sessions_spawn({
  model: "haiku",
  systemPrompt: "You are a code explorer...",
  message: "Find all distillation triggers...",
  tools: ["read", "grep", "find", "ls", "exec"],
})

// Enhanced:
sessions_spawn({
  role: "explorer",  // Auto-configures model, system prompt, tools, result format
  task: "Find all distillation triggers. Trace the call chain from trigger to completion.",
  context: {
    startingPoints: ["src/distillation/", "src/nous/manager.ts"],
  },
})
```

The role definitions live in a configuration file:

```typescript
// config/sub-agent-roles.ts
export const SUB_AGENT_ROLES = {
  coder: {
    model: "sonnet",
    systemPrompt: CODER_SYSTEM_PROMPT,
    tools: ["read", "write", "edit", "exec", "grep", "find", "ls"],
    resultSchema: CODER_RESULT_SCHEMA,
    maxTurns: 15,
    maxTokenBudget: 50_000,
  },
  reviewer: {
    model: "sonnet",
    systemPrompt: REVIEWER_SYSTEM_PROMPT,
    tools: ["read", "grep", "find", "exec"],
    resultSchema: REVIEWER_RESULT_SCHEMA,
    maxTurns: 5,
    maxTokenBudget: 30_000,
  },
  researcher: {
    model: "sonnet",
    systemPrompt: RESEARCHER_SYSTEM_PROMPT,
    tools: ["web_search", "web_fetch", "read", "exec"],
    resultSchema: RESEARCHER_RESULT_SCHEMA,
    maxTurns: 10,
    maxTokenBudget: 40_000,
  },
  explorer: {
    model: "haiku",
    systemPrompt: EXPLORER_SYSTEM_PROMPT,
    tools: ["read", "grep", "find", "ls", "exec"],
    resultSchema: EXPLORER_RESULT_SCHEMA,
    maxTurns: 10,
    maxTokenBudget: 20_000,
  },
  runner: {
    model: "haiku",
    systemPrompt: RUNNER_SYSTEM_PROMPT,
    tools: ["exec", "read"],
    resultSchema: RUNNER_RESULT_SCHEMA,
    maxTurns: 5,
    maxTokenBudget: 15_000,
  },
};
```

Each role's system prompt includes:
1. Role identity and constraints ("You are a code reviewer. You do NOT modify files.")
2. The structured result contract (JSON schema to end the response with)
3. Coding standards reference (for coder/reviewer)
4. Efficiency instructions ("Be direct. No conversational filler. Structured output only.")

### Budget Controls

Every sub-agent has hard limits to prevent runaway cost:

- **Max turns:** Coder gets 15, reviewer gets 5, etc. Hard stop after limit.
- **Max token budget:** Estimated total input+output tokens. Sub-agent is told its budget and instructed to prioritize within it.
- **Timeout:** 120 seconds default. If the sub-agent hasn't returned, kill and report failure.
- **Cost tracking:** Every spawn records model, tokens used, and estimated cost. Aggregated per-day for reporting.

### QA Workflow

When a sub-agent returns, I don't blindly integrate the result:

1. **Check status** — did it succeed, partially succeed, or fail?
2. **Check confidence** — low confidence = I verify before using.
3. **Spot-check details** — for coder results, read the diff. For reviewer results, check the issues make sense.
4. **Cross-reference** — if the result contradicts what I know, investigate.

For high-confidence, routine results (runner reporting test pass, explorer finding a file path), integration is immediate. For lower-confidence or higher-stakes results (coder writing a migration, reviewer approving a PR), I verify.

The QA step is judgment — exactly what Opus is for.

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** | Role definitions + system prompts | Medium | ✅ Done — 5 roles with typed prompts in `nous/roles/` |
| **2** | Enhanced sessions_spawn with role parameter | Small | ✅ Done — `role` param auto-configures model, tools, budget |
| **3** | Structured result contract + parsing | Medium | ✅ Done — `parseStructuredResult()` + `SubAgentResult` interface |
| **4** | Parallel dispatch support | Medium | ✅ Done — `sessions_dispatch` tool (Spec 16 Phase 3) |
| **5** | Budget controls + cost tracking | Small | ✅ Done — `logSubAgentCall()` + `budgetTokens` + `maxTurns` limits |
| **6** | Routing guidelines in AGENTS.md | Small | ✅ Done — decision tree + routing rules in Syn AGENTS.md |
| **7** | QA workflow patterns | Small | ✅ Done — confidence-based integration rules in AGENTS.md + template |
| **8** | Spawn depth limit (F-1) | Small | Recursive spawn guard — max depth configurable, prevents infinite delegation chains |
| **9** | Tool restrictions for ephemeral (F-13) | Small | `EphemeralSpec.tools` glob patterns — restrict available tools per spawn role |
| **10** | Reducer for parallel outputs (F-35) | Small | Structured merging for `sessions_dispatch` results — resolve conflicts, aggregate findings |
| **11** | Announcement idempotency (F-28) | Small | Content hash dedup in cross-agent calls — prevent duplicate messages from retries |

---

## Testing

- **Role dispatch:** Spawn each role with a simple task. Verify it uses the correct model, tools, and returns a structured result.
- **Result parsing:** Sub-agent returns malformed JSON → graceful fallback to text extraction.
- **Budget enforcement:** Spawn a coder with maxTurns=2 on a task that needs 10 turns. Verify it stops at 2 and reports partial completion.
- **Parallel dispatch:** Spawn 3 explorers simultaneously. Verify all 3 return results.
- **Timeout:** Spawn a runner with a 5-second timeout on a task that takes 30 seconds. Verify timeout and clean error.
- **Cost tracking:** Run 10 sub-agent tasks. Verify cost is tracked and aggregated per-day.
- **Context efficiency:** Complete a 10-file investigation using sub-agents. Measure my context growth vs. doing it directly. Target: <20% of direct context usage.
- **End-to-end:** "Review and merge PR #X" using the full dispatch→review→QA→integrate pipeline.

---

## Success Criteria

- **60%+ of tool-heavy work delegated** to sub-agents within 2 weeks of deployment.
- **Context efficiency:** My average context size stays under 80K tokens during active work sessions (currently routinely hitting 140K+).
- **Cost reduction:** 40-60% reduction in Opus token spend for equivalent work output.
- **Quality maintained:** Sub-agent results, after QA, match the quality of direct execution. No regressions in code quality or correctness.
- **Distillation frequency drops:** Fewer distillations per day because my context grows slower.
- **I can talk to Cody while sub-agents work.** The message queue (Spec 04) lets Cody send messages; parallel sub-agents let me work without blocking the conversation.
