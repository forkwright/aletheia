# Technology Stack

**Analysis Date:** 2026-02-24

## Languages

**Primary:**
- TypeScript 5.7.3 - Runtime core, CLI, API gateway, web framework
- Python 3.11+ - Memory sidecar system, data pipelines, graph operations

**Secondary:**
- Svelte 5.53.0 - UI framework (experimental, v5)
- YAML - Configuration format, Docker compose

## Runtime

**Environment:**
- Node.js 22.12.0 (from `.nvmrc`)
- Python 3.11+ for memory/prosoche components

**Package Manager:**
- npm - Primary for Node.js projects
- Lockfile: `package-lock.json` present (inferred from Node.js conventions)
- Hatchling - Python package build (pyproject.toml)

## Frameworks

**Core:**
- Hono 4.12.2 - HTTP server framework for gateway and API
- @hono/node-server 1.14.0 - Hono server transport layer

**Testing:**
- Vitest 4.0.18 (runtime), 3.0.0 (ui) - Test runner, coverage
- @testing-library/svelte 5.0.0 - Svelte component testing
- jsdom 26.0.0 - DOM implementation for tests

**Build/Dev:**
- tsdown 0.20.3 - TypeScript-to-JavaScript bundler
- tsx 4.19.2 - TypeScript executor (dev mode)
- Vite 6.0.0 - Frontend build tool
- @sveltejs/vite-plugin-svelte 5.0.0 - Svelte Vite integration

**Linting/Formatting:**
- oxlint 1.50.0 - Fast linter (replaces ESLint)
- svelte-check 4.0.0 - Svelte type checking

## Key Dependencies

**Critical (Runtime):**
- @anthropic-ai/sdk 0.78.0 - Claude API integration, primary LLM provider
- @modelcontextprotocol/sdk 1.26.0 - MCP protocol for tool discovery
- better-sqlite3 12.6.2 - Session state storage, SQLite WAL mode
- zod 3.24.2 - Configuration validation, schema definitions
- tslog 4.9.3 - Structured logging with context propagation

**Infrastructure:**
- playwright-core 1.58.2 - Headless browser automation (no bundled browser)
- commander 14.0.3 - CLI argument parsing

**UI:**
- codemirror 6.0.2 - Code editor with language support (CSS, HTML, JS, JSON, Markdown, Python, YAML)
- force-graph 1.51.1 - 2D force-directed graph visualization
- 3d-force-graph 1.79.1 - 3D force-directed graph visualization
- three 0.182.0 - 3D graphics library (used by 3d-force-graph)
- marked 15.0.0 - Markdown parser
- highlight.js 11.11.0 - Syntax highlighting
- dompurify 3.2.0 - HTML sanitization

## Configuration

**Environment:**
- `.env.example` present at root (primary for local dev)
- `shared/config/aletheia.env` - systemd EnvironmentFile format (production)
- `~/.aletheia/aletheia.json` - Runtime configuration (JSON, Zod-validated)

**Build:**
- `infrastructure/runtime/tsconfig.json` - TypeScript config
- `infrastructure/runtime/vitest.config.ts` - Test config
- `infrastructure/runtime/vitest.fast.config.ts` - Fast test subset
- `infrastructure/runtime/vitest.integration.config.ts` - Integration test config
- `ui/vite.config.ts` - Frontend build
- `ui/vitest.config.ts` - UI test config

## Platform Requirements

**Development:**
- Linux/Unix environment (Chromium path references)
- Node.js 22.12.0
- Python 3.11+
- Docker & Docker Compose (for infrastructure services)

**Production:**
- Linux container or native host
- Signal Protocol support via signal-cli service (docker-compose)
- Neo4j 2026-community (graph database)
- Qdrant v1.17.0 (vector database)
- SQLite (embedded session store)

**Services:**
- Anthropic API (LLM)
- Voyage AI API (optional embeddings, env var `VOYAGE_API_KEY`)
- Brave Search API (optional web search, env var `BRAVE_API_KEY`)
- Perplexity API (optional research, env var `PERPLEXITY_API_KEY`)

## Deployment

**Container Setup:**
- `docker-compose.yml` at root - Signal-cli service
- `infrastructure/memory/docker-compose.yml` - Qdrant + Neo4j
- Systemd service unit: `aletheia.service` on production host

**Build Output:**
- Runtime: Built to `dist/` via tsdown
- UI: Built to `ui/dist/` via Vite

---

*Stack analysis: 2026-02-24*
