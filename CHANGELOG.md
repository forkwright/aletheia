# Changelog

All notable changes to Aletheia are documented here.

---

## [0.9.1] - 2026-02-20

### Added
- **Working state extraction** - Post-turn LLM extraction of structured task context (current task, completed steps, next steps, decisions, open files). Persisted on session, injected into system prompt, survives distillation.
- **Agent notes tool** - Explicit `note` tool (add/list/delete) with categories (task, decision, preference, correction, context). Notes survive distillation and are injected into system prompt with 2K token cap.
- **Release workflow** - GitHub Releases on tag push with auto-generated changelogs
- **Self-update CLI** - `aletheia update [version]` with locking, rollback, and health-check. Supports `--edge` (latest main), `--check` (dry run), `--rollback`.
- **Version tracking** - Build-time version injection, `/api/status` includes version, 6h periodic update check daemon
- **Thinking UI** - Status pills showing thinking duration during streaming, expandable detail panel with full reasoning
- **2D graph visualization** - Force-graph 2D as default (faster, more readable), lazy-loaded 3D via dynamic import, progressive node loading (batches of 200)
- **Error handling sweep** - `trySafe`/`trySafeAsync` utilities in `koina/safe.ts`, catch block improvements across store, finalize, tools, listener, TTS

### Fixed
- **Duplicate tool_result blocks** - Loop detector was pushing additional `tool_result` blocks with the same `tool_use_id`, causing Anthropic API 400 errors. Now appends to existing result instead.
- **Recall timeouts** - Vector-first search as primary path (~200-500ms), graph-enhanced only as fallback when no vector hits above threshold. Timeout bumped 3s to 5s.
- **Manager test failures** - Restored 27 tests by adding missing store mocks (getThinkingConfig, getNotes, getWorkingState, etc.)

### Changed
- Session metrics (duration, context utilization, distillation count) moved to separate block, injected every 8th turn instead of replacing working state

---

## [0.9.0] - 2026-02-18

### Added
- **Web UI feature pack** - file explorer with syntax-highlighted preview, settings panel, diff rendering in tool results
- **Tool approval system** - risk classification (low/medium/high/critical) with UI approval gates for dangerous operations
- **MCP client support** - stdio, SSE, and HTTP transports for connecting to external MCP servers
- **Prompt caching** - `cache_control` breakpoints on tool definitions and conversation history. Uses 2 of 4 Anthropic cache slots (other 2 on system prompt). 60-80% input token savings during tool loops.
- **Pipeline decomposition** - `manager.ts` broken into composable stages for testability
- **Design specs** - auth/security, data privacy, tool governance, distillation persistence, modular runtime architecture (in `docs/specs/`)

### Fixed
- History endpoint returns newest messages instead of oldest
- WebGPU polyfill for Three.js in non-WebGPU browsers
- Lazy-load GraphView to prevent Three.js crash on startup

---

## [0.8.0] - 2026-02-17

### Added
- **Structured logging** - turn-level tracing with request IDs, tool call timing, cache hit rates
- **Smart loop detection** - warn at 3, halt at 5 identical tool calls (replaces hard cap)
- **Pre-turn memory recall** - Mem0 search injected into context before each agent turn
- **Prosoche signal collectors** - calendar, task, cross-agent signals feed the attention daemon
- **Conversation backfill** - script to import existing session transcripts into the memory pipeline
- **Local embeddings** - Voyage-3-large with Ollama fallback
- **Instance branding** - configurable name, tagline, logo via config
- **Tool governance** - timeouts, turn cancellation, structured approval flow
- **Distillation persistence** - summaries written to agent workspace memory files

### Fixed
- Config env injection via typed `applyEnv()`
- fd binary symlink on Ubuntu CI
- Streaming UI bugs (stuck stop button, collapsed formatting, disappearing messages)

---

## [0.7.0] - 2026-02-16

### Added
- **Graph visualization** - 3D force-directed graph with community detection, progressive loading, Three.js rendering
- **Syntax highlighting** - tree-shaken highlight.js in chat and tool panel
- **Collapsible tool results** - `/compact` command for dense tool output
- **File uploads** - images (vision), PDFs, text files, code files via drag-and-drop
- **3D memory graph** - interactive visualization of entity relationships in the web UI

### Fixed
- Streaming events unbuffered for real-time delivery
- Credential file supports `apiKey` field (not just `authToken`)
- Bootstrap entry point renamed for tsdown v0.20 compatibility

---

## [0.6.0] - 2026-02-15

### Added
- **Web UI** - Svelte 5 chat interface at `/ui`
  - Streaming responses via SSE
  - Real-time event push
  - Per-agent conversation switching
  - Markdown rendering, emoji support
  - Mobile-responsive with swipe sidebar
  - Dark theme
