# Codebase Structure

**Analysis Date:** 2026-02-24

## Directory Layout

```
aletheia/
├── infrastructure/
│   ├── runtime/                    # TypeScript gateway (tsdown compiled)
│   │   ├── src/
│   │   │   ├── aletheia.ts         # Main orchestrator
│   │   │   ├── entry.ts            # CLI entry point
│   │   │   ├── version.ts          # Version/build info
│   │   │   ├── auth/               # Auth sessions, audit logging
│   │   │   ├── daemon/             # Cron, watchdog, update checker
│   │   │   ├── distillation/       # Context compression pipeline
│   │   │   ├── hermeneus/          # Provider router, token counter
│   │   │   ├── koina/              # Shared utilities (errors, logger, events)
│   │   │   ├── mneme/              # SQLite session store
│   │   │   ├── nous/               # Agent bootstrap, pipeline orchestration
│   │   │   │   ├── manager.ts      # Turn coordination
│   │   │   │   ├── pipeline/       # Message processing stages
│   │   │   │   │   ├── runner.ts   # Stage composer
│   │   │   │   │   ├── stages/     # Individual stages (resolve, guard, context, etc.)
│   │   │   │   │   └── types.ts    # Turn types
│   │   │   │   └── roles/          # Role-based system prompts
│   │   │   ├── organon/            # Tool registry
│   │   │   │   ├── registry.ts     # Tool registration
│   │   │   │   ├── built-in/       # 41 built-in tools
│   │   │   │   ├── skills.ts       # Skill registry
│   │   │   │   └── approval.ts     # Tool approval gates
│   │   │   ├── pylon/              # HTTP gateway (Hono)
│   │   │   │   ├── server.ts       # HTTP server setup
│   │   │   │   ├── mcp.ts          # MCP protocol handler
│   │   │   │   ├── ui.ts           # Static UI, SSE events
│   │   │   │   └── routes/         # REST API endpoints
│   │   │   ├── prostheke/          # Plugin system
│   │   │   ├── semeion/            # Signal integration
│   │   │   │   ├── client.ts       # signal-cli RPC
│   │   │   │   ├── listener.ts     # Message listener
│   │   │   │   ├── sender.ts       # Message sender
│   │   │   │   └── commands.ts     # Signal command registry
│   │   │   ├── taxis/              # Configuration
│   │   │   │   ├── schema.ts       # Zod schemas
│   │   │   │   ├── loader.ts       # Config loading
│   │   │   │   └── paths.ts        # Path resolution
│   │   │   └── portability/        # Data export/import
│   │   ├── package.json
│   │   ├── tsconfig.json
│   │   ├── vitest.config.ts
│   │   └── tsdown.config.ts
│   ├── memory/                     # Mem0 sidecar (Python FastAPI)
│   │   ├── sidecar/                # FastAPI memory service
│   │   ├── aletheia-memory/        # Letta plugin
│   │   └── scripts/                # Setup/migration scripts
│   ├── prosoche/                   # Adaptive attention daemon
│   ├── langfuse/                   # Observability (self-hosted)
│   └── evaluation/                 # Testing scenarios
├── ui/                             # Svelte 5 web interface
│   ├── src/
│   │   ├── components/
│   │   │   ├── chat/               # Message view, input, thinking, tools
│   │   │   ├── agents/             # Agent selection, status
│   │   │   ├── sessions/           # Session list, management
│   │   │   ├── layout/             # TopBar, main layout
│   │   │   ├── graph/              # Memory graph visualization
│   │   │   ├── metrics/            # Token usage, costs
│   │   │   └── shared/             # Spinner, toast, badge
│   │   ├── lib/                    # API client, utilities
│   │   ├── routes/                 # SvelteKit page routes
│   │   ├── App.svelte              # Root component
│   │   └── app.css                 # Global styles
│   ├── index.html
│   └── package.json
├── nous/                           # Agent workspaces (templates)
│   └── _example/                   # Example agent
│       ├── SOUL.md                 # Character description
│       ├── AGENTS.md               # Operations & tools
│       ├── MEMORY.md               # Memory strategy
│       ├── IDENTITY.md             # Name, emoji, identity
│       ├── CONTEXT.md              # Domain context
│       ├── GOALS.md                # Agent objectives
│       ├── USER.md                 # User relationship
│       ├── TOOLS.md                # Tool setup
│       └── PROSOCHE.md             # Attention profile
├── shared/                         # Common assets
│   ├── skills/                     # Reusable skill scripts
│   ├── bin/                        # Shared utilities
│   ├── config/                     # Config templates, examples
│   ├── schemas/                    # Reusable Zod schemas
│   ├── templates/                  # Markdown templates
│   ├── commands/                   # Command definitions
│   └── hooks/                      # Git hooks
├── config/                         # Configuration examples
│   └── aletheia.example.json       # Config template
├── docs/                           # Documentation
│   ├── QUICKSTART.md               # Setup guide
│   ├── CONFIGURATION.md            # Config reference
│   ├── DEVELOPMENT.md              # Dev workflow
│   ├── DEPLOYMENT.md               # Production guide
│   ├── API.md                      # REST API docs
│   └── specs/                      # Design specs
├── README.md                       # Project overview
├── ALETHEIA.md                     # Philosophy & naming
├── CONTRIBUTING.md                # Contribution guide
├── CHANGELOG.md                    # Version history
├── CLAUDE.md                       # AI coding conventions
├── llms.txt                        # AI navigation index
└── setup.sh                        # One-command setup
```

