# Unified Gap Analysis: Aletheia vs Claude Code vs OpenClaw

## Context

Three multi-agent systems compared to identify features worth adopting into Aletheia.

**Aletheia** — Self-hosted multi-agent system. 6 persistent nous + ephemeral spawns. Signal + webchat + MCP channels. Mem0 + Qdrant + Neo4j + sqlite-vec memory. Prosoche daemon (proactive attention). Self-observation (competence model, uncertainty tracker, calibration). Distillation pipeline. 28 built-in tools + dynamic loading. Event bus (15 events). Cron, watchdog. Gateway with 50+ endpoints. ~328KB runtime. Stack: Hono, better-sqlite3, @anthropic-ai/sdk, Zod, Commander.

**Claude Code** — Anthropic's official CLI agent (Apache 2.0). Single agent + ephemeral subagents. Terminal-only. Session JSONL + file history. Declarative hook system (15+ events). Plugin marketplace. Purely reactive. No persistent memory, no proactive behavior, no self-observation.

**OpenClaw** — Multi-channel agent framework (MIT). Single agent, multi-session. 17+ channels (Telegram, Discord, Signal, Slack, WhatsApp, iMessage, Matrix, etc.). 38 extension plugins. Docker sandbox. ACP IDE bridge. Playwright browser automation. Auth profile rotation. MMR memory search. 56 bundled skills. iOS/macOS/Android companion apps. ~406K LOC TypeScript.

**Constraints:**
- Channels: Signal + webchat only (no new channel integrations)
- Tools: ALL agents get ALL tools (persistent nous — no per-agent restrictions)
- Evaluate: Docker sandbox, ACP/IDE integration, Android app as webui port
- Skip: WhatsApp, Telegram, Discord, Slack, Matrix, iOS/macOS apps

---

## Architecture Comparison

| Dimension | Claude Code | OpenClaw | Aletheia |
|-----------|-------------|----------|----------|
| **Deployment** | Local CLI per-user | Server + CLI + mobile apps | Self-hosted server, multi-user |
| **Agent model** | Single + ephemeral subagents | Single agent, multi-session | 6 persistent nous + ephemeral spawns |
| **Channels** | Terminal only | 17+ (Telegram, Discord, Signal, Slack, WhatsApp, iMessage, web, etc.) | Signal, webchat, MCP, cron |
| **Memory** | Session JSONL + file history | sqlite-vec + MMR + temporal decay + LanceDB (optional) | Mem0 + Qdrant + Neo4j + sqlite-vec + blackboard |
| **Config format** | JSON settings + Markdown files + hooks.json | JSON5 + env overrides + profiles | Zod-validated JSON + workspace Markdown |
| **Extension model** | Plugin marketplace + hooks (15+ events) | 38 extensions + plugin SDK + 5 hook types | Plugin loader + event bus (15 events) + lifecycle hooks (7 types) |
| **Context mgmt** | Compaction + file snapshots | Prompt cache stability + session file sync | Distillation pipeline (facts/decisions/summary) |
| **Sandbox** | None | Docker containers (config-hash, env sanitization, fs bridge, non-root) | None |
| **IDE integration** | Native (is a CLI agent) | ACP (Agent Code Protocol) server | None |
| **Mobile** | None | Android + iOS/Watch + macOS companion apps | Web UI (Svelte, planned) |
| **Auth** | OAuth/API key (Anthropic only) | Multi-account failover, OAuth chains, pairing per channel | Token/password/JWT/pairing (Signal challenge-code) |
| **Browser** | None | Playwright + CDP + profiles + downloads + session persistence (~90 files) | Playwright-core + browser_use (Python subprocess), 4 actions |
| **Proactivity** | None (reactive only) | Heartbeat cron + wake | Prosoche daemon, cron, foresight signals, attention scoring |
| **Self-observation** | None | None | Competence model, uncertainty tracker, calibration, interaction signals |
| **Cost control** | User selects model | Usage tracking + cost estimation per session/model | Temperature routing, dynamic tool loading, pricing engine per-turn |
| **Streaming** | Terminal native | Draft stream (live message editing in channels) | SSE to webchat; complete responses via Signal |
| **Loop detection** | None | Tool loop detection module | LoopDetector (sliding window, warn/halt, consecutive error detection) |
| **Spawn depth** | Not tracked | maxSpawnDepth=2 | depth propagated, checked against maxPingPongTurns (5) |
| **Tool restrictions** | Glob patterns per command/agent | ownerOnly + ActionGate per tool | ALL tools for persistent nous; `tools?: string[]` on ephemeral spec (unwired) |
| **Onboarding** | `claude setup` | `openclaw onboard` interactive wizard | `aletheia doctor` (validation only) |
| **Skills** | SKILL.md files, auto-load | SKILL.md + npm/git installable + 56 bundled | SKILL.md + skill learner (auto-extract from trajectories) |
| **Testing** | Internal suite | Vitest, V8 coverage (70% threshold), live + Docker E2E | Vitest, 733+ tests, 75 test files |
| **Observability** | None | TSLog structured + OpenTelemetry extension | createLogger + AsyncLocalStorage context + Langfuse |
| **Security** | Permission modes | SSRF guard, symlink prevention, non-root containers, timing-safe auth | Circuit breakers (input+response), SSRF guard, rate limiting, pairing auth |
| **Cron** | None | Cron expressions + heartbeat + active hours + session reaper | 4 cron jobs (heartbeat, consolidation, pattern extraction, adversarial test) |
| **Gateway/API** | None (CLI only) | Hono HTTP/WS, 100+ RPC methods, OpenAI-compatible API | Hono HTTP, 50+ REST endpoints, SSE streaming, MCP server |
| **Build** | Internal | tsdown, 6 entry points, ~293KB | tsdown, single entry, ~328KB |

