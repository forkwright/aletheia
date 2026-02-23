# Codebase Structure

**Analysis Date:** 2026-02-23

## Directory Layout

```
aletheia/
├── infrastructure/           # Core runtime and sidecars
│   ├── runtime/             # Main TypeScript runtime (Node.js)
│   │   ├── src/             # Source code (modules with Greek names)
│   │   ├── dist/            # Compiled output
│   │   └── package.json     # Dependencies
│   ├── memory/              # Memory sidecar (Python FastAPI)
│   │   └── sidecar/         # Memory graph service
│   ├── prosoche/            # Prosoche integration (unused/archive)
│   ├── browser-use/         # Browser automation integration
│   ├── langfuse/            # LLM observability integration
│   └── evaluation/          # Benchmarking and eval scripts
├── ui/                      # Web UI (Svelte 5)
│   ├── src/                 # Components, stores, styles
│   ├── public/              # Static assets
│   └── package.json
├── tui/                     # Terminal UI (Rust/Ratatui)
│   ├── src/                 # Rust application code
│   └── Cargo.toml
├── bin/                     # Executables and shell scripts
├── config/                  # Example configurations
│   └── services/            # Service definitions
├── docs/                    # Specifications and design docs
│   └── specs/               # Numbered phase specs
├── extensions/              # CLI extensions
├── shared/                  # Shared workspace artifacts
│   ├── skills/              # Reusable skill implementations
│   ├── hooks/               # Hook example templates
│   ├── commands/            # Command templates
│   ├── schemas/             # Reusable Zod schemas
│   └── tools/               # Custom tool implementations
├── nous/                    # Nous workspace template
│   └── _example/            # Example agent scaffold
├── .github/                 # GitHub workflows and issue templates
├── .githooks/               # Git hooks
└── package.json             # Root workspace
```

## Directory Purposes

**`infrastructure/runtime/src/`** — Main application logic

Contains all TypeScript modules using Greek naming convention:

- **`aletheia.ts`** — Main orchestration, wires all modules together
- **`entry.ts`** — CLI entry point, command definitions
- **`koina/`** — Foundation utilities
  - `logger.ts` — Structured logging with AsyncLocalStorage context
  - `errors.ts` — Error type hierarchy, error codes registry
  - `event-bus.ts` — Central event emitter
  - `fs.ts` — File operations (read, write, JSON handling)
  - `crypto.ts` — ID generation, session key creation
  - `encryption.ts` — Message encryption/decryption
  - `safe.ts` — Try-catch wrappers (trySafe, trySafeAsync)
  - `hooks.ts` — Hook system for extension points

- **`mneme/`** — Session persistence layer
  - `store.ts` — SQLite session store (messages, metadata, usage)
  - `schema.ts` — Database DDL and migrations

- **`hermeneus/`** — LLM provider abstraction
  - `router.ts` — Routes model strings to providers, handles failover
  - `anthropic.ts` — Anthropic provider implementation, streaming
  - `token-counter.ts` — Token estimation for cost tracking

- **`nous/`** — Agent orchestration
  - `manager.ts` — Lifecycle, turn coordination, session locking
  - `pipeline/` — Turn execution stages
    - `types.ts` — Message types, events, plan structures
    - `runner.ts` — Streaming and buffered pipeline execution
    - `stages/` — Individual pipeline stage handlers
  - `bootstrap.ts` — Session initialization with context
  - `competence.ts` — Agent skill modeling
  - `uncertainty.ts` — Confidence tracking
  - `trace.ts` — Turn trace collection for debugging
  - `loop-detector.ts` — Detects repetitive tool call patterns

- **`organon/`** — Tool system
  - `registry.ts` — Tool registry and context
  - `approval.ts` — Approval gate (autonomous/guarded/supervised)
  - `built-in/` — Integrated tools
    - `exec.ts`, `read.ts`, `write.ts`, `edit.ts` — File I/O
    - `grep.ts`, `find.ts`, `ls.ts` — File search
    - `browser.ts` — Browser automation
    - `web-fetch.ts`, `web-search.ts`, `brave-search.ts` — HTTP
    - `message.ts` — Send messages to users
    - `sessions-*.ts` — Subagent spawning and dispatch
    - `plan.ts` — Plan execution tools
    - `research.ts`, `deliberate.ts` — Metacognitive tools
    - `memory-*.ts`, `mem0-*.ts` — Memory access
    - `blackboard.ts`, `note.ts` — Working memory
  - `skills.ts` — Skill registry for learned tools
  - `self-author.ts` — Runtime tool creation from agent code
  - `custom-commands.ts` — User-defined command loading
  - `mcp-client.ts` — Model Context Protocol integration
  - `reversibility.ts` — Undo mechanism for tool calls
  - `timeout.ts`, `sandbox.ts` — Execution safety