## Directory Purposes

**`infrastructure/runtime/src/`:**
- Purpose: Core TypeScript gateway, compiled to single ~450KB bundle via tsdown
- Contains: All runtime modules, entry point, orchestration
- Key files: `aletheia.ts` (orchestrator), `entry.ts` (CLI)

**`infrastructure/runtime/src/koina/`:**
- Purpose: Shared utilities and cross-cutting infrastructure
- Contains: Error classes, logger, event bus, encryption, crypto, safe wrappers, diagnostics
- Key files: `errors.ts` (AletheiaError hierarchy), `logger.ts` (structured logging), `event-bus.ts` (pub/sub)

**`infrastructure/runtime/src/nous/pipeline/`:**
- Purpose: Turn processing stages
- Contains: Stage implementations (resolve, guard, context, history, execute, finalize), runner, types
- Key files: `runner.ts` (stage composer), `stages/execute.ts` (LLM + tool handling)

**`infrastructure/runtime/src/organon/built-in/`:**
- Purpose: Built-in tool implementations
- Contains: File tools (read, write, edit, grep, find, ls), web tools (fetch, search), memory tools, session tools, approval, planning
- Key files: 41 .ts files, each tool has corresponding .test.ts

**`infrastructure/runtime/src/pylon/routes/`:**
- Purpose: REST API endpoint handlers
- Contains: Per-endpoint logic (sessions, agents, costs, auth, workspace, etc.)
- Key files: `sessions.ts` (message routing), `agents.ts` (agent list), `system.ts` (health/status)

**`ui/src/components/`:**
- Purpose: Svelte component library
- Contains: Chat view, agent selector, session list, thinking display, tool approval, memory graph
- Pattern: One component per file, co-located styles

**`nous/`:**
- Purpose: Agent workspace templates
- Contains: Example agent directory structure
- Usage: Copy `_example` to create new agents, populate SOUL.md, AGENTS.md, MEMORY.md

**`shared/`:**
- Purpose: Shared tooling and templates
- Contains: Skills (reusable functions), scripts, config templates, schemas
- Key files: `skills/` (200+ KB of skill definitions), `config/` (example configs)

## Key File Locations

**Entry Points:**
- `infrastructure/runtime/src/entry.ts`: CLI argument parsing, command routing
- `infrastructure/runtime/src/aletheia.ts`: Runtime orchestration, module wiring
- `ui/src/App.svelte`: Web UI root component
- `infrastructure/runtime/aletheia.mjs`: Compiled entry point (generated by tsdown)

**Configuration:**
- `infrastructure/runtime/src/taxis/schema.ts`: Zod config schemas
- `infrastructure/runtime/src/taxis/loader.ts`: Config file loading
- `~/.aletheia/aletheia.json`: User config location (production)
- `.env`: Environment variables (see `.env.example`)

**Core Logic:**
- `infrastructure/runtime/src/nous/manager.ts`: Agent session management, turn coordination
- `infrastructure/runtime/src/nous/pipeline/runner.ts`: Pipeline stage composition
- `infrastructure/runtime/src/nous/pipeline/stages/execute.ts`: LLM streaming, tool dispatch
- `infrastructure/runtime/src/organon/registry.ts`: Tool registry and execution
- `infrastructure/runtime/src/mneme/store.ts`: SQLite session persistence
- `infrastructure/runtime/src/distillation/pipeline.ts`: Context compression

**Testing:**
- `infrastructure/runtime/src/**/*.test.ts`: Co-located test files (vitest)
- `infrastructure/runtime/vitest.config.ts`: Test runner config
- `infrastructure/runtime/vitest.fast.config.ts`: Parallel test config
- `ui/**/*.test.ts`: UI component tests

