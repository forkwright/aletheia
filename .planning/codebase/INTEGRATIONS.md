# External Integrations

**Analysis Date:** 2026-02-24

## APIs & External Services

**LLM & AI:**
- Anthropic Claude - Primary LLM provider
  - SDK: @anthropic-ai/sdk 0.78.0
  - Auth: `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN` from env
  - Models used: claude-opus-4-6, claude-sonnet-4-6, claude-haiku-4-5-20251001
  - Integration: `infrastructure/runtime/src/hermeneus/anthropic.ts`

**Embeddings:**
- Voyage AI (optional) - Vector embeddings for memory
  - SDK: OpenAI-compatible client (used via httpx)
  - Auth: `VOYAGE_API_KEY` from env
  - Model: voyage-4-large (1024 dims)
  - Integration: `infrastructure/memory/sidecar/aletheia_memory/config.py`

**Search:**
- Brave Search API (optional) - Web search tool
  - Auth: `BRAVE_API_KEY` from env
  - Integration: `infrastructure/runtime/src/organon/built-in/brave-search.ts`

- Perplexity API (optional) - Research tool
  - Auth: `PERPLEXITY_API_KEY` from env
  - Integration: shared tools via env config

**Messaging & Communication:**
- Signal Protocol - SMS-like messaging backend
  - Service: signal-cli (Docker container in `docker-compose.yml`)
  - Transport: JSON-RPC via HTTP at localhost:8080
  - Configuration: `config/aletheia.example.json` channels.signal section
  - Policy: DM pairing, group allowlist

**MCP (Model Context Protocol):**
- Protocol: @modelcontextprotocol/sdk 1.26.0
- Transports: stdio, HTTP, SSE
- Purpose: Tool discovery and registration
- Implementation: `infrastructure/runtime/src/organon/mcp-client.ts`
- Built-in integrations: Aletheia memory plugin at `infrastructure/memory/aletheia-memory`

## Data Storage

**Databases:**
- **Neo4j 2026-community** - Semantic graph store
  - Connection: `NEO4J_URL` env (default: neo4j://localhost:7687)
  - Auth: `NEO4J_USER` (neo4j), `NEO4J_PASSWORD` env
  - Python client: neo4j driver
  - Usage: Memory graph, entity relationships, fact tracking
  - Config: `infrastructure/memory/docker-compose.yml`
  - Plugins: APOC enabled

- **Qdrant v1.17.0** - Vector database
  - Connection: `QDRANT_HOST:QDRANT_PORT` (localhost:6333)
  - Python client: qdrant-client
  - Usage: Semantic memory embeddings, similarity search
  - Collection: `aletheia_memories`
  - Storage: `/mnt/ssd/aletheia/data/qdrant` on production
  - Config: `infrastructure/memory/docker-compose.yml`

- **SQLite** - Session state store
  - Client: better-sqlite3 12.6.2
  - Mode: WAL for concurrency
  - Usage: Session management, audit logs
  - Location: Embedded in runtime process
  - Implementation: `infrastructure/runtime/src/mneme/store.ts`, `infrastructure/runtime/src/auth/`

**File Storage:**
- Local filesystem only (no S3/cloud storage)
- Configuration root: `ALETHEIA_ROOT` env variable

**Caching:**
- None detected (Hono context-level caching for HTTP)

## Authentication & Identity

**Auth Provider:**
- Custom multi-mode implementation
- Modes: none (local), token (API key), session (multi-user)
- Implementation: `infrastructure/runtime/src/auth/`
- Features:
  - Token-based API auth
  - Session cookies (secure mode configurable)
  - Audit trail tracking
  - Refresh token rotation

**OAuth (Anthropic):**
- Optional OAuth token support for Anthropic API
- Code: `infrastructure/runtime/src/hermeneus/router.ts`
- Token storage: Env var `ANTHROPIC_AUTH_TOKEN`

## Monitoring & Observability

**Error Tracking:**
- None detected (structured logging only)

**Logs:**
- tslog 4.9.3 - Structured JSON logging
- AsyncLocalStorage context propagation
- Turn ID tracking for request correlation
- Output: stdout, optional file appending
- Implementation: `infrastructure/runtime/src/koina/logger.ts`

**Watchdog/Health:**
- Configuration-based service health checks
- Monitors: memory-sidecar, Qdrant
- Implementation: `infrastructure/runtime/src/daemon/watchdog.ts`

## CI/CD & Deployment

**Hosting:**
- Self-hosted Linux server (worker-node 192.168.0.29)
- Systemd service: `aletheia.service`
- Alternative: Docker via systemd or compose

**CI Pipeline:**
- None detected in codebase
- Likely GitHub Actions (`.github/` directory exists)

## Environment Configuration

**Required env vars:**
- `ANTHROPIC_API_KEY` - Claude API key (required for LLM)
- `ALETHEIA_ROOT` - Installation root directory

**Optional env vars:**
- `VOYAGE_API_KEY` - Voyage AI embeddings
- `BRAVE_API_KEY` - Brave Search
- `PERPLEXITY_API_KEY` - Perplexity research
- `NEO4J_URL`, `NEO4J_USER`, `NEO4J_PASSWORD` - Graph database
- `QDRANT_HOST`, `QDRANT_PORT` - Vector database
- `PROSOCHE_GATEWAY_TOKEN`, `PROSOCHE_CALENDAR_*` - Scheduler daemon
- `CHROMIUM_PATH` - Headless browser executable path
- `RESEARCH_EMAIL` - For Perplexity/research tools

**Secrets location:**
- Environment files: `.env.example`, `shared/config/aletheia.env` (systemd format)
- Runtime config: `~/.aletheia/aletheia.json` (validated via Zod)

## Webhooks & Callbacks

**Incoming:**
- Signal protocol messages routed to gateway
- HTTP endpoints for each agent
- MCP tool callbacks from external servers

**Outgoing:**
- Sessions dispatch: `sessions-dispatch` tool sends messages to Signal
- Memory updates: Graph mutations to Neo4j
- Vector upserts: Qdrant collection updates

## Browser Automation

**Technology:**
- playwright-core 1.58.2 (headless only, no bundled browser)
- Chromium executable path: env `CHROMIUM_PATH` (default `/usr/bin/chromium-browser`)
- Sandbox disabled in production
- Max concurrent pages: 3
- Page timeout: 30 seconds

**Integration:**
- `infrastructure/runtime/src/organon/built-in/browser.ts`
- Supports navigation, screenshots, DOM interaction
- SSRF guard checks URLs before access
- Used by agents for web interaction

## Tool System

**MCP Clients:**
- Configured via aletheia.json plugins section
- Built-in memory plugin at `infrastructure/memory/aletheia-memory`
- Custom command execution via stdin/stdio
- Tool registry with naming conventions

**Built-in Tools:**
- File operations: read, write, edit, find, grep, ls
- Shell execution: exec with safety checks
- Web: web-fetch, web-search, browser, brave-search
- Memory: mem0-search, mem0-audit, memory-forget, memory-correct
- Sessions: sessions-ask, sessions-dispatch, sessions-send
- Research: research tool (Perplexity)
- Graph queries: various Mem0/Neo4j specific tools

---

*Integration audit: 2026-02-24*