- **`distillation/`** — Context compression pipeline
  - `pipeline.ts` — Orchestrates distillation stages
  - `extract.ts` — Fact extraction from messages
  - `summarize.ts` — Message summarization
  - `chunked-summarize.ts` — Summarize in stages
  - `similarity-pruning.ts` — Semantic deduplication
  - `hooks.ts` — Memory flush triggers
  - `workspace-flush.ts` — Save distilled content to workspace

- **`daemon/`** — Background services
  - `cron.ts` — Scheduler for periodic tasks
  - `reflection-cron.ts` — Nightly/weekly reflection
  - `evolution-cron.ts` — Model evolution cycle
  - `retention.ts` — Session cleanup
  - `watchdog.ts` — Service health monitoring
  - `update-check.ts` — Version checking

- **`pylon/`** — HTTP gateway (Hono)
  - `server.ts` — App initialization, middleware
  - `routes/` — API endpoints
    - `sessions.ts` — Session CRUD
    - `turns.ts` — Turn execution, streaming
    - `agents.ts` — Agent listing, configuration
    - `auth.ts` — Login, token refresh
    - `events.ts` — Event streaming (SSE)
    - `plans.ts` — Plan management
    - `skills.ts` — Skill discovery
    - `memory.ts` — Memory queries
    - And 10+ others for specialized resources

- **`auth/`** — Authentication & authorization
  - `middleware.ts` — Token/session validation
  - `sessions.ts` — Session store for multi-user mode
  - `tokens.ts` — JWT-like token generation
  - `rbac.ts` — Role-based access control
  - `audit.ts` — Audit log recording

- **`taxis/`** — Configuration system
  - `schema.ts` — Zod schemas for entire config
  - `loader.ts` — Config file parsing, environment binding
  - `paths.ts` — Resolves file paths (~/.aletheia, workspaces, etc.)
  - `scaffold.ts` — Agent initialization from template

- **`semeion/`** — Message delivery
  - `sender.ts` — Send messages to external contacts
  - `listener.ts` — Listen for incoming messages (Slack, etc.)
  - `client.ts` — Signal client for async communication
  - `commands.ts` — Message command registry
  - `daemon.ts` — Sidecar process management
  - `tts.ts` — Text-to-speech synthesis

- **`prostheke/`** — Plugin system
  - `loader.ts` — Discovers plugins in workspace
  - `registry.ts` — Plugin registry
  - Plugin interface: exports `setup(runtime)`

- **`hermeneus/`** — LLM inference
  - Handles streaming, token counting, provider routing

- **`portability/`** — Session export/import (minimal)

- **`version.ts`** — Version constant

**`infrastructure/memory/sidecar/`** — Python FastAPI service

- `aletheia_memory/app.py` — FastAPI application
- `routes.py` — Endpoints for memory operations
- `graph.py` — Temporal fact graph storage
- `entity_resolver.py` — Entity linking
- `evolution.py` — Fact refinement logic
- `vocab.py` — Entity vocabulary management

**`ui/src/`** — Svelte 5 web interface

- `App.svelte` — Root component
- `components/` — Reusable UI components
- `lib/` — API client, utilities
- `stores/` — Reactive state (Svelte stores)
- `styles/` — Global CSS, theming

**`tui/src/`** — Rust terminal UI

- `main.rs` — Entry point, event loop
- `app.rs` — Application state machine
- `api/` — HTTP client to gateway
- `view/` — Ratatui rendering
- `theme.rs` — Terminal color theming
- `highlight.rs` — Syntax highlighting for code blocks
- `markdown.rs` — Markdown rendering in terminal

**`bin/`** — Executables

- `aletheia` — Symlink to compiled runtime
- Shell scripts for setup, health checks

**`docs/specs/`** — Design documents

- Numbered by implementation phase (e.g., `spec-01-*.md`)
- Track architecture decisions, API contracts

**`shared/skills/`** — Reusable skill library

- Each subdirectory is a skill (e.g., `add-authentication-wrapper-to-media-urls-across-codebase/`)
- Can be loaded into a workspace via skill system

## Key File Locations

**Entry Points:**
- `infrastructure/runtime/src/entry.ts` — CLI initialization
- `infrastructure/runtime/src/aletheia.ts` — Runtime wiring
- `ui/src/main.ts` — Web UI initialization
- `tui/src/main.rs` — Terminal UI initialization

**Configuration:**
- `~/.aletheia/aletheia.json` — Runtime configuration (validated by `taxis/schema.ts`)
- `infrastructure/runtime/package.json` — Dependencies
- `infrastructure/runtime/tsconfig.json` — TypeScript config

**Core Logic:**
- `nous/manager.ts` — Turn orchestration
- `organon/registry.ts` — Tool execution
- `hermeneus/router.ts` — Model routing
- `mneme/store.ts` — Session persistence
- `pylon/server.ts` — HTTP gateway

**Testing:**
- `*.test.ts` — Vitest unit tests (co-located with source)
- `infrastructure/runtime/vitest.config.ts` — Test configuration