**API Routes:**
- `infrastructure/runtime/src/pylon/routes/sessions.ts`: Message send/stream, session CRUD
- `infrastructure/runtime/src/pylon/routes/agents.ts`: Agent list, config, status
- `infrastructure/runtime/src/pylon/routes/costs.ts`: Token usage and billing
- `infrastructure/runtime/src/pylon/routes/system.ts`: Health, version, metrics

## Naming Conventions

**Files:**
- Modules: `kebab-case.ts` (e.g., `session-store.ts`)
- Components: `PascalCase.svelte` (e.g., `ChatView.svelte`)
- Tests: `{filename}.test.ts` (e.g., `session-store.test.ts`)
- Built-in tools: `kebab-case.ts` (e.g., `web-search.ts`)

**Directories:**
- Modules: `lowercase` (e.g., `koina/`, `nous/`)
- Features: `lowercase` (e.g., `pipeline/`, `stages/`)
- Components: `lowercase` (e.g., `components/chat/`)

**Exports:**
- Functions/classes: camelCase or PascalCase depending on type
- Constants: UPPER_SNAKE_CASE
- Types: PascalCase (interfaces, types, enums)

**Greek Module Names:**
- `koina`: Common utilities
- `taxis`: Configuration and ordering
- `mneme`: Memory store
- `hermeneus`: Provider interpretation
- `nous`: Agent minds
- `organon`: Tools (instruments of thought)
- `semeion`: Signs and communication
- `pylon`: Gateway entrance
- `prostheke`: Plugin additions
- `distillation`: Context compression

## Where to Add New Code

**New Feature (e.g., email integration):**
- Primary code: `infrastructure/runtime/src/semeion/email.ts` (follow pattern of signal handler)
- Tests: `infrastructure/runtime/src/semeion/email.test.ts`
- Integration: Wire in `infrastructure/runtime/src/aletheia.ts` with other listeners
- Config: Add schema to `infrastructure/runtime/src/taxis/schema.ts`

**New Tool (e.g., calculator):**
- Implementation: `infrastructure/runtime/src/organon/built-in/calculator.ts`
- Tests: `infrastructure/runtime/src/organon/built-in/calculator.test.ts`
- Registration: Export from `organon/built-in/` and add to registry in `aletheia.ts`
- Docs: Add to `TOOLS.md` in agent workspace

**New Pipeline Stage (e.g., rate limiter):**
- Implementation: `infrastructure/runtime/src/nous/pipeline/stages/rate-limit.ts`
- Tests: `infrastructure/runtime/src/nous/pipeline/stages/rate-limit.test.ts`
- Integration: Add to `runner.ts` between appropriate existing stages
- Types: Add to `types.ts` if new event types needed

**New API Route (e.g., /api/goals):**
- Implementation: `infrastructure/runtime/src/pylon/routes/goals.ts`
- Tests: `infrastructure/runtime/src/pylon/routes/goals.test.ts`
- Registration: Import in `infrastructure/runtime/src/pylon/server.ts`
- Docs: Add to `docs/API.md`

**New UI Page (e.g., goals management):**
- Route: `ui/src/routes/goals/+page.svelte`
- Components: `ui/src/components/goals/GoalList.svelte`, etc.
- Styles: Co-locate in component files or shared CSS
- Tests: `ui/src/routes/goals/+page.test.ts`

**Utilities / Helpers:**
- Internal to module: `{module}/{filename}.ts`
- Shared utilities: `infrastructure/runtime/src/koina/{filename}.ts`
- Type definitions: `infrastructure/runtime/src/{module}/types.ts`

## Special Directories

**`infrastructure/runtime/dist/`:**
- Purpose: Compiled output from tsdown
- Generated: Yes (via `npx tsdown` build)
- Committed: No (gitignored, rebuilt on deploy)

**`infrastructure/runtime/coverage/`:**
- Purpose: Test coverage reports
- Generated: Yes (via `npx vitest run --coverage`)
- Committed: No

**`ui/dist/`:**
- Purpose: Compiled UI bundle
- Generated: Yes (via `npm run build`)
- Committed: No

**`nous/` (user workspaces):**
- Purpose: Agent workspace instances
- Generated: No (user creates by copying _example)
- Committed: Yes (part of config)
- Structure: Each agent gets a directory with SOUL.md, AGENTS.md, etc.

**`shared/skills/`:**
- Purpose: Reusable skill implementations
- Generated: No
- Committed: Yes (versioned with runtime)
- Usage: Loaded by agents, callable as tools

**`docs/specs/`:**
- Purpose: Design specifications and RFC docs
- Generated: No
- Committed: Yes
- Pattern: Numbered by implementation order (e.g., `001-streaming.md`, `002-distillation.md`)

---

*Structure analysis: 2026-02-24*
