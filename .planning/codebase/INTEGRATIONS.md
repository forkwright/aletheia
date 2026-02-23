# External Integrations

**Analysis Date:** 2026-02-23

## APIs & External Services

**LLM Providers:**
- Anthropic Claude API - Primary agent backbone
  - SDK: `@anthropic-ai/sdk` 0.78.0
  - Auth: `ANTHROPIC_API_KEY` (x-api-key header) or `ANTHROPIC_AUTH_TOKEN` (OAuth 2025-04-20 beta for Max/Pro routing)
  - Features: Streaming messages, prefix caching (4 breakpoints), thinking (extended), token counting, context management (2025-06-27 beta)
  - Models supported: claude-opus-4-6 (default), claude-sonnet-4-6, claude-haiku-4-5-20251001, configurable fallbacks
  - Error handling: Distinguishes 429 (rate limit, 60s backoff), 529 (overloaded, 30s backoff), 401/403 (auth), 5xx (recoverable)
  - Implementation: `infrastructure/runtime/src/hermeneus/anthropic.ts`

**Web Search:**
- Brave Search API - Current information lookup
  - SDK: Native fetch via `https://api.search.brave.com/res/v1/web/search`
  - Auth: `BRAVE_API_KEY` (X-Subscription-Token header)
  - Query params: q (query), count (max 20, default 5)
  - Returns: Title, URL, description, result age
  - Timeout: 10 seconds
  - Implementation: `infrastructure/runtime/src/organon/built-in/brave-search.ts`

**Text-to-Speech:**
- OpenAI TTS API - Speech synthesis (primary)
  - Endpoint: `https://api.openai.com/v1/audio/speech`
  - Auth: `OPENAI_API_KEY` (Bearer token)
  - Model: tts-1 (real-time synthesis)
  - Config: voice (alloy default), speed (1.0 default), response_format (mp3), max input 4096 chars
  - Timeout: 30 seconds
  - Fallback: Piper (local binary)
  - Implementation: `infrastructure/runtime/src/semeion/tts.ts`

**Speech Synthesis (Local):**
- Piper - Local TTS fallback when OpenAI unavailable
  - Binary: `PIPER_BIN` (default: /usr/local/bin/piper)
  - Model: `PIPER_MODEL` (default: /usr/local/share/piper/en_US-lessac-medium.onnx)
  - Input: Text (max 4096 chars)
  - Output: WAV files
  - Timeout: 30 seconds
  - Implementation: `infrastructure/runtime/src/semeion/tts.ts`

## Data Storage

**Databases:**
- SQLite (better-sqlite3) - Session memory, message history, execution state
  - Connection: File-based, WAL mode enabled
  - Client: `better-sqlite3` 12.6.2 (`Database` class)
  - Schema: `infrastructure/runtime/src/mneme/schema.ts`
  - Tables: sessions, messages, agent_notes, queued_messages, usage_records, execution_plans
  - Encryption: Optional (configurable in `aletheia.json`)
  - Location: `~/.aletheia/sessions.db` (default)

**Vector Database:**
- Qdrant - Memory embeddings and semantic search
  - Client: `qdrant-client` 1.12.0+
  - Purpose: Memory extraction, entity similarity, context retrieval
  - Deployed as: Sidecar service (memory sidecar FastAPI)
  - Config: Mem0 integration in sidecar

**Knowledge Graphs:**
- Neo4j - Entity graphs, relationship mapping
  - Client: `neo4j` 5.0.0+
  - Purpose: Entity resolution, relationship discovery, temporal reasoning
  - Deployed as: Sidecar service (memory sidecar FastAPI)
  - Config: Mem0 integration in sidecar

**File Storage:**
- Local filesystem only
  - TTS output: `ALETHEIA_TTS_DIR` (default: `/tmp/aletheia-tts-{random}`)
  - Audio transcription: Temp files in `/tmp/aletheia-audio-{random}`
  - Cleanup: Auto-removal of files >1 hour old (TTS)

**Caching:**
- In-memory (via AsyncLocalStorage for context during request processing)
- Prefix caching via Anthropic API (ephemeral cache control on system prompt, tools, conversation history)
- No external cache service (Redis, Memcached) currently integrated

## Authentication & Identity

**Auth Provider:**
- Custom (no third-party OAuth/OIDC)
- Implementation: Token-based or session-based depending on `auth.mode` in config
- Modes: none (local only), token (static token), session (multi-user sessions)
- Token generation: Random 24-byte hex (48 chars), stored in config
- Route: `infrastructure/runtime/src/pylon/routes/auth.ts`

**Bearer Token:**
- When `auth.mode: token`, requests include `Authorization: Bearer {token}` header
- No RBAC or permissions model; token grants full access

## Monitoring & Observability

**Error Tracking:**
- None detected — errors are logged locally

**Logs:**
- Local file-based via tslog
  - Config: `ALETHEIA_LOG_LEVEL`, `ALETHEIA_LOG_JSON`, `ALETHEIA_LOG_MODULES`
  - Format: Structured JSON when `ALETHEIA_LOG_JSON=true`
  - Location: Console output (no log file path detected)
  - Per-module loggers: `createLogger("module-name")` with AsyncLocalStorage context

**Metrics:**
- Token usage tracking: Stored in SQLite `usage_records` table (input, output, cache read/write tokens per turn)
- No external metrics service (Datadog, CloudWatch, Prometheus)

