# Architecture

**Analysis Date:** 2026-02-23

## Pattern Overview

**Overall:** Hub-and-spoke event-driven system with distributed cognition agents

**Key Characteristics:**
- Multi-agent orchestration with single runtime managing many nous (agents)
- Message-driven turns with streaming event architecture
- Session-scoped persistence with progressive context compression (distillation)
- Modular tool system with runtime-discoverable skills and plugins
- HTTP gateway exposing agent capabilities to TUI, web UI, and external services

## Layers

**Entry & CLI:**
- Purpose: Command-line interface, daemon lifecycle, first-run setup
- Location: `infrastructure/runtime/src/entry.ts`, `infrastructure/runtime/src/aletheia.ts`
- Contains: Program initialization, credential bootstrapping, configuration loading
- Depends on: All modules via `createRuntime()`
- Used by: Node.js process, shell invocation

**Configuration & Paths (Taxis):**
- Purpose: Schema validation, environment binding, file path resolution
- Location: `infrastructure/runtime/src/taxis/`
- Contains: Zod schemas, loader, scaffold, paths utility
- Defines: `AletheiaConfig` (root config), `NousConfig` (per-agent), environment variable mapping
- Depends on: koina (encryption, errors)
- Used by: runtime initialization, reload on config changes

**Core Services:**
- **Session Store (Mneme):** SQLite persistence layer
  - Location: `infrastructure/runtime/src/mneme/store.ts`
  - Manages: Sessions, messages, quotes, agent notes, usage records
  - Uses: better-sqlite3 in WAL mode, supports optional encryption
- **Provider Router (Hermeneus):** Model provider abstraction
  - Location: `infrastructure/runtime/src/hermeneus/router.ts`, `anthropic.ts`
  - Routes: Model strings to Anthropic providers, handles failover
  - Manages: Streaming responses, token counting, error recovery
- **Tool Registry (Organon):** Tool discovery and execution
  - Location: `infrastructure/runtime/src/organon/registry.ts`
  - Contains: Built-in tools (exec, read, write, grep, browser, etc.), custom commands, skill learner
  - Pattern: Tool handlers as async functions with approval gate, timeout, reversibility checks
- **Agent Manager (Nous):** Orchestration, turn lifecycle, session locking
  - Location: `infrastructure/runtime/src/nous/manager.ts`
  - Coordinates: Turn execution via streaming or buffered pipeline
  - Tracks: Active turns per agent, session locks, approval gates
  - Fires: Events for turn lifecycle (`turn:before`, `turn:after`, `tool:called`)
- **Memory Sidecar (Python FastAPI):**
  - Location: `infrastructure/memory/sidecar/aletheia_memory/`
  - Provides: Entity resolution, temporal fact graph, memory evolution
  - Consumed by: Memory flush operations during distillation

**Processing Pipelines:**
- **Turn Pipeline:** `nous/pipeline/`
  - Stages: resolve nous → validate session → load context → execute turn → compress → flush
  - Streaming: Events pushed via AsyncChannel to gateway
  - Buffered: Entire turn result collected before return
- **Distillation Pipeline:** `distillation/`
  - Steps: Extract facts → Summarize messages → Prune by similarity → Flush to memory
  - Trigger: Token count threshold or manual trigger
  - Lightweight mode: Single-sentence summary for background sessions

**Gateway & HTTP API:**
- Purpose: Accept messages, serve events, manage auth, expose system
- Location: `infrastructure/runtime/src/pylon/server.ts`, `pylon/routes/`
- Routing: Hono-based with route modules (sessions, turns, agents, auth, etc.)
- Auth: Optional token/session modes, RBAC, audit log
- Events: Broadcast turn stream events to connected clients via SSE or polling

**Terminal UI (Rust):**
- Purpose: Real-time dashboard in terminal
- Location: `tui/src/`
- Architecture: Ratatui event-driven state machine
- Communication: HTTP to gateway with token auth
- Display: Dashboard with agent status, session list, turn streaming

**Web UI (Svelte):**
- Purpose: Browser interface for turn execution, history, settings
- Location: `ui/src/`
- Architecture: Svelte 5 with reactive stores
- Communication: API calls to gateway
- Features: Chat-like interface, session history, configuration UI

**Daemon Services:**
- **Cron Scheduler:** `daemon/cron.ts`
  - Schedules: Nightly reflection, weekly reflection, evolution cycle
  - Triggers: Distillation, model fine-tuning signals
- **Watchdog:** `daemon/watchdog.ts`
  - Monitors: Service probes, provider health, circuit breaker
  - Acts: Auto-restart services, adjust routing based on health
- **Update Checker:** `daemon/update-check.ts`
  - Checks: GitHub releases, manages autoupdate (if enabled)

**Plugin System (Prostheke):**
- Purpose: Runtime-loadable extensions
- Location: `infrastructure/runtime/src/prostheke/`
- Discovery: Scan workspace for `nous/*/plugin/index.ts`
- Lifecycle: Load at startup, reload on config change
- Plugins can: Define new tools, modify hooks, add event listeners

## Data Flow

**Turn Execution (Happy Path):**

1. Message arrives via HTTP POST to `/api/sessions/{id}/turns`
2. NousManager acquires session lock, initializes turn context
3. Pipeline resolves nous ID from config, validates against bindings
4. Loads session context: previous messages, system prompts, tools allowed
5. ModelRouter selects provider based on model string + failover
6. Calls `anthropic.complete()` with streaming enabled
7. Tool calls intercepted, executed via ToolRegistry (with approval gate if needed)
8. Each tool result streamed back as `tool_result` event
9. Token counting tracks input/output/cache usage
10. Turn complete event emitted with outcome (TurnOutcome)
11. Distillation check: if tokens > threshold, async pipeline triggered
12. Response returned or streamed to client via EventSource