---

## Deconflicted Gaps

Where both Claude Code and OpenClaw have a feature Aletheia lacks, the better approach is selected.

### DECONF-1: Hook System

**Claude Code:** Declarative `hooks.json` config, 15+ event types (`PreToolUse`, `PostToolUse`, `Stop`, `SessionStart`, etc.). Shell/Python handlers receive JSON via stdin, control flow via exit codes (0=allow, 2=deny). Output fields: `permissionDecision`, `additionalContext`, `decision`+`reason`.

**OpenClaw:** TypeScript `registerInternalHook(eventKey, handler)` with `type:action` naming (e.g., `command:new`, `message:received`). Plugin hooks: 5 lifecycle types. Handlers receive context objects, can transform messages or block processing.

**Aletheia now:** Internal event bus (15 events: `turn:before`, `turn:after`, `tool:called`, etc.). Plugin lifecycle hooks (7 types: `onStart`, `onShutdown`, `onBeforeTurn`, `onAfterTurn`, etc.). Neither is user-facing.

**Recommendation: Hybrid.** Keep Aletheia's event bus + plugin lifecycle as internal foundation. Add user-facing hook layer configured in `aletheia.json` that maps to the event bus. Use Claude Code's JSON stdin/stdout protocol for shell interop (proven, language-agnostic). Use OpenClaw's `noun:verb` event naming (already matches Aletheia's convention). Hook handlers: shell commands receiving JSON stdin, exit codes controlling flow.

**Why:** CC's protocol is a portable standard. OC's naming is more systematic. Aletheia's bus provides the wiring.

---

### DECONF-2: Command Definitions

**Claude Code:** `.md` files with YAML frontmatter (`description`, `argument-hint`, `allowed-tools`, `model`), `$ARGUMENTS` substitution, inline bash execution.

**OpenClaw:** Slash command parsing in gateway. Plugin-contributed commands via `registerCommand()`. Code-registered, not file-based.

