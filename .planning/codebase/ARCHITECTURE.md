# Architecture

**Analysis Date:** 2026-02-24

## Pattern Overview

**Overall:** Multi-agent distributed cognition system with a unified message pipeline and pluggable provider routing.

**Key Characteristics:**
- Event-driven architecture with streaming message pipeline
- Modular layer separation with clean boundaries via type interfaces
- Session-based state persistence with context summarization (distillation)
- Provider-agnostic routing with complexity-based model selection
- Daemon processes for background tasks (scheduling, health monitoring, memory flushing)

## Layers

**Taxis (Configuration & Loading):**
- Purpose: Schema validation, configuration loading, agent resolution, environment application
- Location: `infrastructure/runtime/src/taxis/`
- Contains: Zod schemas, config loader, path resolver, environment application
- Depends on: Node filesystem, koina utilities
- Used by: Entry point, Aletheia orchestrator, all modules needing config context

**Koina (Shared Utilities & Cross-Cutting):**
- Purpose: Error hierarchy, logging, event bus, encryption, diagnostics, safe wrappers
- Location: `infrastructure/runtime/src/koina/`
- Contains: Structured error classes, typed logger, event emitter, encryption, safe/trySafe helpers
- Depends on: Node built-ins, third-party logging/crypto
- Used by: Every other module

**Mneme (Session Store & Message History):**
- Purpose: SQLite persistence layer with message history, working state, distillation priming
- Location: `infrastructure/runtime/src/mneme/`
- Contains: SQLite schema, migrations, session CRUD, message append, usage tracking
- Depends on: better-sqlite3, encryption layer
- Used by: Pipeline stages, distillation, API routes

**Hermeneus (Provider Router & LLM Abstraction):**
- Purpose: Model abstraction, token counting, streaming response handling, complexity-based routing
- Location: `infrastructure/runtime/src/hermeneus/`
- Contains: Anthropic SDK wrapper, router logic, token counter, provider selection
- Depends on: Anthropic SDK, token estimator
- Used by: Pipeline execute stage, distillation, agents

**Organon (Tool Registry & Execution):**
- Purpose: Tool registration, permission checks, execution, approval gates, MCP server management
- Location: `infrastructure/runtime/src/organon/`
- Contains: 41 built-in tools (read, write, exec, grep, browser, memory, sessions, etc.), tool registry, approval logic
- Depends on: Koina, Mneme (for audit), hermeneus (for external tools)
- Used by: Pipeline execute stage

**Semeion (Signal Integration & Commands):**
- Purpose: Signal messenger integration, command parsing, message sending, TTS, daemon spawning
- Location: `infrastructure/runtime/src/semeion/`
- Contains: signal-cli RPC client, listener, sender, command registry, TTS wrapper
- Depends on: Node child_process, HTTP client, signal-cli daemon
- Used by: Message routing, tool handlers

**Pylon (HTTP Gateway & API):**
- Purpose: HTTP server (Hono), REST API routes, Server-Sent Events (SSE), WebSocket bridging
- Location: `infrastructure/runtime/src/pylon/`
- Contains: Route handlers (sessions, agents, costs, auth, workspace, etc.), MCP route, UI static serve
- Depends on: Hono, Express-style middleware
- Used by: Entry point as main server interface

**Nous (Agent Bootstrap & Turn Pipeline):**
- Purpose: Agent workspace loading, turn coordination, pipeline orchestration, competence tracking
- Location: `infrastructure/runtime/src/nous/`
- Contains: NousManager, pipeline runner, pipeline stages (resolve, guard, context, history, execute, finalize)
- Depends on: Taxis, Mneme, Hermeneus, Organon, Semeion, Distillation
- Used by: API routes, message handlers

**Prostheke (Plugin System):**
- Purpose: Plugin discovery and loading for extensibility
- Location: `infrastructure/runtime/src/prostheke/`
- Contains: Plugin loader, registry, plugin types
- Depends on: Koina (logging)
- Used by: Aletheia orchestrator

**Distillation (Context Compression Pipeline):**
- Purpose: Multi-pass session summarization, fact extraction, memory flush, context truncation
- Location: `infrastructure/runtime/src/distillation/`
- Contains: Extraction pipeline, summarizer, chunked summarizer, workspace flusher, hooks
- Depends on: Hermeneus, Mneme, Koina
- Used by: Daemon cron jobs, manual API triggers, automatic thresholds

**Daemon (Background Tasks):**
- Purpose: Cron scheduling, health monitoring, update checking, retention policies, reflection cycles
- Location: `infrastructure/runtime/src/daemon/`
- Contains: Cron scheduler, watchdog, reflection cycle, evolution cycle, retention manager
- Depends on: Mneme, Hermeneus, Distillation, Koina
- Used by: Aletheia orchestrator

**Auth (Session Management & Audit):**
- Purpose: Authentication, session tokens, audit logging
- Location: `infrastructure/runtime/src/auth/`
- Contains: Auth session store, audit log, token generation
- Depends on: Crypto utilities, Koina
- Used by: API routes, gateway middleware

## Data Flow

**Turn Processing (Core Message Pipeline):**