- **Streaming API** - `completeStreaming()` on AnthropicProvider and ProviderRouter
- **Collapsible sidebar** - slash commands (`/new`, `/switch`, `/help`), copy code blocks
- **Live tool activity feed** - real-time tool execution status in UI
- **Event bus to SSE bridge** - turn, tool, session events broadcast to web clients
- **Cost endpoints** - `/api/costs/summary`, `/api/costs/session/:id`

### Changed
- JS bundle: 1,031KB to 199KB (81% reduction via hljs tree-shaking)
- Runtime bundle: 293KB to 354KB
- Static file serving with SPA fallback, immutable caching for hashed assets

---

## [0.5.0] - 2026-02-14

### Added
- **Memory intelligence** - graph analytics (PageRank, Louvain), query rewriting, foresight signals
- **Self-observation tools** - check_calibration, what_do_i_know, recent_corrections
- **Cross-agent deliberation** - structured dialectic protocol between agents
- **Discovery engine** - cross-domain connection finding via shared memory
- **Memory evolution** - merge, reinforce, decay lifecycle for stored facts
- **Temporal memory layer** - Graphiti-style episodic memory in the sidecar
- **Research meta-tool** - memory search, web search, synthesize pipeline
- **Whisper transcription** - wraps whisper.cpp for voice message handling
- **Browser automation** - LLM-driven web browsing via Playwright

### Changed
- Temperature routing based on message content classification
- MCP security hardening (auth, rate limiting, scopes, CORS)
- Cross-agent blackboard (SQLite migration v6, TTL-based expiry)

---

## [0.4.0] - 2026-02-13

### Added
- **CI/CD pipeline** - PR gates, nightly quality checks, deploy pipeline
- **Security scanning** - Dependabot, CodeQL, npm audit, pip-audit, TruffleHog
- **P0 test suite** - git pre-commit hooks, vitest with coverage thresholds
- **TTS pipeline** - Piper voice synthesis for audio responses
- **Circuit breakers** - input/response quality gates on agent turns
- **Ephemeral agents** - spawn short-lived sub-agents for isolated tasks
- **Self-authoring tools** - agents can create new tools at runtime
- **Competence model** - per-agent skill tracking and confidence scoring

### Changed
- Tests: 287 to 689 (80%+ coverage across 68 files)
- Dual licensing: AGPL-3.0 (runtime) + Apache-2.0 (SDK/client)

---

## [0.3.0] - 2026-02-11

### Added
- **Capability rebuild** - Signal commands (10 built-in), link preprocessing, media understanding, CLI admin, contact pairing, skills directory
- **Adaptive routing** - complexity-based model selection with tiered routing
- **Planning tools** - multi-step task decomposition for agents
- **Disagreement detection** - flags conflicting agent responses
- **Session replay CLI** - replay and re-execute past sessions
- **Operational metrics** - `/api/metrics` with per-agent stats, token usage, cache rates, cron status
- **Concurrent agent turns** - fire-and-forget SSE dispatch

### Fixed
- Distillation amnesia: summary marked `isDistilled: true` made it invisible to future turns
- Tool results excluded from extraction/summarization
- Tool definition tokens not subtracted from history budget
- Watchdog pgrep regex never matched gateway process

### Changed
- Token counter: added safety margin (1.15x), per-tool overhead accounting
- Distillation trigger uses actual API-reported input tokens instead of heuristic
- Multi-stage summarization for large conversations
- Heartbeat uses Haiku (95% token savings)

---

## [0.2.0] - 2026-02-08

### Added
- **Runtime v2** - clean-room rewrite, removed all upstream dependencies (789k lines, 47 packages)
- **Stack**: Hono (gateway), better-sqlite3 (sessions), @anthropic-ai/sdk, Zod (config), Commander (CLI)
- **OAuth authentication** - Anthropic Max plan routing with Bearer tokens
- **Mem0 memory integration** - automatic fact extraction via Claude Haiku
  - Qdrant vector store for semantic search
  - Neo4j graph store for entity relationships
  - FastAPI sidecar service (port 8230)
  - Cross-agent shared memory + agent-specific domain memory
- **Federated memory search** - sqlite-vec + Mem0 queried in parallel, merged and deduplicated
- **Prosoche daemon** - adaptive attention engine with configurable signal weights
- **Evaluation framework** - objective metrics and bias monitoring

### Changed
- FalkorDB retired, data migrated to Neo4j
- Memory plugin hooks: before_agent_start (recall), agent_end (extract)
- Config paths: `.openclaw/` to `.aletheia/`

---

## [0.1.0] - 2026-02-05

### Added
- Initial fork from OpenClaw as Aletheia
- Multi-agent architecture with per-agent workspaces
- Signal messaging via signal-cli
- sqlite-vec local memory search
- Structured distillation (pre-compaction fact extraction)
- Context assembly pipeline
- Knowledge graph with ontology
- Research tools (scholar, wiki)
- Langfuse observability integration