**Aletheia now:** `CommandRegistry` in `semeion/commands.ts` with 15 TypeScript handlers (`!ping`, `!help`, `!status`, etc.). Skills from `SKILL.md` are knowledge-injection only.

**Recommendation: Claude Code.** Extend `CommandRegistry` to scan `shared/commands/*.md` for file-based commands with frontmatter. Aletheia already uses workspace Markdown extensively. `SKILL.md` demonstrates the file-based pattern. OpenClaw's code-registered approach requires rebuilds.

---

### DECONF-3: Tool Restrictions

**Claude Code:** `allowed-tools` with glob patterns per command/agent. `"Read"` (exact), `"Bash(git:*)"` (prefix with glob on args).

**OpenClaw:** `ownerOnly` boolean per tool. `ActionGate` for channel-specific policies. `tool-policy.ts` in sandbox context.

**Aletheia now:** ALL agents get ALL tools (user directive, absolute). `EphemeralSpec` has `tools?: string[]` but it's not wired through the pipeline.

**Recommendation: Claude Code's glob patterns, ephemeral-only.** Wire `EphemeralSpec.tools` through the pipeline's resolve stage to filter `ToolRegistry.getDefinitions()`. Support patterns: `"exec"` (exact), `"web_*"` (prefix glob), `"*"` (all). Never apply to persistent nous.

---

### DECONF-4: Plugin Layout

**Claude Code:** Standard directory layout (`commands/`, `agents/`, `skills/`, `hooks/`, `.mcp.json`). Auto-discovery. `${CLAUDE_PLUGIN_ROOT}` env var. Marketplace manifest.

**OpenClaw:** 38 extensions in `extensions/` dir, each with own `package.json` and build. Heavy plugin SDK with HTTP route registration, service lifecycle, CLI registrars.

**Aletheia now:** Plugin loader accepts `manifest.json` or `*.plugin.json`. Only 1 plugin exists (aletheia-memory).

**Recommendation: Claude Code.** Standard directory layout is right-sized. OC's 38-extension architecture is over-engineered for Aletheia's needs. Define: `manifest.json`, `tools/`, `hooks/`, `skills/`, `commands/`. Set `ALETHEIA_PLUGIN_ROOT` env var. Auto-discover components.

---

### DECONF-5: Memory Search Quality

**Claude Code:** No persistent memory. N/A.

**OpenClaw:** MMR (Maximal Marginal Relevance) re-ranking with Jaccard word overlap. Lambda parameter (0=diversity, 1=relevance, default 0.7). Temporal decay with exponential half-life (30 days default).

**Aletheia now:** Mem0 cosine similarity. Sidecar `/search_enhanced` with query rewriting. No diversity re-ranking. Consolidation daemon has Neo4j decay but not vector search decay.

**Recommendation: OpenClaw's MMR.** Port MMR as post-processing on Mem0 search results. Addresses a real problem: when multiple memories about the same topic are returned, they crowd out diverse relevant context. Clean ~120 LOC module. Temporal decay in search scoring is a separate lower-priority addition.

---

### DECONF-6: Browser Automation

**Claude Code:** No browser capability.

**OpenClaw:** Sophisticated Playwright-based system (~90 files). CDP direct access, profile management, persistent contexts, download handling with file chooser, screenshot with element targeting, auth token injection, role snapshots.

**Aletheia now:** `organon/built-in/browser.ts` — Playwright-core with 4 actions (navigate, screenshot, extract, browser_use). Max 3 concurrent pages. SSRF guard. 30s timeout. No CDP, no profiles, no downloads, no session persistence.

**Recommendation: Incremental upgrade.** OC's 90-file browser system would be a multi-week port. Aletheia's browser is functional for most tasks. Highest-value additions: (1) CDP for connecting to existing browser instances, (2) download capture, (3) cookie/session persistence between calls. Target ~200 LOC additions, not a rewrite.

---

## New Gaps from OpenClaw

Features only OpenClaw has that are worth evaluating. Not covered by Claude Code comparison.

