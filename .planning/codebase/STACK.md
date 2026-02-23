# Technology Stack

**Analysis Date:** 2026-02-23

## Languages

**Primary:**
- TypeScript 5.7.3 - Runtime, API server, TUI, core agent logic
- Python 3.11+ - Memory sidecar (Mem0), attention engine (Prosoche)
- Svelte 5 - UI frontend with reactive components

**Secondary:**
- Shell - CLI tooling, transcription wrapper via ffmpeg
- SQL - Session storage schema in SQLite WAL mode

## Runtime

**Environment:**
- Node.js 22 - Runtime target (tsdown)
- Python 3.11+ - FastAPI sidecars

**Package Manager:**
- npm - TypeScript dependencies and scripts
- uv (implied by project directives) - Python package management
- Lockfile: package-lock.json (present)

## Frameworks

**Core:**
- Hono 4.12.2 - HTTP routing, API server (`@hono/node-server@1.14.0`)
- Anthropic Claude SDK 0.78.0 - LLM provider integration
- Model Context Protocol (MCP) SDK 1.26.0 - Tool discovery and integration

**UI:**
- Svelte 5.53.0 - Component framework
- Vite 6.0.0 - Build and dev server
- Svelte Check 4.0.0 - Type checking

**Testing:**
- Vitest 4.0.18 - Unit and integration tests, coverage via v8
- jsdom 26.0.0 - DOM simulation for component tests
- @testing-library/svelte 5.0.0 - Component testing utilities

**Build/Dev:**
- tsdown 0.20.3 - TypeScript bundler, builds to single ESM module
- TypeScript 5.7.3 - Type checking
- oxlint 1.50.0 - Linting with auto-fix

**Python frameworks:**
- FastAPI 0.115.0+ - Memory sidecar and attention engine APIs
- Uvicorn[standard] 0.32.0+ - ASGI server for FastAPI

## Key Dependencies

**Critical:**
- better-sqlite3 12.6.2 - Local session storage, message history, execution state (WAL mode)
- @anthropic-ai/sdk 0.78.0 - Claude API access with streaming, prefix caching, OAuth support
- @modelcontextprotocol/sdk 1.26.0 - Tool registration and MCP server connection (stdio, HTTP, SSE transports)
- mem0ai[graph] 0.1.0+ - Memory extraction, entity graphs, temporal reasoning
- qdrant-client 1.12.0+ - Vector database for memory embeddings

**Infrastructure:**
- commander 14.0.3 - CLI argument parsing
- zod 3.24.2 - Configuration validation (schemas, runtime type checks)
- tslog 4.9.3 - Structured logging with AsyncLocalStorage context
- playwright-core 1.58.2 - Headless browser automation (optional, behind feature flag)
- dompurify 3.2.0 - XSS protection for markdown HTML rendering
- marked 15.0.0 - Markdown parsing and HTML conversion
- highlight.js 11.11.0 - Code syntax highlighting
- three.js 0.182.0 - 3D graph visualization
- force-graph 1.51.1, 3d-force-graph 1.79.1 - Graph layout and rendering
- CodeMirror 6.0.2+ - Code editor with language syntax highlighting (JS, Python, YAML, JSON, CSS, HTML, Markdown)
- neo4j 5.0.0+ - Knowledge graph storage for Mem0
- networkx 3.2.0+ - Graph algorithms for entity resolution and discovery

## Configuration

**Environment:**
- Config file: `~/.aletheia/aletheia.json` — validated via Zod schemas in `taxis/schema.ts`
- Env-based overrides supported for:
  - `ANTHROPIC_API_KEY` (x-api-key header)
  - `ANTHROPIC_AUTH_TOKEN` (OAuth bearer token for Max/Pro routing)
  - `BRAVE_API_KEY` (web search)
  - `OPENAI_API_KEY` (TTS fallback)
  - `ALETHEIA_*` (workspace, config, logging, memory)
  - MCP server env vars (substituted from config via `${VAR_NAME}`)

**Key configs required:**
- Agent workspace paths (absolute)
- Model specifications (primary + fallbacks)
- Tool profiles (minimal, coding, messaging, full)
- MCP server definitions (transport: stdio/http/sse, command, args, headers, env, timeout)
- Compaction settings (token limits, distillation model)
- Approval modes (autonomous/guarded/supervised)

**Build:**
- `tsconfig.json` - Strict mode, ES2022 target, isolated modules, exact optional properties
- `tsdown.config.ts` - Single-entry ESM bundler targeting Node22
- `vite.config.ts` - Svelte plugin, /ui/ base, /api proxy to localhost:18789
- `vitest.config.ts` - Coverage thresholds (80%+ statements/lines, 78%+ branches, 90%+ functions), fork pool (max 2 local, env-configurable for CI)

## Platform Requirements

**Development:**
- Node 22+
- Python 3.11+
- npm or compatible
- Optional: ffmpeg (transcription), whisper-cpp (local audio model), Piper (local TTS), Chromium/Playwright (browser automation)

**Production:**
- Node 22 runtime
- SQLite database support (WAL mode, file-based)
- Memory sidecar: Python 3.11+ FastAPI instance
- Prosoche attention engine: Python FastAPI instance
- Optional: MCP servers (stdio, HTTP, or SSE transports)

---

*Stack analysis: 2026-02-23*