## Naming Conventions

**Files:**
- Modules: Greek names in lowercase (koina, nous, organon, hermeneus, mneme, etc.)
- Test files: `{name}.test.ts` (vitest co-located)
- Tools: Descriptive kebab-case (exec, read-file, grep, etc.)
- Skills: Descriptive kebab-case in `shared/skills/`

**Directories:**
- Lowercase, descriptive purpose
- No excessive nesting (2-3 levels typical)
- `built-in/` for framework-provided tools

**Functions:**
- camelCase for public functions
- Private prefixed with `_` or in closure scope

**Variables & Types:**
- camelCase for variables
- PascalCase for types, interfaces, classes
- Prefer descriptive names over single letters (except loop counters)

**Nous (Agents):**
- IDs: lowercase with hyphens (e.g., `atlas`, `default-explorer`)
- Workspaces: `~/nos/{nous-id}/` on filesystem
- Directories: `{nous-id}/` in config

## Where to Add New Code

**New Tool (Built-in):**
- Location: `infrastructure/runtime/src/organon/built-in/{tool-name}.ts`
- Pattern:
  ```typescript
  export const {toolName}Tool = (): ToolHandler => {
    return {
      name: "tool-name",
      description: "...",
      execute: async (input, context) => { ... }
    };
  };
  ```
- Register: Import in `aletheia.ts` and add to tool registry

**New Skill (Custom Tool from Agent):**
- Location: Stored in database via skill-learner
- Discovered at: Runtime when agent writes `~/.aletheia/skills/{skill-id}.ts`
- Loaded via: `organon/self-author.ts` at turn time

**New Nous (Agent):**
- Template: Copy `nous/_example/` to `nous/{agent-id}/`
- Config: Add entry to `aletheia.json` agents.list
- Workspace: Initialize in `~/.aletheia/agents/{agent-id}/`
- First run: `aletheia init` to scaffold

**New Route (API Endpoint):**
- Location: `infrastructure/runtime/src/pylon/routes/{domain}.ts`
- Pattern: Export route group from Hono app
- Register: Import in `pylon/server.ts`, mount to app
- Namespace: Use `/api/{resource}` pattern

**New Daemon Task:**
- Location: `infrastructure/runtime/src/daemon/{task-name}.ts`
- Export: Async function that takes CronScheduler, runs scheduled task
- Lifecycle: Created in `aletheia.ts`, triggered by cron
- Logging: Use `createLogger("{task-name}")`

**New UI Component (Web):**
- Location: `ui/src/components/{ComponentName}.svelte`
- Pattern: Svelte 5 component with props and reactive state
- Styling: Scoped `<style>` blocks, reference global vars in `styles/global.css`

**New Database Schema:**
- Location: `infrastructure/runtime/src/mneme/schema.ts`
- Pattern: Add DDL statements, increment SCHEMA_VERSION
- Migration: Add function to MIGRATIONS array
- Applies: Automatically on session store initialization

## Special Directories

**`~/.aletheia/`** — User configuration and data

- `aletheia.json` — Main configuration
- `credentials/` — API keys, credentials (encrypted if enabled)
- `encryption.salt` — Encryption key salt
- `sessions.db` — SQLite message and session store
- `agents/{nous-id}/` — Agent workspace (can use symlinks to external)

**`dist/`** — Build output

- Generated: Yes (built via `npx tsdown` in runtime)
- Committed: No (.gitignored)
- Contents: Transpiled JavaScript, ESM format

**`public/` (UI)** — Static assets for web UI

- Generated: No
- Committed: Yes
- Served: Via Vite dev server or bundled into final UI

**`test-results/`** — Vitest output

- Generated: Yes (from test runs)
- Committed: No
- Contains: Test reports, coverage data

## Import Ordering

**Standard pattern across codebase:**
```typescript
// 1. Node.js built-ins
import { join } from "node:path";
import { readFileSync } from "node:fs";

// 2. External packages
import { z } from "zod";
import Database from "better-sqlite3";

// 3. Internal absolute paths (@/ prefix if configured) — typically not used
// (this codebase prefers relative imports)

// 4. Relative imports (same directory, parent, siblings)
import { createLogger } from "../koina/logger.js";
import type { Session } from "./store.js";

// 5. .js extensions required in all imports (ESM, no bundler)
```

## Module Export Patterns

**Utilities (koina, etc.):**
- Named exports for functions, classes, types
- Example: `export function createLogger(name: string): Logger`

**Registries (ToolRegistry, SkillRegistry, PluginRegistry):**
- Class-based, methods to register and retrieve
- Example: `registry.registerTool(name, handler)`

**Handlers (Tool, Hook, Plugin):**
- Function factories returning handler object
- Example: `export const myTool = (): ToolHandler => ({ ... })`

**Schemas (Zod):**
- Named exports for each schema
- Re-export inferred types using `z.infer<typeof schema>`

---

*Structure analysis: 2026-02-23*