## CI/CD & Deployment

**Hosting:**
- Not detected — self-hosted runtime

**CI Pipeline:**
- None detected in codebase

**Deployment Model:**
- Standalone binary: `npm run build` produces `dist/entry.js` (ESM, Node 22 target)
- CLI: `aletheia init` (setup), `aletheia run` (daemon)
- Configuration: First-run interactive setup wizard
- Database: SQLite file lives in `~/.aletheia/`

## Environment Configuration

**Required env vars:**
- `ANTHROPIC_API_KEY` - Claude API access (or `ANTHROPIC_AUTH_TOKEN` for OAuth)
- `ALETHEIA_ROOT` - Workspace root directory (absolute path)
- `ALETHEIA_CONFIG_DIR` - Config directory location (defaults to `~/.aletheia/`)

**Optional env vars:**
- `BRAVE_API_KEY` - Brave Search API (for web_search tool)
- `OPENAI_API_KEY` - OpenAI TTS (tts-1 model)
- `WHISPER_MODEL_PATH` - Local audio transcription model (whisper-cpp)
- `WHISPER_BINARY` - whisper-cpp binary path (default: whisper-cpp)
- `PIPER_BIN` - Piper TTS binary (default: /usr/local/bin/piper)
- `PIPER_MODEL` - Piper TTS model path (default: /usr/local/share/piper/en_US-lessac-medium.onnx)
- `ALETHEIA_TTS_DIR` - TTS output directory
- `ALETHEIA_MEMORY_URL` - Memory sidecar endpoint (e.g., http://localhost:8001)
- `ALETHEIA_MEMORY_KEY` - Memory sidecar auth key
- `ALETHEIA_PII_HASH_SALT` - PII anonymization salt
- `ALETHEIA_LOG_LEVEL` - Log verbosity (debug, info, warn, error)
- `ALETHEIA_LOG_JSON` - Structured logging format
- `ALETHEIA_PLUGIN_ROOT` - Plugin directory for custom tools
- `ENABLE_BROWSER` - Enable Playwright-based browser automation
- `CHROMIUM_PATH` - Path to Chromium/Chrome binary

**Secrets location:**
- `~/.aletheia/aletheia.json` - Main config (not secrets, but contains sensitive defaults)
- Environment variables - Primary source for secrets (API keys, tokens)
- `.env` files - Not used; no `.env.example` or `.env.local` detected

## Webhooks & Callbacks

**Incoming:**
- HTTP POST `/api/sessions/{sessionId}/turn` - Send message to agent session
- HTTP GET `/api/sessions/{sessionId}/events` - SSE stream (tool calls, streaming deltas, completion)
- HTTP POST `/api/mcp/servers/{name}/reconnect` - Reconnect MCP server

**Outgoing:**
- None detected — MCP servers are pulled (client-side discovery), not pushed

## MCP (Model Context Protocol) Integration

**Transport Types:**
- Stdio - Local process spawning (command + args + env)
- HTTP - HTTP endpoints with headers and auth
- SSE - Server-Sent Events for streaming
- Configuration: `mcp.servers` in `aletheia.json`

**MCP Client:**
- SDK: `@modelcontextprotocol/sdk` 1.26.0
- Transports: `StdioClientTransport`, `SSEClientTransport`, `StreamableHTTPClientTransport`
- Tool registration: Discovered tools registered into `ToolRegistry`
- Connection management: Automatic reconnect, connection pooling
- Implementation: `infrastructure/runtime/src/organon/mcp-client.ts`

**Tool Discovery:**
- Tools from MCP servers merged with built-in tools (web_search, web_fetch, browser, code_runner, etc.)
- Tool profiles: minimal, coding, messaging, full
- Allowlist/denylist per agent config

## Memory Sidecar (Python FastAPI)

**Service:**
- FastAPI 0.115.0+
- Uvicorn[standard] 0.32.0+ server
- Auth: `ALETHEIA_MEMORY_KEY` header-based
- Endpoint: `ALETHEIA_MEMORY_URL` (default: http://localhost:8001)

**Backends:**
- Mem0 0.1.0+ with graph module
- LLM: Three-tier fallback (Anthropic OAuth > Anthropic API key > Ollama > embedding-only)
- Vector DB: Qdrant
- Knowledge Graph: Neo4j
- Integration: Tool-calling bridge between Mem0 and Anthropic SDK

**Routes:**
- `/api/memory/add` - Extract and store memory
- `/api/memory/search` - Semantic search
- `/api/memory/graph` - Entity relationships
- `/api/foresight/*` - Prediction and proactive suggestions
- `/api/discovery/*` - Entity discovery and resolution
- `/api/evolution/*` - Memory evolution and updating
- `/api/temporal/*` - Time-aware memory retrieval

## Attention Engine (Prosoche)

**Service:**
- Python 3.11+ FastAPI
- anyio 4.0+, httpx 0.27+, loguru 0.7+, msgspec 0.19+, pyyaml 6.0+
- Purpose: Adaptive attention scheduling, turn prioritization
- Deployed as: Optional sidecar service

**Signals:**
- Calendar signals - Schedule and availability
- Health signals - Agent well-being metrics
- Memory signals - Memory load and freshness
- Task signals - Current task context
- Hex signals - Dashboard integration

---

*Integration audit: 2026-02-23*