### OC-1: Docker Sandbox (HIGH — evaluate)

**What:** Containerized agent execution. Config-hash recreation (rebuild only when config changes). Env var sanitization (strip secrets from container). Filesystem bridging (mount specific paths, controlled read/write). Non-root hardening.

**Why it matters:** Tools like `exec` and `browser_use` run with full `syn` user permissions. A hallucinated `rm -rf` could damage the system. Docker isolates tool execution. Critical for ephemeral spawns running untrusted tasks.

**Approach:** Start with sandboxing `exec` tool only. Docker image with minimal runtime. Mount workspace read-only except ephemeral workspace. Sanitize env vars. ~300 LOC for sandbox runner + Dockerfile.

**Evaluation needed:** Performance overhead of Docker exec per tool call? Which tools need sandboxing (exec only? browser too?)? Docker availability on worker-node?

---

### OC-2: ACP / IDE Integration (MEDIUM — evaluate)

**What:** Agent Code Protocol server bridging IDE extensions (VS Code, JetBrains) to the gateway. Session management with idle reaping, rate limiting, prompt prefix preservation, ndjson stream transport.

**Why it matters:** Would allow interaction with Aletheia agents from within an editor. ACP connects via WebSocket to the existing gateway — additive, not replacing Signal/webchat.

**Approach:** Aletheia already has gateway + WebSocket infrastructure. ACP adapter translates protocol messages to/from the existing session model. ~200-300 LOC for adapter layer.

---

### OC-3: Auth Profile Rotation (LOW)

**What:** Multi-account failover per provider. Cooldown tracking on failures. Exponential backoff before retry. OAuth refresh chains. Last-used preference tracking.

**Why it matters:** Aletheia uses a single Anthropic credential. If it's rate-limited, everything stops. A second credential as failover is straightforward.

**Approach:** `ModelSpec` already has `primary` + `fallbacks`. Extend `hermeneus/anthropic.ts` to try fallback credentials on 429/5xx. Track cooldown per credential. ~100 LOC.

---

### OC-4: Spawn Depth Enforcement (HIGH — trivial)

**What:** `maxSpawnDepth` config. When spawn creates sub-spawn, depth is tracked. Exceeding limit rejects the spawn.

**Why it matters:** `sessions_spawn` propagates `depth: (context.depth ?? 0) + 1`. `maxPingPongTurns` (5) limits back-and-forth but not spawn chains. A recursive A→B→C→D→... could exhaust resources.

**Approach:** Add `maxSpawnDepth` to config (default 3). Check at top of `sessions_spawn` handler. ~15 LOC.

---

### OC-5: Stream Preview / Typing Indicators (MEDIUM)

**What:** As LLM generates tokens, channel message is progressively edited to show partial content. Throttled at 1s intervals. Min-chars debounce to avoid notification spam.

**Why it matters:** Signal doesn't support message editing, so webchat-only for true streaming. But Signal users wait with no feedback during long turns. Even a status indicator helps.

**Approach:** Webchat: SSE streaming already works. Signal: send brief placeholder ("Thinking...") via typing indicator or placeholder message, replace with final response. ~50-100 LOC.

---

### OC-6: Onboarding Wizard (LOW)

**What:** `openclaw onboard` — interactive CLI wizard. Guides through API key, channel config, gateway setup. Writes config.

**Why it matters:** `aletheia doctor` validates but doesn't create config. First-time setup requires manually writing `aletheia.json`.

**Approach:** `aletheia init` subcommand. Prompts for: API key, gateway port, Signal account, agent list. Writes template config. ~150 LOC.

---

### OC-7: Config Doctor with Repair (LOW)

**What:** Detects permission issues, symlink traversal, missing directories, and repairs them. Creates backups before modifying.

**Why it matters:** The ACL bug on 2026-02-13 required manual `setfacl`. Doctor could have auto-fixed it. Adding `--fix` flag is high value relative to effort.

