# Specification Archive — Decisions & Patterns

Consolidated reference for 33 implemented specs. Organized by domain, preserving key decisions, rejected alternatives, and patterns that constrain future work. Code is the source of truth — this document captures the *why*.

> **On the future of specs:** Specs are a transitional artifact. They exist because Aletheia's planning system (Dianoia) wasn't mature enough to own the design process when development started. As Dianoia grows — persistent projects, requirements scoping, phase execution, verification — new work should flow through Dianoia projects rather than spec documents. Specs that remain will be architectural constraints and principles (this file), not implementation plans. The goal is: Dianoia proposes → human approves → Dianoia executes → Dianoia verifies. Specs become unnecessary when that loop closes.

---

## Foundation

### Modular Runtime Architecture (PR #21)

Decomposed monolithic `manager.ts` (1,446 lines) into a 6-stage composable pipeline: resolve → guard → context → history → execute → finalize.

- **4-layer config resolution:** hardcoded fallback → `agents.defaults.pipeline.*` → `agents.list[id].pipeline.*` → `nous/<id>/config-overrides.yaml` (machine-written overlay)
- **Stage ordering is static** — users cannot reorder or inject custom stages; behavioral extensions go through `prostheke` plugin system
- **`buildMessages` is a utility, not a stage** — structural plumbing, not configurable behavior
- **Tool loop not decomposed** into sub-pipeline (over-engineering)
- **Hot reload via SIGUSR1 only** — no HTTP config endpoint until auth existed
- **`RuntimeServices` bundle** replaces six `setX()` setter methods
- Model swap during hot reload can trigger unexpected distillation if new context window is smaller
- `config_write` tool lets agents modify only their own `pipeline.*` — writes to overlay file, not main config

### Code Quality & Standards (PRs #37, #45, #52, #60, #62)

Established `AletheiaError` hierarchy, `trySafe`/`trySafeAsync` boundary helpers, dead code removal, and `CONTRIBUTING.md` standards.

- **Error subclasses:** `PipelineError`, `StoreError`, `ToolError`, `TransportError`, `ConfigError` — namespaced codes like `PIPELINE_STAGE_FAILED`
- **`trySafe`/`trySafeAsync`** for non-critical ops (skill learning, interaction signals) — makes "optional and must not crash" intent explicit. Rejected silent empty catches.
- **Self-documenting code:** one header comment per file, zero "what" comments, inline only for non-obvious "why"
- **Import order:** node builtins → external packages → internal absolute → local relative
- **Event naming:** `noun:verb` (e.g., `turn:before`, `tool:called`)
- Dead code included entire scaffolded-but-never-integrated modules (parts of `auth/`)

### Development Workflow (PRs #71, #79, #86)

Branch conventions, commit standards, PR workflow, CI policy, versioning.

- **All git authorship as Cody Kickertz** — no `Co-authored-by: Claude` (rejected agent attribution for public repo credibility)
- **Always squash merge** — rejected mixed strategies
- **Agents never run full test suite locally** — CI is the authority
- **Zero broken windows:** pre-existing test failures must be fixed or deleted, never ignored. `test.skip()` requires a linked GitHub issue.
- **Versioning:** `0.<major>.<minor>` pre-1.0 where major = capability milestone. Intentional releases at milestones, not per-commit.
- **Commit format:** `<type>: <description>` with optional body and `Spec: NN` footer
- Local validation = typecheck + lint only

---

## Turn Pipeline & Safety

### Turn Safety (PRs #38, #39)

Pipeline error boundaries wrapping each stage, distillation guards, orphan diagnostics.

- **Pipeline always returns `TurnOutcome`** (with optional `error` field) instead of throwing through the lock chain
- **Distillation deferred via `scheduleDistillation`** to run after lock release, not inline during finalize — finalize already holds the session lock
- **`.then(fn, fn)` lock pattern** intentionally runs the next turn even after failures (deadlock prevention)
- **Every turn must produce a visible outcome** — silence is never acceptable
- `identifyFailedStage()` uses state markers (systemPrompt, messages, outcome) to name which stage failed

### Tool Call Governance (PR #22)

Framework-level timeouts, turn cancellation, abort propagation.

- **Defense in depth:** framework timeouts are safety net; tool-level timeouts remain primary
- **Cancellation is cooperative** — `AbortSignal` propagated; tools opt-in to checking it
- **Framework timeout doesn't kill the underlying operation** — prevents turn blocking, but tool may leave zombie process
- **`exec` is always-sequential** because commands can affect each other
- "Stop generation" button previously only closed SSE — server-side turn continued. Fixed with `POST /api/turns/:id/abort`.
- Aborting mid-turn can leave orphaned `tool_use` blocks without `tool_results` — existing orphan repair handles this

### Cost-Aware Orchestration (PRs #59, #89, #99)

Mid-turn message queue, sub-agent infrastructure, plan mode, complexity-based model routing, token budgets.