1. Inbound message arrives (HTTP, Signal, Signal command)
2. **Resolve Stage** (`nous/pipeline/stages/resolve.ts`): Route to agent, load workspace, create/reuse session
3. **Guard Stage** (`nous/pipeline/stages/guard.ts`): Check approval gates, guard rails, permissions
4. **Context Stage** (`nous/pipeline/stages/context.ts`): Load system prompt, gather context from memory/workspace, build context
5. **History Stage** (`nous/pipeline/stages/history.ts`): Prepare message history, handle distillation priming, truncate if needed
6. **Execute Stage** (`nous/pipeline/stages/execute.ts`): Stream or buffer LLM call, handle tool requests, emit events
7. **Finalize Stage** (`nous/pipeline/stages/finalize.ts`): Save outcome, emit completion event, trigger auto-distillation

**State Transitions:**
- Message persisted at each step, state object passed through stages
- On error, pipeline captures stage, logs, broadcasts event, returns error outcome
- Session locked during turn to prevent concurrent modification

**Distillation Flow (Background):**
1. Cron or API trigger
2. Check token threshold and message count
3. Extract facts from recent messages
4. Summarize in stages (chunked summaries → final summary)
5. Prune by semantic similarity
6. Flush to memory system if enabled
7. Mark messages as distilled, store priming for next turn

**State Management:**
- Session state live in SQLite: messages, usage, status, working state, distillation priming
- Working state tracked: current task, completed steps, next steps, open files
- Distillation priming persists extracted facts/decisions for context bootstrap
- Turn state object flows through pipeline stages, immutable at boundary

## Key Abstractions

**RuntimeServices:**
- Purpose: Injects all dependencies into pipeline stages
- Examples: `infrastructure/runtime/src/nous/pipeline/types.ts`
- Pattern: Typed bag of services (store, router, tools, config, plugins, etc.)
- Reduces coupling: stages depend on interface, not concrete implementations

**AletheiaError:**
- Purpose: Structured error hierarchy with codes, module, context, recoverable flag
- Examples: `koina/errors.ts` (ConfigError, SessionError, ProviderError, ToolError, etc.)
- Pattern: All errors inherit from AletheiaError, toJSON() for serialization

**Tool Definition:**
- Purpose: Declarative tool interface with input schema, execution, permissions
- Examples: `organon/registry.ts`, 41 built-in tools in `organon/built-in/`
- Pattern: Each tool exports create function, implements execute (sync or async), validates input

**Session:**
- Purpose: First-class entity representing agent-user conversation
- Examples: `mneme/store.ts` (Session, Message, WorkingState, DistillationPriming)
- Pattern: Atomic persistence, foreign key to agent, message history is append-only

**Pipeline Stage:**
- Purpose: Pure function that transforms TurnState, emits events
- Examples: All files in `nous/pipeline/stages/`
- Pattern: `(state, services) => state | error`, throws on fatal conditions

## Entry Points

**CLI (`infrastructure/runtime/src/entry.ts`):**
- Location: `infrastructure/runtime/src/entry.ts`
- Triggers: `node aletheia.mjs <command>`
- Responsibilities: Command parsing (init, gateway, status, send, sessions, cron, update, doctor), error handling

**Gateway HTTP Server (`infrastructure/runtime/src/pylon/server.ts`):**
- Location: `infrastructure/runtime/src/pylon/server.ts`
- Triggers: `aletheia gateway` command
- Responsibilities: Listen on port 18789 (configurable), route requests, SSE streaming, WebSocket proxy

**Signal Listener (`infrastructure/runtime/src/semeion/listener.ts`):**
- Location: `infrastructure/runtime/src/semeion/listener.ts`
- Triggers: Started by Aletheia orchestrator if Signal enabled
- Responsibilities: Listen to signal-cli, dispatch messages to agents

**Daemon Scheduler (`infrastructure/runtime/src/daemon/cron.ts`):**
- Location: `infrastructure/runtime/src/daemon/cron.ts`
- Triggers: Started by Aletheia orchestrator
- Responsibilities: Trigger cron jobs (reflection, evolution, heartbeat, retention)

**Aletheia Orchestrator (`infrastructure/runtime/src/aletheia.ts`):**
- Location: `infrastructure/runtime/src/aletheia.ts`
- Triggers: Called by entry point
- Responsibilities: Load config, wire all modules, start HTTP server, Signal listener, daemons, handle lifecycle

## Error Handling

**Strategy:** Typed, recoverable, context-rich errors that allow graceful degradation.

**Patterns:**
- All thrown errors inherit from AletheiaError (koina/errors.ts)
- Each error has: code (machine-readable), module (originating component), recoverable (retry-safe), context (diagnostics)
- Try-safe wrappers (`koina/safe.ts`): trySafe/trySafeAsync convert exceptions to `{ ok: true/false, error?, value? }`
- Pipeline catches at stage boundary, emits error event, returns error outcome
- Tools fail gracefully: invalid input returns error result, execution error returns tool_result with error message
- No empty catch blocks (enforced by lint rules)

## Cross-Cutting Concerns

**Logging:** Structured logger via `createLogger("module-name")` using AsyncLocalStorage context injection. Logs include: timestamp, level, module, message, context object.

**Validation:** Zod schemas at config boundaries (taxis/schema.ts). Tool input validated via tool schema before execution. API payloads validated via route-specific schemas.

**Authentication:** Mode-based (none, token, session) via auth routes. Token stored in Bearer header or query param. Audit log all API calls.

**State Isolation:** Sessions locked during turns via sessionLocks map. Concurrent writes prevented. Distillation blocked if already in progress.

**Event System:** eventBus (koina/event-bus.ts) emits: turn:start, turn:complete, turn:error, tool:called, tool:result, pipeline:error, session:distilled. Subscribed by API streaming, metrics collection.

---

*Architecture analysis: 2026-02-24*
