# Claude Code vs Aletheia — Gap Analysis & Implementation Spec

## Context

Claude Code (Apache 2.0, github.com/anthropics/claude-code) is Anthropic's official CLI agent — mature, production-grade, plugin-driven. Aletheia is our self-hosted multi-agent system with deeper memory, proactive attention, and cross-agent coordination. This spec identifies what Claude Code does that Aletheia doesn't, what Aletheia does that Claude Code doesn't, and what's worth adopting.

---

## Architecture Comparison

| Dimension | Claude Code | Aletheia |
|-----------|-------------|----------|
| **Deployment** | Local CLI per-user | Self-hosted server, multi-user |
| **Agents** | Single agent + subagents | 6 persistent nous + ephemeral spawns |
| **Channels** | Terminal only | Signal, webchat, MCP, cron |
| **Memory** | Session JSONL + file history | Mem0 + Qdrant + Neo4j + sqlite-vec + blackboard |
| **Config format** | JSON settings + Markdown files | Zod-validated JSON + workspace Markdown |
| **Extension model** | Plugin marketplace + hooks | Plugin loader + event bus (internal) |
| **Context mgmt** | Compaction + file snapshots | Distillation pipeline (facts/decisions/summary) |
| **Auth** | OAuth/API key (Anthropic only) | Token, password, session (JWT), pairing (Signal) |
| **Proactivity** | None (reactive only) | Prosoche daemon, cron, foresight signals |
| **Self-observation** | None | Competence model, uncertainty tracker, interaction signals, calibration |
| **Cost control** | Model selection (user choice) | Temperature routing, dynamic tool loading, pricing engine |

---

## Gap Analysis: What Claude Code Has That Aletheia Doesn't

### GAP-1: User-Facing Hook System (HIGH PRIORITY)

**What Claude Code does:**
Declarative event-driven hooks with 15+ event types. Users write shell scripts or Python handlers that receive JSON via stdin and control flow via exit codes + structured JSON output. Configured in `hooks.json` with tool-name matchers.