- **Sub-agents get no conversation history** — only task description + relevant files (rejected giving session history: cost + context pollution)
- **Sub-agents cannot spawn further sub-agents** (no recursion)
- **Model routing is per-turn, not per-session** — a conversation can route between Opus/Sonnet/Haiku across turns
- **Identity stays consistent regardless of model tier** — Haiku-routed turns still respond as the named agent
- **Sub-agents are runtime-configured** (aletheia.json) not file-based (rejected Claude Code's YAML frontmatter approach)
- Message queue lives on session, not transport — both Signal and webchat can queue into the same turn
- Plan mode uses `awaiting_approval` turn state, rendered inline (not modal)
- Conservative estimate: 40-60% of Opus tokens can run on Haiku/Sonnet

### Efficiency (PRs #75, #94)

Parallel tool execution, token audit, truncation, dynamic thinking budget, hot-reload config.

- **Three-tier tool parallelism:** `always` (read-only), `never` (exec, message), `conditional` (write/edit — safe if different file paths). Rejected all-or-nothing.
- **`Promise.allSettled`** for parallel batches; batches themselves sequential (ordering between write groups matters)
- **Tool result truncation at storage time, not return time** — model sees full result for current turn, future turns see truncated (70% head, rest tail)
- **Dynamic thinking budget:** 2K for short messages, 6K-16K for complex. 30% reduction on tool-loop iterations 2+.
- **Hot-reload via file watcher** — agent additions, model changes, bindings all live without restart
- Approval gates break parallel batches — if any tool needs approval, extract it and run separately
- Bootstrap cache invalidation: static blocks must stay stable across turns for Anthropic prompt cache hits

---

## Memory & Distillation

### Memory Continuity (PRs #36, #43, #44, #55)

Five-tier memory system to survive distillation without losing context.

- **Multiple representations at different abstraction levels** instead of a single summary — rejected single-pass narrative blob
  1. Expanded preserved tail (10 messages / 12K tokens, up from 4)
  2. Structured multi-section distillation summary
  3. Context editing API (`clear_tool_uses_20250919`) clears old tool results at 60% context
  4. Auto-maintained working state scratchpad (Haiku per-turn, ~500 tokens)
  5. Agent `note` tool for "sticky notes" always injected into system prompt
- **Context editing at 60%, distillation at 70%** — tool result clearing extends useful conversation length
- **Working state maintained by Haiku** (cheap) not Opus — rejected making the main model responsible
- **Agent notes are always-present** in system prompt vs. Mem0 memories which are vector-retrieved — different retrieval guarantees
- Working state structure: `currentTask`, `taskChain`, `completedSteps`, `openFiles`, `recentDecisions`, `blockers`
- Summary format: Task Context, Completed Work, Key Decisions & Rationale, Current State, Open Threads, Corrections & Failed Approaches, Tone & Register
- Tool results are the biggest context consumers (single `exec` can be 2-5K tokens)

### Session Continuity (PRs #53, #85)

Session classification, multi-signal distillation triggers, receipts, ephemeral sessions.

- **Three session types with different lifecycles:** `primary` (permanent, IS the agent's identity), `background` (stripped-down distillation), `ephemeral` (never distilled, deleted after 24h). Rejected treating all sessions equally.
- **Multi-signal distillation triggers** over single `last_input_tokens >= 140K` — message count catches many-small-message sessions
- **Compute context size directly** before each turn rather than relying on stale `last_input_tokens` field
- **Background sessions:** last 20 messages only, no fact extraction
- `distillation_log` table for full audit trail
- Recency boost: linear decay over 24h, max +0.15 score
- Context utilization bar in header (green/yellow/orange/red at 60/80/90%)
- Post-distillation priming: inject extracted facts into next turn regardless of vector similarity

### Distillation Persistence (hooks)

Workspace flush writing distillation summaries + facts to daily markdown files.

- **Synchronous file I/O** (`appendFileSync`) — sub-millisecond, avoids race conditions. Rejected async writes.
- **Non-blocking:** file write failures never fail the distillation pipeline
- **Capped at 20 facts per section** in workspace files (full extraction goes to Mem0)
- **Unconditional** — not gated behind config; activates whenever agent has a workspace
- Append-only daily files: `{workspace}/memory/YYYY-MM-DD.md` with `## Distillation #N - HH:MM` sections
- Discovered: a session had 1,916 messages across 13 distillation cycles with zero workspace persistence — data was in DB but invisible to agents on boot

### Memory Pipeline (PR #83)

End-to-end memory extraction, quality, and retrieval fixes.

- **Bypass Mem0's extraction entirely** — use our own prompt and store directly in Qdrant via `/add_direct` (rejected letting Mem0 re-extract: latency, cost, noise)
- **Vector search is the primary path;** Neo4j is supplementary (don't block turns on graph queries)
- **Semantic dedup at cosine > 0.90** to prevent duplicate memories
- **"Less is more"** — 200 high-quality memories beat 2,000 noisy ones
- `source` metadata on memories: `distillation`, `reflection`, `turn`
- Hard filters for noise patterns (`/^(Uses|Familiar with|Works with)/i`)
- After-turn extraction is async and non-blocking (<200ms added latency)
- Pre-audit: 13% of Qdrant was noise, 81% of Neo4j relationships were generic `RELATES_TO`
- Open question: whether to rip out Neo4j entirely vs. fix it

### Knowledge Graph (PRs #61, #85, #86)

Vector recall optimization, Neo4j degradation, domain-scoped memory.

- **Vector-only search as primary** path, graph enrichment as async fire-and-forget — rejected blocking on Neo4j graph traversals
- **Neo4j made optional** with graceful degradation rather than replaced with SQLite — kept for future use
- **Domain-scoped memory** (`personal`, `craft`, `work`, `system`, etc.) with explicit cross-domain queries allowed
- Extraction prompt tuned per conversation type: tool-heavy turns extract conclusions not invocations
- MMR diversity re-ranking (Jaccard overlap penalty). Confidence boosts on access, decays over time.
- Recall frequently hit the 3-second timeout due to Neo4j + Qdrant compound latency
- Semantic drift causes cross-domain bleed (e.g., "tools" matching both leatherwork and vehicle contexts)

### Sleep-Time Compute (PR #80)

Nightly and weekly reflection pipelines during idle time.

- **Confidence gating per category:** preferences require HIGH only (higher bar); patterns/contradictions accept HIGH+MEDIUM
- **Weekly reflection operates on distillation summaries,** not raw messages (cheaper, more signal)
- **Built-in cron commands** (`reflection:nightly`, `reflection:weekly`) rather than external scheduling
- Findings tagged with `[reflection:category]` source prefix
- `reflection_log` table (migration v15) records every run
- Cost: ~$1/day total across all agents on Haiku
- Chunking required for large message sets (split by token budget, reflected per-chunk, merged with dedup)

---

## Agents

### Sub-Agent Workforce (PRs #72, #86)

Five typed roles with structured results, parallel dispatch, budget controls.

- **Sub-agents are disposable workers, NOT peers** like named agents — rejected using Demiurge/Syl for mechanical tasks
- **3-tool-call rule:** if <3 tool calls needed, handle directly (delegation overhead not worth it)
- **Roles:** Coder (Sonnet), Reviewer (Sonnet), Researcher (Sonnet), Explorer (Haiku), Runner (Haiku) — match cost to complexity
- **Structured JSON result contract** (`SubAgentResult`), not free-form text
- **QA step before integrating results** — confidence-based verification, rejected blind integration
- Recursive spawn guard (depth limit) prevents infinite delegation chains
- 15x cheaper than direct Opus execution for equivalent work
- Context efficiency: 40K tokens direct vs. 1K tokens via sub-agent summaries

### Recursive Self-Improvement (PRs #106, #107, #128)

Self-evaluation, competence routing, memory curation, pipeline config, code patching, evolutionary config search.

- **Three-layer safety architecture:**
  - Frozen: model weights, circuit breakers, core infra
  - Test-gated: code patches, tool authoring, config changes
  - Self-modifiable: workspace files, notes, memory
- **Code patches scope-restricted:** allowed in `organon/`, `nous/`, `distillation/`, `daemon/`; forbidden in `pylon/`, `koina/`, `semeion/`, `taxis/`
- **Cross-agent review** for code patches (no agent reviews its own)
- **Rate limiting as economic brake:** 1 patch/hour/agent, 3 patches/day total
- **Evolutionary config changes require human approval** via Signal (or 24h timeout + >10% improvement)
- Process restart (not hot module swap) after successful patches
- Sakana AI Scientist cautionary tale: modified its own timeout instead of optimizing code

### Plug-and-Play Onboarding (PRs #137, #138)

Conversational agent self-construction.

- **Agent builds itself through conversation** — first session gets a special system prompt guiding it to interview the operator and write its own SOUL.md, USER.md, MEMORY.md. Rejected form-based setup and template-fill.
- **Non-negotiable defaults** baked in (verification, output discipline, self-correction, coding standards) — operator doesn't choose these
- **Operator chooses:** identity, communication preferences, uncertainty handling, correction style
- Onboarding prompt injection only on first session; transitions naturally
- Target: git clone to calibrated agent in under 10 minutes

### Agent Portability (PRs #100, #124, #128)

Export/import, scheduled backups, checkpoint time-travel.

- **`aletheia export <nous-id>`** produces single JSON with config, workspace, Qdrant vectors, Neo4j graph, session history, receipts
- **Embeddings optional in export** (can re-embed on import) — keeps file size manageable
- **Session history truncated** to last distillation + tail (not full history)
- **Memory dedup on import** at 0.92 cosine similarity threshold
- **Time-travel:** fork a session from any distillation point; memories created after checkpoint are NOT removed (may be valid independently)
- `AgentFile` schema versioned (`version: 1`). Import generates new IDs to avoid collisions.

---

## Security & Auth

### Auth & Security Foundation (PR #26)

JWT session auth, RBAC, TLS, audit logging.

- **Hand-rolled JWT** via `node:crypto` HMAC-SHA256 — sufficient for single-server self-hosted (rejected JWT library: unnecessary surface area; rejected RSA/ECDSA: no multi-service verification)
- **Access tokens in memory only** (not localStorage — XSS-safe); refresh tokens in httpOnly cookies
- **Multi-mode auth middleware** supports token, password, and session simultaneously
- Route permissions in single `ROUTE_PERMISSIONS` map
- No OAuth2/OIDC (overkill for self-hosted)
- SSE connections can't use custom headers with `EventSource` — previously leaked token in URL query params

### Auth & Updates (PRs #50, #70, #99, #126)

Session-based login, self-update system, update daemon.

- **Access token in memory, refresh token as httpOnly SameSite=Strict cookie** — avoids both XSS and CSRF
- **`mode: "session"` is new default;** `mode: "token"` preserved for backward compat and API access
- **Update script is standalone bash** (not part of runtime) — survives runtime being stopped
- **Conditional `npm ci`** — only runs if `package-lock.json` changed between versions
- `GET /api/auth/mode` (public, no auth) lets UI detect auth type
- If `mode: "session"` but no users configured, gateway enters "setup required" mode
- Scrypt password hashing. Refresh token rotation on use.

### Security Hardening (PRs #99, #106, #124)

PII detection, Docker sandbox, tamper-evident audit, encrypted memory.

- **PII detection** (phone, email, SSN, credit cards, API keys) with mask/hash/warn modes on three surfaces: memory storage, Signal outbound, LLM context
- **PII mode `mask` as default** — rejected `hash` (preserves referential equality but less intuitive)
- **Sandbox is opt-in per-agent,** default for sub-agent spawns, bypass for persistent nous (rejected mandatory sandboxing: breaks git/systemctl)
- **Audit hash chain** uses `previous_checksum` linking (rejected merkle tree as overkill)
- **Memory encryption:** AES-256-GCM with envelope encryption (DEK + master key). Vectors not encrypted (already lossy representations), only metadata/text.
- Encryption does NOT protect against a compromised runtime (key in memory)
- PII detector false positive risk on technical content (UUIDs, hex strings, IP addresses)

### Data Privacy (PR #33)

File permissions, SQLCipher, retention policy, encrypted export.

- **SQLCipher via `better-sqlite3-multiple-ciphers`** drop-in replacement (rejected column-level encryption: overkill for single-user)
- **LUKS full-disk encryption preferred** over per-directory eCryptfs (simpler, protects everything)
- **Key source options:** env var / keyfile / Linux kernel keyring (rejected HSM/TPM as overkill)
- **Local embeddings option** (`fastembed` / GTE-large) to keep all memory processing on-device
- Three sensitivity tiers: Critical/Sensitive/Operational
- `aletheia forget "topic"` right-to-erasure pattern
- sessions.db was world-readable (644). Neo4j password hardcoded in docker-compose.yml. Memory sidecar had zero auth on port 8230. SQLCipher adds ~5-15% overhead — negligible.

---

## UI & Interaction

### Webchat UX (PR #47)

Global SSE notifications, refresh resilience, file editor.

- **CodeMirror 6 over Monaco** — ~150KB vs ~2MB, right balance for the use case
- **Turn lifecycle is server-owned, not client-owned** — client disconnecting detaches observation but does not cancel the turn
- **30-second history poll fallback** when SSE disconnects
- Removed `needsTextSeparator` entirely — let markdown renderer handle paragraph spacing
- Split pane layout with draggable divider, width persisted to localStorage
- `safeWorkspacePath` for path traversal protection on file save

### Thinking UI (PRs #40, #54, #63)

Extended thinking pills, live streaming summaries.

- **Extended thinking enabled only for Opus** — rejected Haiku/Sonnet (adds latency/cost without sufficient value)
- **Summary extraction is pure client-side** — last complete sentence from tail, no LLM call
- **Thinking pill reuses tool detail panel interaction** — rejected separate UI paradigm
- **Thinking collapsed by default** on completed messages, conclusion text primary
- Amber accent for thinking, blue for tools. 10K token thinking budget default.
- Thinking budget adds latency/cost even on simple turns

### Chat Output Quality (PR #86)

Narration suppression, formatting standards.

- **Narration reclassified to thinking pane, NOT deleted** — rejected content filtering
- **Every sentence** in the response passes through `isNarration()` (6 regex patterns)
- Min length 10 chars to prevent false positives; sentences >200 chars pass through as likely substantive
- ~0.5ms per chunk overhead
- Mixed sentences ("The file has 200 lines, let me check the first 50") pass through as substantive
- The filter can only catch patterns it knows — prompt regression can reintroduce unrecognized narration

### UI Interaction Quality (PRs #54, #72, #86)

Thinking persistence, tool input display, categorization.

- **Tool input is the most useful information,** not output — `getInputSummary()` extracts primary param per tool type
- **Tool categorization is a view-mode change,** not data transformation — underlying data stays sequential
- **6 tool categories:** filesystem, search, execute, memory, communication, system
- Thinking panel persistence: `$effect` watching `thinkingIsLive` transition, captures final text before store clears it
- `tool_start` SSE event includes `input` field. Tool inputs from history parsed from stored `tool_use` content blocks (different code path than live streaming).

### Integrated IDE (Spec 25, PR #307)

Lightweight file editor embedded in the web UI so humans and agents work on the same files without context-switching.

- **Not an IDE replacement** — no LSP, no debugger, no terminal emulator. Goal: "good enough to not tab away" for reviewing agent work and quick edits.
- **Multi-tab with stale detection** — `EditorTab` state tracks `dirty` (local changes) and `stale` (agent modified externally). Tab switch preserves editor state via `Map<string, string>` cache.
- **Agent edit notifications via SSE** — `tool_start` events for `write`/`edit` tools captured, matched to `tool_result` by `toolId`, toast shown: "Syn edited `config.ts`" with [Open] action.
- **Conflict resolution: reload or keep** — when tab is both dirty and stale (human and agent both edited), prompt offers two choices. No CRDT, no OT. Last-write-wins at API level.
- **CodeMirror 6 with full language support** — js/ts/tsx/jsx/py/json/yaml/md/css/html/svelte. Existing `@codemirror/lang-*` packages.
- **Path traversal protection** — `safeWorkspacePath()` rejects `..` escapes on all workspace endpoints.
- **Clickable file paths in chat** — regex post-processing detects paths in agent messages, makes them clickable to open in editor.
- **Workspace search API** — `GET /api/workspace/search` using ripgrep subprocess, results replace tree temporarily.
- **Auto-commit on agent writes** — `commitWorkspaceChange()` in `workspace-git.ts` fires on write/edit tool calls.
- **Deferred:** TreeContextMenu.svelte (right-click create/delete/rename) and inline rename — backend APIs exist (`DELETE /api/workspace/file`, `POST /api/workspace/file/move`) but UI was never built. See #325.
- 1MB hard file size limit. Files >500KB get a warning banner. Single-server, no collaborative editing.

### Graph Visualization (PRs #56, #90, #91)

2D/3D graph, node cards, communities, search, memory auditing, drift detection.

- **2D as default, 3D opt-in** — 3D adds zero information, just cosmetics
- **Lazy-load Three.js** (~150KB) only when user toggles 3D
- **Progressive loading:** top 20 nodes by pagerank immediate, 80 background, rest on-demand (rejected full upfront fetch)
- **Memory fetch per-node on click** from Qdrant, not pre-loaded
- Performance budget: <1s first render, 60fps at 200 nodes in 2D, <30KB 2D bundle
- Node opacity/saturation maps to confidence score. Stated vs. inferred memories visually distinct (solid vs. dashed border).

---

## Extensibility

### Extensibility System (PRs #98, #107, #124)

Hooks, custom commands, plugin auto-discovery.

- **Shell handler protocol matches Claude Code's hook system** (JSON on stdin, exit codes 0/1/2) for ecosystem compatibility
- **Hook `failAction` options:** `warn | block | silent` — hooks don't block by default
- **Commands are markdown files** with YAML frontmatter + `$ARGUMENTS` substitution (rejected code-based definitions)
- **Plugins namespaced** to prevent collisions
- **Plugin path safety** requires `realpath()` validation — all paths must resolve within plugin root, no `..` after resolution
- Hooks: `shared/hooks/*.yaml`. Commands: `shared/commands/*.md`. Plugin layout: `manifest.yaml` + `hooks/` + `commands/` + `tools/`.
- Loop guard implemented as a built-in hook template, not core logic


---

## Platform

### TUI — Terminal Client (Spec 28)

Ratatui-based terminal interface with Elm Architecture (TEA) event loop, SSE streaming, and agent switching.

- **One agent, one conversation** — no multiplexed views. Switch agents like switching tabs, not windows.
- **Elm Architecture (TEA)** — `Model → Msg → Update → View` cycle. All state transitions through message dispatch, no side effects in view.
- **SSE for real-time** — `GET /api/events` for system events (health, agent status), `POST /api/sessions/stream` for turn responses. Reconnect with exponential backoff.
- **Crossterm 0.29** — resolved input handling differences vs 0.28. Custom input widget over `tui-textarea` for Ctrl+Enter multiline and Emacs keybindings.
- **Custom markdown renderer** — `pulldown-cmark` → ratatui `Line/Span` conversion. Rejected `tui-markdown` (unmaintained, limited formatting).
- **Auth: prompt + session file** — token stored in `~/.config/aletheia/session.json` after first login. No browser-based OAuth for terminal context.
- **Dashboard mode default** — system status, agent list, recent activity. `Ctrl+F` toggles focused mode (chat only). `@mention` routing in input.
- **WebUI replacement, not supplement** — designed as the primary interface for operators who prefer terminal. Same API surface as webchat.
- Platform support: Linux primary, macOS secondary, Windows via WSL only.

### Agora — Channel Abstraction + Slack Integration (Spec 34, PRs #299–304)

Channel-agnostic messaging layer. Signal becomes a channel provider within agora, not a special case.

- **Agora (ἀγορά) = the gathering place.** Channels are stoa (covered walkways) — each enters through its own protocol but converges into a single discourse (the nous pipeline). No channel gets privileged access.
- **`ChannelProvider` interface** — `id`, `name`, `capabilities`, `start(ctx)`, `stop()`, `send(envelope)`. New channels implement this and register. Signal refactored to the same interface Slack uses.
- **`ChannelCapabilities` flags** — `threads`, `reactions`, `attachments`, `richText`, `streaming`, `presence`, `ephemeral`. Each channel declares what it supports; the dispatcher adapts.
- **Binding resolution by channel** — `channel: "slack"` in binding config, same pattern as existing `channel: "signal"`. Bindings already supported this conceptually.
- **CLI onboarding** — `aletheia channel add slack` guides token creation, scopes, channel selection. Same pattern for future channels.
- **Slack-specific:** Bot token + Socket Mode (no public URL needed). Scopes: `chat:write`, `channels:history`, `im:history`, `reactions:write`, `users:read`. App-level token for Socket Mode events.
- **Thread auto-creation for streaming** — Slack's `ChatStreamer` requires `thread_ts`. Non-threaded channel messages get a "…" anchor, stream within it.
- **DM access policy: `open | allowlist | pairing | disabled`** — pairing mode sends challenge code via DM, admin approves with `!approve <code>` from any channel.
- **`!command` interception** — shared `CommandRegistry` routes admin commands (`!approve`, `!deny`, `!contacts`, `!status`) before messages reach nous dispatch. `adminOnly` gating by user ID.
- **Idempotent reactions** — `addSlackReaction`/`removeSlackReaction` handle `already_reacted`/`no_reaction` errors silently. Processing emoji wraps entire dispatch lifecycle in `finally`.
- **No runtime entanglement** — unconfigured channels don't load, crashed channels don't take down others or the pipeline. Channel isolation is structural.
- 142 agora tests across 6 phases. Config hot-reload deferred (cross-cutting concern).


### Dianoia — Persistent Multi-Phase Planning Runtime (Spec 31)

Replaced session-scoped planning tools with a persistent SQLite-backed planning runtime driven by an 11-state FSM.

- **SQLite persistence over session memory.** All planning state survives runtime restart, session expiry, and agent swaps. 6 migrations (v20–v25) building incrementally.
- **Pure FSM as single source of truth.** `transition()` in `machine.ts` is a pure function — no I/O, no side effects. All state changes go through `DianoiaOrchestrator` which delegates to the FSM. No direct state writes bypass it.
- **Constructor injection everywhere.** Every orchestrator accepts `db` and `dispatchTool` as constructor arguments. Zero global state.
- **11 states, 15 transitions.** idle → questioning → researching → requirements → roadmap → phase-planning → executing → verifying → complete. Plus `blocked` and `abandoned` terminal/recovery states.
- **Wave-based parallel execution.** Dependency graph determines which phases can run in parallel. Phases within a wave execute concurrently via `sessions_dispatch`. Failed phases cascade-skip dependents.
- **Four parallel research dimensions.** Stack, features, architecture, pitfalls — spawned as parallel subagents, synthesized before requirements.
- **Interactive category-scoped requirements.** Requirements presented by category with table-stakes vs. differentiators. REQ-ID assignment. v1/v2/out-of-scope tiering with rationale.
- **Goal-backward verification.** `GoalBackwardVerifier` checks completed phases against their stated goals. Gap analysis generates closure plans.
- **3-tier risk checkpoints.** Low/medium/high risk evaluation. YOLO mode auto-approves low/medium. True-blocker category for high-risk items requiring human decision.
- **Spawn records survive restart.** v24 migration adds `planning_spawn_records` table. Zombie detection for spawns that started but never completed.
- **Legacy tools deprecated.** `plan_create` and `plan_propose` still work but emit deprecation warnings. New tool surface: `plan_research`, `plan_requirements`, `plan_roadmap`, `plan_execute`, `plan_verify`, `plan_discuss`.
- 88 source files, 40 test files. Integration test exercises full idle→complete pipeline with mocked dispatch.


### Dianoia v2 — Context-Engineered Planning with Sub-Agent Isolation (Spec 32)

Layered context engineering and file-backed state on top of Dianoia v1, solving orchestrator context saturation and state loss across distillation.

- **Files are source of truth.** `.dianoia/projects/{id}/` directory with PROJECT.md, REQUIREMENTS.md, ROADMAP.md, RESEARCH.md, and per-phase DISCUSS.md/PLAN.md/STATE.md/VERIFY.md. SQLite is the index; markdown files are the record. Projects reconstructable from files alone.
- **Atomic file writes.** `atomicWriteFile()` writes to `.tmp` then `rename()` — no partial file corruption on crash. Every file generator in `project-files.ts` uses this.
- **ContextPacketBuilder** assembles role-scoped context packets from project files. Sub-agents start at token 1 with exactly what they need. Budget per task type: research 8k, synthesis 16k, requirements 12k, discussion 16k (Opus), planning 24k, execution 32k, verification 16k.
- **Priompt-based token counting.** `js-tiktoken` (cl100k_base) for accurate token measurement. No character-based estimation.
- **Orchestrator stays under 40k tokens.** Reads PROJECT.md + ROADMAP.md + current phase summary. Everything else delegated. `orchestrator-context.ts` produces compact <4k summaries.
- **Discussion is first-class.** `discussing` FSM state added between roadmap and planning. Gray areas surfaced per-phase via Opus sub-agent. Decisions captured in DISCUSS.md and propagated as hard constraints to planning. `discuss-tool.ts` handles the full flow.
- **Codebase context mapping.** `codebase-map.ts` (599 lines) builds relevant file sets for execution steps — reads step targets from PLAN.md, includes referenced types/interfaces/tests, hard-caps at budget.
- **Sub-agent handoff.** `handoff.ts` wires context packets to spawn dispatches. Each execution step is an isolated sub-agent with no inherited chat history.
- **File-sync bidirectional.** `file-sync.ts` keeps SQLite and disk files consistent. Either can be canonical — import from files to recover from DB loss.
- **Verifier uses context packets.** `GoalBackwardVerifier.verify()` calls `buildContextPacketSync()` for scoped verification context. No raw execution output in orchestrator.
- **Planning UI: 19 Svelte components, 7,286 lines.** PlanningDashboard, milestone TimelineView, DiscussionPanel with decision cards, RequirementsTable, ExecutionStatus with wave tracking, SpawnStatus, VerificationPanel, CheckpointApproval, AnnotationPanel, EditHistory, ContextBudget visualization, MessageQueue, RoadmapView, ProjectHeader, RetrospectiveView, CategoryProposal, TaskList.
- **Learning/retrospective deferred to Spec 42.** Cross-project skill extraction, project retrospective generation, and reusable insights are the core thesis of Spec 42 (Nous Team — closing feedback loops). Verification is complete here; learning belongs in the spec that owns the feedback loop architecture.
- **Migration was non-breaking.** File generation layered on existing SQLite writes. Tool surface stable throughout. Phase 2 refactored internals only. Phase 3 added new UI + API endpoints.

---

## Absorbed Into Rust Rewrite (2026-02-28)

The following specs were absorbed into `docs/PROJECT.md` during the Rust rewrite planning consolidation. Their key decisions and design patterns are preserved here; implementation details are in the Rust crate design.

### Gnomon Alignment (Spec 33)

Module identity and naming infrastructure for the gnomon naming system.

- **Module identity declared once in `meta.ts` (TS) / module root (Rust).** Name, logger prefix, error module, route prefix, event namespace defined at the boundary. Consumers import from the boundary, never from internals.
- **Renames are O(1) after boundary infrastructure.** Rename directory + update one config = done. No grep-and-replace.
- **Topology before labels.** SOUL.md + TELOS.md + MNEME.md triad is topologically coherent. Workspace files use gnomon vocabulary.
- **Recognition, not disruption.** Internal module names change; external API routes stay stable (`/api/auth/*` doesn't become `/api/symbolon/*`).
- **Phase 1 (barrel exports, meta.ts) is TS-only — moot in Rust** where crate boundaries enforce this natively. Phase 2 (GOALS→TELOS, MEMORY→MNEME) happens during oikos migration. Phase 3-5 inherent in Rust's type system and crate naming.
- **Sub-agent role renames deferred** — `coder`/`reviewer`/etc. are clear enough as configuration labels; gnomon naming applies to modules, not sub-agent config values.

### Context Engineering (Spec 35)

Cache-aware bootstrap, skill filtering, turn classification.

- **Cache-group bootstrap:** Stable prefix must be byte-identical across turns for Anthropic prompt cache (~10x cost reduction). Volatile content (recall, turn-specific) goes at the bottom, never the middle.
- **Two-tier tool/skill descriptions:** Short manifest in prompt, full definition injected only when tool is called. ~4K token savings per turn.
- **Semantic skill retrieval:** Top-5 relevant skills per turn (embedding similarity to user message), not all 130. ~2K savings.
- **Turn bypass classifier:** Lightweight pre-pipeline check routes trivial turns past recall + working-state + fact extraction. Saves ~600-900ms + 2-3 Haiku calls.
- **Tool consolidation:** sessions_spawn+dispatch→1 tool, sessions_send+ask→1, 5 memory mutation→2. ~800 token savings.
- **Agent pinned context:** Agents control what stays resident vs. retrieved on demand. Recursive self-improvement at the prompt layer.
- **Current TS system: ~31,500–46,500 tokens/turn** for system prompt. Target savings: ~7,700 tokens/turn across all optimizations.
- Rust implementation: built into `nous::bootstrap` with stable prefix strategy, `nous::skills` for relevance filtering, `nous::classifier` for turn bypass. All grounded in oikos context assembly.

### Config Taxis (Spec 36)

4-layer workspace architecture and SecretRef credential storage. **Largely superseded by Spec 44 (Oikos).**

- **SecretRef pattern retained:** Credentials referenced by name in config, resolved at runtime. `taxis::secrets` crate handles file-based, env-var, and (future) Vault resolution.
- **Exec tool config:** Per-call `cwd` parameter, per-nous `workingDir` config, 120s default timeout (was 30s), dedicated `glob` tool for pattern matching.
- **Deploy pipeline gaps:** npm install in deploy, anchor.json scaffolding, agent workspace scaffolding, systemd config — all addressed by oikos migration + single binary deployment.
- **Memory sidecar security:** Bind to 127.0.0.1 (not 0.0.0.0), token auth at init. **Moot in Rust** — no sidecar, CozoDB is embedded.
- **Shell injection in start.sh:** Pass API key via env var, not string interpolation. **Moot in Rust** — no shell scripts.

### Metadata Architecture (Spec 37)

Declarative over imperative. Convention-based discovery. **Realized by Spec 44 (Oikos).**

- **Core principle: everything that *can* be config *is* config.** New capabilities added by dropping in files, not modifying code. Every `if (agentId === "X")` in core code is a failure of this principle.
- **Configuration cascade:** Global defaults → agent-level → session-level → message-level. Most specific wins.
- **Convention-based discovery:** File presence = feature enabled. No registration, no manifest.
- **Schema-first validation:** Invalid config fails fast at boot. Figment + validator in Rust.
- All patterns absorbed into oikos three-tier hierarchy (theke → shared → nous).

### Provider Adapters (Spec 38)

`trait LlmProvider` with multiple backend support.

- **`LlmProvider` trait:** `complete()`, `stream()`, `count_tokens()`, `supported_models()`. Provider-agnostic request/response types.
- **Provider registry:** Convention-based discovery from config. Model ID → provider mapping.
- **Anthropic first, others later:** OpenAI (GPT-4o, o1), Ollama (local models) are planned but not v1.
- **Fallback chain:** Retry on 429/5xx with next provider in chain. Per-agent model overrides via oikos cascade.
- **Open questions preserved:** Thinking budget normalization across providers, tool schema translation (Anthropic vs OpenAI formats), streaming event normalization.
- Rust implementation: `hermeneus` crate with `trait LlmProvider`, `AnthropicProvider` as default.

### Autonomy Gradient (Spec 39)

Confidence-gated step execution in Dianoia FSM.

- **Four autonomy levels:** `confirm-all` (0), `confirm-destructive` (1, default), `confirm-novel` (2), `confirm-never` (3). Configurable per agent via oikos cascade.
- **Level 1 (default):** Auto-advance read-only phases (research, requirements, roadmap). Confirm before writing files or executing code.
- **Level 2:** Auto-advance when task matches previously successful pattern (requires competence model integration).
- **Level 3:** Fully autonomous with audit trail. Confirm only on BLOCK or error.
- **Rollback:** Open question — mechanism if auto-advanced step produces bad output.
- Rust implementation: internal to `dianoia` crate FSM. Trust level per-nous via oikos config.

### Nous Team (Spec 42)

Closing feedback loops between primitives and autonomous operation.

- **Gap 1 — Closed feedback loop:** Competence scores influence routing. Reflection findings auto-promote to MNEME.md. Kritikos rejections increment corrections for relevant domain.
- **Gap 2 — Structured task handoff:** `task-create` / `task-send` as first-class primitives with state machine (created → assigned → in-progress → review → done). Context travels with handoff.
- **Gap 3 — Autonomous prioritization:** Prosoche triggers auto-create dianoia projects for high-urgency signals. Priority queue for idle agents.
- **Gap 4 — Epistemic confidence tiers:** Verified (checked against ground truth), Inferred (reasoned from context), Assumed (unchecked). Behavioral norm first, structured metadata later.
- **Gap 5 — Pressure-triggered memory consolidation:** Sub-agent spawn on conversation pressure (turns elapsed, token pressure, domain switch, session idle). Supplements nightly cron. Haiku-powered, async, non-blocking.
- **Gap 6 — Workspace hygiene check:** Session-start check for TELOS staleness, MNEME bloat, orphaned files, stale skills. Local file reads only, no LLM call.
- **Gap 7 — Agent-writable workspace files:** MNEME.md append-only, CONTEXT.md writable. SOUL.md/TELOS.md/USER.md operator-only. Duplicate detection via embedding similarity. Max 5 writes/turn.
- Rust implementation: Competence routing → `nous::routing`. Reflection → `daemon::evolution`. Consolidation → `daemon::consolidation`. Task handoff → `nous::tasks`. Confidence tiers → behavioral norm in AGENTS.md template.