**Approach:** Extend doctor to emit `{check, ok, fixable, action}` tuples. With `--fix`, execute repair actions (chmod, setfacl, mkdir, remove broken symlinks). ~100 LOC.

---

### OC-8: Prompt Cache Stability (MEDIUM)

**What:** System prompt assembly ensures deterministic ordering. Per-message metadata (IDs, timestamps) moved to dynamic context blocks that don't invalidate the Anthropic prompt cache.

**Why it matters:** Aletheia's bootstrap has `static`, `semi-static`, `dynamic` cache groups with `cache_control: { type: "ephemeral" }` breakpoints. But `bootstrap-diff.ts` may change cache-stable blocks when semi-static content updates, wasting cache hits.

**Approach:** Audit bootstrap assembly. Ensure `contentHash` uses correct granularity. Move per-turn data out of `semi-static` into `dynamic`. ~50 LOC.

---

### OC-9: Plugin Path Safety (LOW)

**What:** `realpath()` check prevents symlink traversal. Env var sanitization for plugin execution.

**Why it matters:** `prostheke/loader.ts` doesn't validate for symlink traversal. Only 1 plugin exists, but third-party plugins would make this a real vector.

**Approach:** Add `realpath()` check to loader. Ensure loaded paths don't escape plugin root. ~30 LOC.

---

### OC-10: Announcement Idempotency (LOW)

**What:** UUID-based dedup on broadcast chains. Prevents duplicate delivery on retry.

**Why it matters:** `sessions_send` has no dedup. If Syn sends the same message twice (retry, heartbeat race), the target processes it twice.

**Approach:** Hash message content + target + timestamp window. Check `cross_agent_calls` for recent duplicates (30s). ~30 LOC.

---

### OC-11: Android Companion App (EVALUATE)

**What:** Kotlin/Gradle native Android app. OpenClaw's a2ui renderer for mobile canvas.

**Why it matters:** Could serve as a native port of Aletheia's Svelte web UI for mobile. Better than browser-based access for notifications, offline, and native gestures.

**Evaluation needed:** Can the a2ui architecture wrap a Svelte webview? Or is a standalone Kotlin app wrapping the webchat API more practical? PWA as alternative?

---

## What Aletheia Has That Neither Has

### Cross-Agent Memory System
Neither Claude Code nor OpenClaw has cross-agent semantic memory with graph relationships. OpenClaw has embeddings + MMR but no graph, no entity resolution, no community detection (PageRank/Louvain), no autonomous link generation (A-Mem pattern). Claude Code has session JSONL only.

### Proactive Attention (Prosoche)
Neither has anything comparable. OpenClaw has heartbeat cron and wake triggers but no attention scoring, no foresight signals, no activity prediction model, no dynamic workspace injection.

### Self-Observation & Calibration
Neither has competence models, uncertainty tracking (Brier score, ECE), interaction signal classification, or mid-session eval feedback injection. Unique to Aletheia.

### Multi-Phase Distillation
Claude Code has simple compaction. OpenClaw has session file sync. Neither extracts structured facts/decisions/open items from conversation history. Neither has similarity pruning on distillation output.

### Persistent Named Agents
Claude Code has ephemeral subagents. OpenClaw has a single agent with multiple sessions. Neither has persistent agents with individual identities, workspaces, memory, and domain expertise that persist across sessions.