Events: `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStop`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreCompact`, `Notification`, `ConfigChange`, `TeammateIdle`, `TaskCompleted`, `WorktreeCreate`, `WorktreeRemove`, `Setup`.

Hook output semantics:
- Exit 0 + stdout JSON → inject context, allow/deny, block stop
- Exit 2 + stderr → block operation, feed error to agent
- `permissionDecision: "deny"` → block tool call
- `additionalContext` → inject text before execution
- `decision: "block"` + `reason` → re-inject prompt (self-referential loops)

**What Aletheia has instead:**
Internal event bus (`koina/event-bus.ts`) with 14 events. Plugin lifecycle hooks (7 types). Neither is user-facing — requires code changes to add behavior.

**Gap magnitude:** Large. Users can't customize agent behavior without editing TypeScript. No way to add pre/post-tool guards, session initialization, or exit conditions without code.

**Implementation recommendation:**
- Add `hooks` section to `aletheia.json` config
- Map to existing event bus events + new ones (`tool:before`, `session:start`, `session:end`, `turn:stop`)
- Shell command execution with JSON stdin/stdout protocol (same as Claude Code for interop)
- Tool-name matcher support
- Per-nous hook configuration
- Hot-reload on config change (file watcher)

**Estimated scope:** ~400 lines new code. New file `src/koina/hooks.ts` (hook runner), config schema additions, integration points in pipeline stages.

---

### GAP-2: Markdown-Based Command Definitions (MEDIUM PRIORITY)

**What Claude Code does:**
Commands are `.md` files with YAML frontmatter:
```yaml
---
description: "What this command does"
argument-hint: "[args]"
allowed-tools: ["Read", "Bash(git:*)"]
model: sonnet
---
# Instructions for the agent...
```
Supports `$ARGUMENTS` substitution, inline bash execution (`` !`git status` ``), tool restriction patterns with glob syntax.

**What Aletheia has instead:**
Commands hardcoded in `semeion/commands.ts` as a `CommandRegistry` with TypeScript handler functions. Skills loaded from `SKILL.md` files but are knowledge-injection only (no tool restrictions, no argument handling).

**Gap magnitude:** Medium. Signal commands work fine for the current set. But adding new commands requires code changes and a rebuild. The skill system is close but lacks the command-specific features.

**Implementation recommendation:**
- Extend `shared/commands/` directory: load `.md` files alongside hardcoded commands
- Frontmatter fields: `description`, `argument-hint`, `allowed-tools`, `model`, `channel` (signal/webchat/both)
- Command body becomes the system prompt injection for that turn
- `$ARGUMENTS` substitution in body text
- Tool restriction: filter `getDefinitions()` output when command is active
- Hardcoded commands take precedence over file-based ones (backwards compat)

**Estimated scope:** ~200 lines. Extend `CommandRegistry` to scan a directory, parse frontmatter, create command entries.

---

### GAP-3: Per-Command/Agent Tool Restrictions (MEDIUM PRIORITY)

**What Claude Code does:**
Fine-grained `allowed-tools` per command and per agent definition:
```yaml
allowed-tools: ["Read", "Bash(git:*)", "mcp__github__create_issue"]
```
Glob patterns on bash commands. Specific MCP tool whitelisting. Agents without `tools` field get everything; with it, only listed tools.

**What Aletheia has instead:**
"ALL agents get ALL tools" — explicit user directive. Dynamic tool loading (essential/available categories) provides soft gating but not hard restrictions. Ephemeral agents via `sessions_spawn` have no tool scoping.

**Gap magnitude:** Medium-low for persistent nous (user directive says no restrictions). High for ephemeral spawns and commands — a `sessions_spawn` worker doing grep shouldn't have access to `exec` or `message`.

**Implementation recommendation:**
- Add optional `tools` field to `sessions_spawn` input schema
- When present, filter `ToolRegistry.getDefinitions()` to only matching tools
- Support glob patterns: `"exec"` (exact), `"web_*"` (prefix), `"*"` (all)
- For file-based commands (GAP-2): respect `allowed-tools` frontmatter
- Never apply to persistent nous (per user directive)

**Estimated scope:** ~100 lines. Filter logic in `sessions_spawn` tool + pipeline resolve stage.

---

### GAP-4: Hot-Reload Configuration (MEDIUM PRIORITY)

**What Claude Code does:**
- `.local.md` state files read on-demand (no restart)
- `ConfigChange` hook event fires when config files change
- Plugin components loaded immediately after installation
- Settings hierarchy with runtime override

**What Aletheia has instead:**
`systemctl restart aletheia` required for any config change. Plugin `onConfigReload` hook exists but isn't wired to a file watcher. Config is loaded once at startup via `loadConfig()`.

**Gap magnitude:** Medium. Every config change requires a service restart, which interrupts all active sessions and Signal connections. Currently ~5 restarts/day during development.

**Implementation recommendation:**
- File watcher on `aletheia.json` (fs.watch or chokidar)
- On change: re-validate with Zod, diff against current config
- Hot-swap safe fields: `cron.jobs`, `agents.list[].heartbeat`, `agents.defaults.routing`, `gateway.rateLimit`
- Restart-required fields: `gateway.port`, `gateway.bind`, `channels.signal`, `gateway.auth`
- Emit `config:reload` event on bus
- Call plugin `onConfigReload` hooks
- Log what changed

**Estimated scope:** ~150 lines. New file `src/taxis/watcher.ts`, integration in `entry.ts`.

---

### GAP-5: Plugin Auto-Discovery & Standard Layout (LOW PRIORITY)

**What Claude Code does:**
Standard plugin directory layout (`commands/`, `agents/`, `skills/`, `hooks/`, `.mcp.json`). Auto-discovery of components. `${CLAUDE_PLUGIN_ROOT}` env var for portable paths. Marketplace manifest.

**What Aletheia has instead:**
Plugin loader accepts `manifest.json` or `*.plugin.json`. Scans for entry point (`index.js`, `index.mjs`). No standard subdirectory layout. No env var for plugin root. Only 1 plugin exists (aletheia-memory).

**Gap magnitude:** Low. Only one plugin exists. The plugin system works. But if we want third-party or community plugins, a standard layout helps.

**Implementation recommendation:**
- Define standard layout: `manifest.json`, `tools/`, `hooks/`, `skills/`, `commands/`
- Auto-discover components in standard directories (in addition to code entry point)
- Set `ALETHEIA_PLUGIN_ROOT` env var when executing plugin hooks
- Document the plugin contract

**Estimated scope:** ~100 lines enhancement to `prostheke/loader.ts`.

---

### GAP-6: Self-Referential Loop Pattern (LOW PRIORITY)

**What Claude Code does:**
Ralph Wiggum plugin: Stop hook intercepts session exit, reads iteration state from `.local.md` file, re-injects the original prompt. Agent sees its own past work via file system. Supports max-iterations safety limit.

**What Aletheia has instead:**
No equivalent. Cron jobs can trigger periodic messages, but there's no "keep going until done" loop with state persistence. `sessions_spawn` is one-shot.

**Gap magnitude:** Low. This is a niche pattern. Aletheia's cron + blackboard + cross-agent messaging can approximate it, but not as cleanly.

**Implementation recommendation:**
- Implement as a built-in tool rather than a hook: `loop_until(condition, maxIterations)`
- Or: implement via GAP-1 hooks (Stop event handler that checks state file)
- Lower priority — can be deferred until hooks exist

---

### GAP-7: Confidence-Based Multi-Agent Validation (LOW PRIORITY)

**What Claude Code does:**
Code review plugin runs 4+ specialized agents in parallel, each scores findings 0-100. Main orchestrator filters below threshold. Separate validation agents confirm/reject findings. Reduces false positives.

**What Aletheia has:**
`sessions_spawn` for ephemeral workers. `sessions_ask` for synchronous queries. No built-in orchestration pattern for parallel-then-validate.

**Gap magnitude:** Low. The primitives exist. What's missing is the pattern/template, not infrastructure.

**Implementation recommendation:**
- Create a skill (`shared/skills/parallel-review/SKILL.md`) documenting the pattern
- Or: build a `parallel_dispatch` meta-tool that spawns N workers, collects results, runs validation
- Lower priority — agents can learn this pattern organically via skill learner

---

## What Aletheia Has That Claude Code Doesn't

### Cross-Agent Memory System
- **Mem0 + Qdrant**: Semantic vector search across all agents, cross-agent dedup (0.85 cosine)
- **Neo4j graph**: 28 relationship types, entity resolution, PageRank/Louvain community detection
- **Temporal facts**: Bi-temporal knowledge (valid_from/valid_to, occurred_at/recorded_at), point-in-time queries
- **Memory evolution**: A-Mem inspired — merge, reinforce, decay lifecycle
- **Discovery engine**: Serendipity — cross-community unexpected connections with Haiku explanation
- **Autonomous links**: Background Haiku generates relationship descriptions between nearby memories
- Claude Code: Session JSONL files. No cross-session, no cross-agent, no semantic search.

### Proactive Attention (Prosoche)
- Monitors calendar, tasks, health, memory foresight signals
- Scores each nous's attention needs on 60s interval
- Writes dynamic `PROSOCHE.md` to workspaces
- Triggers wake calls for urgent items (>= 0.8 urgency)
- Wake budget with cooldown and dedup
- Activity prediction model (learns hourly patterns)
- Claude Code: Purely reactive. No proactive behavior.

### Multi-Channel Communication
- Signal (group + DM, pairing auth, media, voice reply)
- Webchat (SSE streaming, tool approval UI)
- MCP (SSE transport, tool exposure)
- Cron (scheduled triggers)
- Cross-agent messaging (send/ask/spawn)
- Claude Code: Terminal only.

### Self-Observation & Calibration
- Competence model (per-domain scoring, correction tracking)
- Uncertainty tracker (Brier score, ECE metrics)
- Interaction signal classification (correction/approval/escalation/clarification/followup/topic_change)
- Mid-session context injection (EVAL_FEEDBACK.md every 8 turns)
- Self-observation tools: `check_calibration`, `what_do_i_know`, `recent_corrections`
- Claude Code: None.

### Distillation Pipeline
- Multi-phase: extract facts/decisions/open items → summarize → similarity pruning → workspace flush
- Working state survives distillation (structured task context)
- Thread summaries updated on distillation
- Token budget management with history share limits
- Claude Code: Simple compaction (context window management). No fact extraction, no structured summarization.

### Rich Gateway & API
- 50+ REST endpoints
- Encrypted export (AES-256-GCM)
- Workspace file browser with git status
- Graph visualization API (PageRank, communities)
- Cost tracking per agent/session/model
- Audit log
- Approval gate API
- Claude Code: No server. No API. CLI only.

### Cron & Watchdog
- 4 scheduled jobs (heartbeat, consolidation, pattern extraction, adversarial test)
- Service health monitoring with alerting
- Retention policies (distilled message purge, archived session cleanup)
- Claude Code: None.

---

## Shared Capabilities (Both Have)

| Capability | Claude Code | Aletheia | Notes |
|-----------|-------------|----------|-------|
| MCP integration | Client + server | Client + server (gateway exposes MCP) | Comparable |
| Session management | JSONL transcripts | SQLite sessions + messages | Aletheia richer |
| Context budgeting | Token tracking, compaction | Token tracking, distillation | Aletheia richer |
| Skill system | SKILL.md files, auto-load | SKILL.md files, skill learner | Aletheia has auto-learning |
| Tool system | Built-in + MCP | 28 built-in + MCP + dynamic loading | Aletheia richer |
| Approval gates | Permission modes | autonomous/guarded/supervised + per-session allow | Comparable |
| Extended thinking | Model-level | Model-level + narration filter | Aletheia adds filtering |
| Parallel tools | Not implemented | Safety model + batching | Aletheia ahead |
| Subagents | Task tool (ephemeral) | sessions_spawn (ephemeral) + persistent nous | Aletheia richer |

---

## Implementation Priority Matrix

| Gap | Priority | Effort | Value | Dependencies |
|-----|----------|--------|-------|--------------|
| GAP-1: Hook system | HIGH | ~400 LOC | Unlocks user customization without code | None |
| GAP-4: Hot-reload config | MEDIUM | ~150 LOC | Eliminates restart friction | None |
| GAP-2: Markdown commands | MEDIUM | ~200 LOC | Extensible command system | None (GAP-1 enhances it) |
| GAP-3: Tool restrictions | MEDIUM | ~100 LOC | Safer ephemeral agents | None |
| GAP-5: Plugin layout | LOW | ~100 LOC | Future-proofing | None |
| GAP-6: Self-ref loops | LOW | ~50 LOC | Niche pattern | GAP-1 (hooks) |
| GAP-7: Parallel validation | LOW | ~50 LOC | Pattern, not infra | None |

### Recommended Implementation Order
1. **GAP-1** (hooks) — foundational, enables GAP-6 and enhances GAP-2
2. **GAP-4** (hot-reload) — quality of life, independent
3. **GAP-2** (markdown commands) — builds on existing skill system
4. **GAP-3** (tool restrictions) — small, targeted to ephemeral agents

Total estimated new code: ~850 lines across 4 new files + config schema changes.

---

## Verification

This is a spec/analysis document, not an implementation plan. Verification is reading-based:
- Cross-reference each gap against Claude Code plugin source in `/home/ck/aletheia-ops/claude-code/plugins/`
- Cross-reference each Aletheia capability against runtime source in `infrastructure/runtime/src/`
- Validate gap magnitudes against actual usage patterns