**Distillation (Async):**

1. shouldDistill() checks token threshold and message count
2. Extract facts from recent messages using distillation model
3. Summarize older messages in chunks (chunked-summarize)
4. Prune facts by semantic similarity (similarity-pruning)
5. Flush to memory sidecar (entity graph stored in Python sidecar)
6. Update session.distillationCount, lastDistilledAt
7. Fire `session:distilled` event

**Plugin Loading:**

1. At runtime startup, discoverPlugins() scans workspace for plugin manifest
2. Each plugin exports `setup(runtime: AletheiaRuntime)` function
3. Plugins register: tools, hooks, event listeners
4. Tools added to registry, can be used in turns immediately

## Key Abstractions

**Session:**
- Represents a conversation thread with an agent
- Grain: sessionId (UUID), sessionKey (public sharable key)
- Contains: Messages (history), metadata (status, token counts, distillation state)
- Scoped to: One nous (agent), can have parent session for sub-conversations

**Turn:**
- Single request/response cycle from user to agent
- Contains: Inbound message, system prompts, tool calls, agent response
- Streaming: Events emitted continuously, client subscribes via EventSource
- Buffered: Entire turn collected, returned as JSON

**Nous (Agent):**
- Represents one AI agent instance
- Configuration: Model, workspace, allowed tools, subagents, heartbeat
- Multiple nous can exist in single runtime (multi-tenant)
- Bindings: Match channel/peerId to route incoming messages to specific nous

**Tool:**
- Executable action with schema, approval requirements, reversibility
- Execution: Runs in dockerized sandbox (if docker available), timeout protected
- Results: Streamed back to agent as tool_result messages
- Built-in: exec, read, write, edit, grep, find, ls, web-fetch, browser, message
- Custom: Self-authored via skill-learner or uploaded via prostheke plugins

**Memory:**
- Fact graph stored in Python sidecar (better-embed-soft with temporal indexing)
- Flush trigger: Distillation pipeline writes extracted facts
- Consumption: Memory context injected at turn start via mem0-search tool
- Evolution: Daemon runs nightly/weekly evolution cycle to refactor facts

## Entry Points

**HTTP API:** `infrastructure/runtime/src/pylon/server.ts`
- Pattern: Hono app with route modules
- Auth: Middleware checks token/session if enabled
- Key routes: `/api/sessions`, `/api/turns`, `/api/agents`, `/api/events`
- WebSocket: None (uses SSE for event streaming)

**CLI Commands:** `infrastructure/runtime/src/entry.ts`
- `aletheia init` - First-run setup wizard
- `aletheia start` - Launch runtime + gateway + daemon
- `aletheia doctor` - Config validation and diagnostics

**Terminal UI:** `tui/src/main.rs`
- Entrypoint: `main()` async fn
- Connects to gateway via HTTP (TLS optional)
- Event loop: Ratatui + Tokio + crossterm

**Web UI:** `ui/src/main.ts`
- Entrypoint: Mount Svelte app to DOM
- Stores: Reactive state for sessions, turns, configuration
- API calls: Fetch to gateway (CORS, JSON)

## Error Handling

**Strategy:** Typed error hierarchy with recovery information

**Patterns:**
- `AletheiaError` base class in `koina/errors.ts`
  - Includes: `code`, `module`, `message`, `context`, `recoverable`, `retryAfterMs`
- Tool errors: Caught, sanitized, returned as tool_result
- Provider errors: Logged, trigger failover to backup credentials
- Session errors: Return 400/404 with error detail
- Unhandled rejections: Logged, process exit(1)

**Specific Cases:**
- Tool timeout: Error returned to agent, counted in turn
- Tool approval denied: Turn aborted with specific error
- Distillation conflict: SESSION_LOCKED error with retry-after
- Config validation: Errors collected, returned in diagnostic
- Memory sidecar offline: Distillation skipped, warning logged

## Cross-Cutting Concerns

**Logging:**
- Framework: `koina/logger.ts`
- Pattern: `createLogger("module-name")` per module
- Context: AsyncLocalStorage for request ID, session ID, nous ID
- Levels: Debug/info/warn/error, structured JSON output

**Validation:**
- Zod schemas for: Config, message payloads, tool inputs, API responses
- Parse at: Load config, receive API request, prepare tool call
- Failure: Return validation error with field details

**Authentication:**
- Modes: `none` (localhost only), `token` (API key), `session` (multi-user)
- Middleware: `auth/middleware.ts` checks bearer token or session cookie
- RBAC: Simple role system (admin, agent-runner, viewer)
- Audit log: Record auth events, tool approvals, config changes

**Encryption:**
- Session messages optionally encrypted (enable in config)
- Key: Passphrase from `ALETHEIA_ENCRYPTION_KEY` env var
- Salt: Stored in `~/.aletheia/encryption.salt` with 0600 perms
- Only applied to message content, not metadata

**Event Bus:**
- Central event emitter in `koina/event-bus.ts`
- Patterns: `noun:verb` (e.g., `turn:before`, `session:distilled`)
- Used by: Distillation hooks, daemon tasks, plugins
- No built-in persistence (fire-and-forget)

---

*Architecture analysis: 2026-02-23*