### Skill Learning from Trajectories
Claude Code loads SKILL.md files. OpenClaw installs skills from npm/git. Neither auto-extracts skills from successful multi-tool-call trajectories (Aletheia's `skill-learner.ts` via Haiku, rate-limited 1/hr/agent).

### Cross-Agent Blackboard
SQLite-based shared state with TTL expiry. Neither Claude Code nor OpenClaw has a cross-agent coordination primitive.

### Circuit Breakers
Input quality + response quality circuit breakers preventing harmful prompts from reaching the LLM and catching low-quality responses. Neither has this.

### Reversibility Tagging
Tools tagged as reversible/irreversible for informed approval decisions. Neither has this.

---

## Canonical Prioritized Feature Set

### Tier 1: HIGH Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-1 | Spawn depth limits | ~15 | OpenClaw | None | Add `maxSpawnDepth` to config schema, check in `sessions_spawn` handler |
| F-2 | User-facing hooks | ~400 | Hybrid CC+OC | None | New `koina/hooks.ts`, JSON stdin/stdout protocol, `noun:verb` event names, config in `aletheia.json` hooks section |
| F-3 | Hot-reload config | ~150 | Claude Code | None | New `taxis/watcher.ts`, fs.watch on aletheia.json, diff + swap safe fields, emit `config:reload` event |
| F-4 | Docker sandbox for exec | ~300 | OpenClaw | Docker on server | Sandbox runner for `exec` tool, env sanitization, workspace mounting, Dockerfile. Evaluation item E-1 first. |
| F-5 | Wire loop detection into guard | ~30 | OpenClaw | None | LoopDetector exists but guard stage only checks circuit breakers. Wire depth check into guard.ts |

### Tier 2: MEDIUM Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-6 | MMR diversity re-ranking | ~120 | OpenClaw | None | Port MMR algorithm to sidecar `/search` or apply in `mem0-search` tool as post-processing |
| F-7 | Markdown command definitions | ~200 | Claude Code | None | Extend `CommandRegistry` to scan `shared/commands/*.md`, parse YAML frontmatter, `$ARGUMENTS` substitution |
| F-8 | Tool restrictions for ephemeral spawns | ~100 | Hybrid CC+OC | None | Wire `EphemeralSpec.tools` through pipeline resolve. Glob patterns: `"exec"`, `"web_*"`, `"*"` |
| F-9 | Prompt cache stability audit | ~50 | OpenClaw | None | Audit `bootstrap.ts` + `bootstrap-diff.ts`. Move per-turn data from `semi-static` to `dynamic` |
| F-10 | Stream preview / typing indicators | ~100 | OpenClaw | None | Signal: placeholder message. Webchat: already SSE. Signal typing indicator if supported. |
| F-11 | ACP / IDE integration | ~300 | OpenClaw | F-2 | ACP adapter translating protocol to gateway sessions. WebSocket bridge. |
| F-12 | Doctor with --fix | ~100 | OpenClaw | None | Extend doctor to emit fixable actions. Execute with `--fix` (chmod, setfacl, mkdir, fix symlinks) |

### Tier 3: LOW Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-13 | Auth credential failover | ~100 | OpenClaw | None | Try fallback credentials on 429/5xx. Cooldown tracking per credential. |
| F-14 | Plugin standard layout | ~100 | Claude Code | None | Standard dirs in plugin root. Auto-discover. `ALETHEIA_PLUGIN_ROOT` env var. |
| F-15 | Plugin path safety | ~30 | OpenClaw | None | `realpath()` validation in `prostheke/loader.ts` |
| F-16 | Onboarding wizard | ~150 | OpenClaw | None | `aletheia init` command with prompts. Write template config. |
| F-17 | Self-referential loop pattern | ~50 | Claude Code | F-2 | Stop hook + state file, or `loop_until` built-in tool |
| F-18 | Announcement idempotency | ~30 | OpenClaw | None | Content hash dedup in `cross_agent_calls` (30s window) |
| F-19 | Parallel validation pattern | ~50 | Claude Code | None | Document as skill (`shared/skills/parallel-review/SKILL.md`), not infrastructure |
| F-20 | Temporal decay in search scoring | ~80 | OpenClaw | None | Exponential decay on search scores based on memory age |

### Evaluation Items (research before committing)

| ID | Question | Source | What to determine |
|----|----------|--------|-------------------|
| E-1 | Docker sandbox scope | OpenClaw | Which tools need sandboxing? Performance cost per tool call? Docker available on worker-node? |
| E-2 | Android app as webui port | OpenClaw | Wrap Svelte webview in Kotlin? PWA alternative? a2ui architecture portability? |
| E-3 | Browser automation upgrade | OpenClaw | CDP for existing browsers? Download handling? Session persistence? LOC estimate? |
| E-4 | Skill installation from npm/git | OpenClaw | Does skill learner + SKILL.md cover enough? Worth the attack surface? |

### Recommended Implementation Order

1. **F-1** (spawn depth) — 15 min, prevents crash scenario
2. **F-5** (wire loop detection) — 15 min, complements F-1
3. **F-3** (hot-reload config) — eliminates restart friction
4. **F-2** (user-facing hooks) — foundational, enables F-11 and F-17
5. **F-7** (markdown commands) — extensible commands without rebuilds
6. **F-8** (tool restrictions) — safer ephemeral spawns
7. **F-6** (MMR) — immediate memory search quality improvement
8. **F-9** (cache stability) — token cost reduction
9. **F-12** (doctor --fix) — operational quality of life
10. **F-10** (stream preview) — UX for long-running tasks

Items F-4 (Docker), F-11 (ACP), and E-1 through E-4 require evaluation before implementation commitment.

**Total estimated new code:** ~1,825 LOC across Tiers 1-2 (the actionable tiers). Tier 3 adds ~590 LOC.

---

## Cross-Reference with Existing Specs

| Feature | Overlapping Spec | Relationship |
|---------|-----------------|--------------|
| F-3 (hot-reload) | IMPROVEMENTS.md | Direct overlap — implement per that design |
| F-4 (Docker sandbox) | Spec 13 (Sub-Agent Workforce) | Sandbox enhances ephemeral agent safety |
| F-6 (MMR) | Spec 07 (Knowledge Graph) | MMR applies to vector search, not graph queries |
| F-8 (tool restrictions) | Spec 13 (Sub-Agent Workforce) | Spec 13 proposes role-specific tool sets — F-8 provides the mechanism |
| F-9 (cache stability) | Spec 16 (Efficiency) | Cache stability is part of token economy optimization |
| F-10 (stream preview) | Spec 15 (UI Interaction Quality) | Stream preview is webchat UX improvement |
| F-11 (ACP) | None | New capability |

---

## Verification

- Cross-reference each gap against OpenClaw source in `openclaw-ref/src/`
- Cross-reference each gap against Claude Code source in `claude-code/`
- Cross-reference each Aletheia capability against runtime source in `infrastructure/runtime/src/`
- Validate no canonical feature violates user directives (ALL tools for persistent nous, Signal+webchat only)
- Validate against IMPROVEMENTS.md for overlap

### Key Implementation Files

| File | Relevant Features |
|------|-------------------|
| `src/nous/pipeline/stages/guard.ts` | F-1 (spawn depth), F-5 (loop wiring) |
| `src/koina/event-bus.ts` | F-2 (hook foundation) |
| `src/semeion/commands.ts` | F-7 (markdown commands) |
| `src/taxis/schema.ts` | F-1, F-2, F-3 (config schema additions) |
| `src/taxis/loader.ts` | F-3 (hot-reload) |
| `src/organon/registry.ts` | F-8 (tool filtering for ephemeral) |
| `src/organon/built-in/sessions-spawn.ts` | F-1 (depth check), F-8 (tool passthrough) |
| `src/organon/built-in/browser.ts` | E-3 (browser upgrade) |
| `src/hermeneus/anthropic.ts` | F-13 (auth failover) |
| `src/prostheke/loader.ts` | F-14 (standard layout), F-15 (path safety) |
| `src/nous/bootstrap.ts` | F-9 (cache stability audit) |
| `entry.ts` | F-12 (doctor --fix), F-16 (onboarding) |
| `infrastructure/memory/sidecar/` | F-6 (MMR), F-20 (temporal decay) |
