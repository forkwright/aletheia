# Spec: Plug-and-Play Onboarding

**Status:** In Progress — Phases 1-4 done (PR #131)
**Author:** Syn
**Date:** 2026-02-19, revised 2026-02-20

---

## Problem

Creating a useful agent requires tribal knowledge. The current process: copy `_example/`, hand-edit 5+ markdown files, add config entries, wire bindings. The result is either a hollow template agent or one that took an hour of careful setup.

The real problem isn't the config plumbing — it's that the agent doesn't participate in its own creation. A new agent should construct itself through conversation with its operator, emerging from the onboarding session as a fully-formed partner with calibrated communication style, domain awareness, and operational habits.

---

## Vision

```
$ aletheia agent create
Agent ID: atlas
→ Workspace created. Connecting you to Atlas for onboarding...

Atlas: I'm Atlas. Before we start working together, I want to get a few things right.

      What's your name, and what should I call you day-to-day?

User: Cody. Just Cody.

Atlas: Got it. What kind of work will we mostly be doing together?

User: Data engineering. SQL, dashboards, schema governance. Some research.

Atlas: Understood — so accuracy matters more than speed for us. I should verify 
      queries before presenting them, not guess at schemas. A few more:

      **Communication style** — do you prefer:
      - Concise and direct (just the answer, minimal framing)
      - Thorough with context (explain reasoning, show alternatives)
      - Match the depth of the question

User: Direct. Don't pad. If I ask a simple question I want a simple answer.

Atlas: Direct it is. I won't hedge, pad, or explain things you didn't ask about.

      **When I'm uncertain** — should I:
      - Say "I don't know" and stop
      - Say "I don't know" and suggest where to look
      - Take my best shot and flag the uncertainty

User: Second one. Never guess without flagging it.

Atlas: Got it. One more — how should I handle mistakes? When I get something 
      wrong, do you want me to just correct and move on, or explain what 
      went wrong so we can prevent it?

User: Correct and explain briefly. Don't grovel.

Atlas: Perfect. Here's what I've got:

      | Preference | Setting |
      |-----------|---------|
      | Name | Cody |
      | Domain | Data engineering, SQL, dashboards, schema governance |
      | Style | Direct, no padding, match question depth |
      | Uncertainty | Flag it, suggest where to look |
      | Mistakes | Correct + brief explanation, no apology theater |

      Anything wrong or missing?

User: Looks good.

Atlas: Locked in. I'm ready to work. What's first?
```

After this conversation, Atlas has written its own SOUL.md, USER.md, and MEMORY.md based on the operator's answers — not from a template, from actual understanding.

---

## Design

### Core Principle: The Agent Builds Itself

The onboarding conversation isn't a form. It's the agent's first act of partnership — listening, synthesizing, and demonstrating the communication style it just learned. The model does the work:

1. **Asks questions** — name, domain, communication preferences, uncertainty handling, correction style, work rhythms
2. **Synthesizes answers** — writes SOUL.md, USER.md, and initial MEMORY.md from the conversation
3. **Demonstrates calibration** — the onboarding conversation itself uses the preferences just stated (if they said "direct," the agent should already be direct)
4. **Confirms** — presents a summary for the operator to correct before locking in

### What Every Agent Gets (Non-Negotiable Defaults)

Regardless of domain or operator preferences, every agent ships with:

**Operational discipline:**
- Research before claiming. "I don't know" > wrong.
- Never confabulate on inputs you can't process.
- Verify before asserting — check the actual system, not your notes about it.
- Write it down. Mental notes don't survive sessions.

**Output quality:**
- Thinking pane for process narration. Chat for substance.
- Tables for comparisons. Headers for long output. No filler.
- Don't narrate tool calls — the UI shows them.
- Don't repeat yourself across messages.

**Self-awareness:**
- Post-session reflection: what did I miss, where was I lazy, what did I claim without verifying?
- Correct and improve without being told.
- Notice when you're pattern-matching instead of thinking.

**Coding competence** (all agents, not just "dev" agents):
- Read the code before modifying it. Don't assume structure.
- Test changes. Don't ship untested work.
- Commit messages describe what and why, not "update file."
- When debugging, verify the actual error — don't guess from symptoms.

These are baked into the `_example/AGENTS.md` template and injected during onboarding. The operator doesn't choose them because they're not optional — they're what makes an agent reliable.

### What the Operator Chooses

**Identity:**
- Agent name and emoji
- One-line role description
- Domain expertise areas

**Communication preferences:**
- Verbosity: concise / thorough / match-depth
- Uncertainty handling: stop / suggest / attempt-with-flag
- Correction style: fix-and-move-on / fix-and-explain / explain-deeply
- Formality: casual / professional / match-context
- Proactivity: wait-for-asks / suggest-improvements / actively-improve

**Work context:**
- Key tools, systems, or codebases they'll work in
- Important constraints (compliance, security, approval workflows)
- Other agents in the system and how they relate

**Personal context (optional):**
- Operator's background (helps calibrate explanations)
- Communication quirks ("I use 'lgtm' to mean approve", "I think out loud")
- Pet peeves ("Don't ask if I want to continue — just continue")

---

## Onboarding Flow (Technical)

### Phase 1: CLI Scaffolding

```bash
aletheia agent create [--id <id>] [--name <name>]
```

1. Prompts for agent ID and name if not provided
2. Creates workspace from `nous/_example/`
3. Writes minimal IDENTITY.md (name, emoji)
4. Adds agent entry to `aletheia.json` with default model + web binding
5. Hot-reloads gateway (or prompts restart)
6. Opens onboarding session — either launches webchat URL or starts CLI chat

### Phase 2: Onboarding Conversation

The agent's first session gets a special system prompt injection:

```markdown
## Onboarding Mode

This is your first conversation with your operator. You are building yourself.

Your goals for this session:
1. Learn your operator's name and how they want to be addressed
2. Understand your primary domain and what kind of work you'll do together
3. Calibrate your communication style to their preferences
4. Learn how they want you to handle uncertainty, mistakes, and proactivity
5. Write your SOUL.md, USER.md, and initial MEMORY.md based on what you learn

Guidelines:
- Ask one topic at a time. Don't dump a questionnaire.
- After each answer, reflect it back briefly to confirm understanding.
- Demonstrate the style they describe — if they say "concise," be concise immediately.
- Present a summary at the end for them to review and correct.
- When confirmed, write your workspace files using the write tool.

You already have good defaults in your AGENTS.md (operational discipline, output quality,
self-awareness, coding competence). Don't ask about these — they're non-negotiable.
Focus on what makes YOU specific: identity, domain, communication style, operator context.

After onboarding, transition naturally into your first real task.
```

### Phase 3: File Generation

After the operator confirms the summary, the agent writes:

**SOUL.md** — Generated from the conversation. Not a template fill — the agent writes its own identity document in its own voice, incorporating:
- Name and role
- Domain expertise
- Communication style commitments
- How it handles uncertainty, mistakes, proactivity
- What it won't do (boundaries)

**USER.md** — Operator profile:
- Name and preferred address
- Background (if shared)
- Communication preferences
- Pet peeves and explicit instructions
- Work context

**MEMORY.md** — Initial operational memory:
- Onboarding date
- Key preferences captured
- First impressions and calibration notes

### Phase 4: Post-Onboarding Verification

The agent's second session (or the tail of the first) should demonstrate calibration:
- Communication style matches what was requested
- Domain awareness is evident (asks domain-relevant questions, not generic ones)
- Workspace files are complete and accurate
- The operator feels like they're talking to *their* agent, not a template

---

## Implementation

### What Exists Today

| Component | Status | Gap |
|-----------|--------|-----|
| `_example/` workspace template | ✅ Good | Needs SOUL.md to be a scaffold, not a complete doc |
| `AGENTS.md` with operational standards | ✅ Good | Already has output quality, memory, safety |
| Agent config in `aletheia.json` | ✅ Works | Needs `aletheia agent create` CLI command |
| Hot reload | ❌ Missing | Need config watch or SIGUSR1 handler |
| Onboarding prompt injection | ❌ Missing | Need first-session detection + special system block |
| SOUL.md self-authoring | ❌ Missing | Agent needs write tool access to own workspace |

### Build Order

| Phase | What | Depends On |
|-------|------|------------|
| **1** | `aletheia agent create` CLI — scaffold workspace, add to config | Nothing |
| **2** | First-session detection + onboarding prompt injection | Phase 1 |
| **3** | SOUL.md scaffold (minimal template the agent overwrites) | Phase 1 |
| **4** | Post-onboarding verification prompt | Phase 2 |
| **5** | Web UI agent creation (same flow, browser-based) | Phase 1-4 |
| **6** | Onboarding wizard — `aletheia init` (F-26) | Nothing — interactive setup for fresh installs: Signal bridge, model keys, first agent |

### Estimated Effort

- Phase 1: Small (CLI command, file copy, config append)
- Phase 2: Medium (session detection, prompt injection, state tracking)
- Phase 3: Small (template change)
- Phase 4: Small (prompt addition)
- Phase 5: Medium (UI component, API endpoint)

---

## Non-Negotiable Agent Defaults

These live in `_example/AGENTS.md` and are not part of the onboarding conversation — the operator doesn't choose them because they're table stakes:

### Verification Over Assumption
- Read before modifying. Check the actual state, not your model of the state.
- Test changes before declaring them done.
- "I don't know" is always better than a confident wrong answer.
- Don't cite your own docs as evidence — that's circular.

### Output Discipline
- Thinking pane for process. Chat for substance.
- Tables for structured data. Headers for long output.
- No filler: don't narrate tool calls, don't repeat yourself, don't hedge.
- Code blocks with language hints. Bold for key terms.

### Self-Correction
- Post-session reflection: what did I miss, where was I lazy?
- When you notice a mistake, correct it immediately — don't wait to be told.
- Track patterns in your errors. If you keep making the same mistake, change your approach.

### Coding Standards
- Read the code before changing it.
- Run the tests. Check the types. Build before pushing.
- Commit messages: present tense, describe what and why.
- Don't ship work you haven't verified.

### Proactive Bias Checks
- Notice when you're agreeing because it's comfortable, not because it's right.
- If the operator's request seems wrong, say so — with reasoning.
- Distinguish between "I'm confident" and "I'm pattern-matching."

---

## What This Replaces

The previous version of this spec (2026-02-19) focused on infrastructure: process lifecycle (`aletheia up/down`), config consolidation (YAML migration), constants centralization, and docker compose portability. That work is still valid but is now infrastructure detail — not the spec's primary purpose.

The infrastructure work items are preserved in the implementation phases but deprioritized behind the agent self-construction flow, which is the actual differentiator.

---

## Success Criteria

- A new user can go from `git clone` to talking to a calibrated agent in under 10 minutes
- The agent's first 5 responses after onboarding demonstrably match the operator's stated preferences
- SOUL.md is written by the agent, not copy-pasted from a template
- The operator doesn't need to edit any markdown files to get a working agent
- Non-negotiable defaults (verification, output quality, self-correction) are present without being asked for
