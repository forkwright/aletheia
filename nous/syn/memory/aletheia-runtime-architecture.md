# Aletheia Runtime — Architecture Specification

*Clean-room design for a purpose-built distributed cognition runtime.*
*Author: Syn | Date: 2026-02-14*
*Status: DRAFT — awaiting audit validation*

---

## Design Principles

1. **Signal-only.** One channel, done well. No multi-channel abstraction layer.
2. **Agent-native.** The runtime exists to run persistent agents with identity, memory, and tools. Not a chat gateway that happens to have agents.
3. **Plugin-first for extensions.** Core is minimal. Memory, eval, attention — all plugins.
4. **Synchronous cross-agent.** Agents can ask each other questions and get answers in the same turn. This is the orchestration primitive.
5. **Observable.** Every agent turn, tool call, and cross-agent message is logged and auditable.
6. **Proprietar.** No inherited code. Clean IP.

---

## Module Architecture

### Core (must exist in runtime)

#### 1. Config (`config/`)
- Load `aletheia.json` from `~/.aletheia/`
- Agent definitions (id, name, workspace, model, tools)
- Binding rules (which agent handles which Signal peer)
- Plugin registration
- Runtime validation (`aletheia doctor`)
- **Config read API** — agents can read their own config (scoped, read-only)

#### 2. Agent Manager (`agents/`)
- Agent lifecycle: create, wake, sleep, compact
- Session management: persistent sessions with history
- Compaction pipeline: context summarization when approaching token limits
- Memory flush: pre-compaction distillation hook
- Bootstrap: inject SOUL.md, AGENTS.md, MEMORY.md, PROSOCHE.md, workspace files
- Model selection: primary + fallback chain
- Tool execution: dispatch tool calls, collect results, handle timeouts
- **Prompt caching:** leverage Anthropic cache headers for static bootstrap content

#### 3. Session Store (`sessions/`)
- Persistent JSONL transcript storage
- Session routing: map inbound messages to correct agent session
- Session types: main (Signal-bound), sub-agent (spawned, ephemeral), cross-agent
- History retrieval with token budgeting
- Compaction trigger and execution

#### 4. Tool Framework (`tools/`)
- Tool registry: built-in + plugin-provided tools
- Built-in tools:
  - `exec` — shell execution with PTY support
  - `read` / `write` / `edit` — file operations
  - `web_search` / `web_fetch` — web access
  - `browser` — Playwright browser control
  - `canvas` — A2UI canvas presentation
  - `message` — outbound messaging
  - `cron` — scheduled jobs
  - `mem0_search` — memory search (plugin-provided)
  - `config_read` — config inspection
  - `session_status` — usage/model info
  - `sessions_send` — fire-and-forget cross-agent
  - `sessions_ask` — **synchronous cross-agent with timeout** (NEW)
  - `sessions_spawn` — sub-agent execution
  - `sessions_list` / `sessions_history` — session inspection
  - `tts` — text to speech
  - `image` — vision model analysis
  - `nodes` — paired device control
- Tool result handling: size limits, smart truncation for large outputs
- Tool policy: per-agent allow/deny lists

#### 5. Provider Router (`providers/`)
- Anthropic (primary): Claude Opus, Sonnet, Haiku
- Fallback chain with automatic failover
- Rate limiting and cooldown management
- Prompt caching support (cache retention headers)
- Streaming response handling
- **Single concern:** translate tool calls + messages into provider API calls

#### 6. Signal Channel (`signal/`)
- Signal-CLI integration (daemon mode)
- Inbound message handling: DMs, groups, reactions, media
- Outbound: send text, media, reactions
- Group policy: allowlist
- DM policy: pairing
- Media handling: attachments, voice messages, images
- **No channel abstraction layer.** Direct Signal integration.

#### 7. Gateway Server (`gateway/`)
- HTTP server on configurable port
- WebSocket for real-time communication
- API endpoints:
  - Sessions (list, send, ask, spawn, history, status)
  - Agent management
  - Config read
  - Health check
- Authentication (token-based)
- Web UI (control panel, chat interface)

#### 8. CLI (`cli/`)
- `aletheia gateway start|stop|restart|status|logs`
- `aletheia doctor` — config validation
- `aletheia status` — system health
- Minimal command set. No wizard, no onboarding flow.

#### 9. Daemon (`daemon/`)
- Process management: start gateway, manage child processes
- Graceful shutdown
- Signal handling (SIGUSR1 for config reload)
- Port conflict detection

### Plugin System (`plugins/`)

Plugins extend the runtime without modifying core. Each plugin is a directory with a manifest and entry point.

#### Plugin API Surface:
- **Lifecycle hooks:** onSessionStart, onSessionEnd, onCompaction, onPreCompaction
- **Tool registration:** plugins can register new tools
- **HTTP routes:** plugins can add API endpoints
- **Memory hooks:** onMemoryFlush, onMemorySearch

#### Expected Plugins:
- `aletheia-memory` — Mem0 integration (search, extraction, compaction context)
- `aletheia-signal` — Signal channel (could be core or plugin — TBD)
- Future: eval plugins, bias monitoring hooks

### Supporting Modules

#### 10. Routing (`routing/`)
- Binding rules: match inbound messages to agents
- Peer identification (DM vs group, Signal UUID/group ID)
- Default agent fallback

#### 11. Cron (`cron/`)
- Job scheduler: at, every, cron expressions
- Job types: systemEvent (inject into session), agentTurn (run agent)
- Session targeting: main vs isolated
- Run history

#### 12. Media (`media/`)
- Inbound media handling (images, audio, documents)
- Outbound media (file paths, URLs)
- Transcription integration (whisper)

#### 13. Logging (`logging/`)
- Structured logging
- Per-agent log streams
- Langfuse integration for observability

#### 14. Security (`security/`)
- Gateway authentication
- Tool execution sandboxing (per-agent)
- File access controls

---

## Data Flow

```
Signal Message
    ↓
Signal Channel (inbound)
    ↓
Routing (binding match → agent ID)
    ↓
Session Store (find/create session)
    ↓
Agent Manager (bootstrap context + history)
    ↓
Provider Router (send to Anthropic)
    ↓
[Model responds with text + tool calls]
    ↓
Tool Framework (execute tools, collect results)
    ↓
[Loop until model produces final text response]
    ↓
Signal Channel (outbound)
```

### Cross-Agent Flow (sessions_ask)
```
Agent A calls sessions_ask(agentB, question, timeout=30s)
    ↓
Gateway injects question into Agent B's session
    ↓
Agent B processes, produces response
    ↓
Response returned to Agent A's tool call
    ↓
Agent A continues with the answer in context
```

---

## What This Eliminates

From the current OpenClaw runtime, the following are NOT in scope:

- WhatsApp/Baileys integration
- Discord/Slack/Telegram/Line/Matrix/Mattermost/Teams channels
- Channel abstraction layer (multi-channel routing)
- TUI (terminal UI)
- Onboarding wizard
- QR code pairing
- iOS/Android/macOS app support
- Pi RPC agent mode
- 30+ stripped extensions
- Multi-provider auth flows (GitHub Copilot, Google, etc.)

---

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | TypeScript | Existing expertise, Anthropic SDK is TS-native |
| Runtime | Node.js | Same as current, proven stable |
| Build | tsdown or esbuild | Fast, simple bundling |
| HTTP | Express or Hono | Lightweight, well-known |
| Signal | signal-cli (daemon) | Proven, stable, same as current |
| Browser | playwright-core | Same as current |
| Testing | vitest | Fast, TS-native |
| Config | JSON (aletheia.json) | Simple, no YAML complexity |

---

## Migration Path

This is NOT a big-bang rewrite. Phased approach:

### Phase 1: Core + Signal + Sessions
- Config loading
- Agent manager with session store
- Signal channel (direct integration, no abstraction)
- Provider router (Anthropic)
- Basic tool framework (exec, file ops)
- CLI (gateway start/stop)
- **Milestone:** Single agent responds to Signal messages

### Phase 2: Full Tool Suite + Plugins
- All built-in tools
- Plugin system with lifecycle hooks
- Memory plugin (Mem0)
- Cron scheduler
- **Milestone:** Feature parity with current deployment

### Phase 3: Cross-Agent + Advanced
- sessions_ask (synchronous)
- Sub-agent spawning
- Prompt caching optimization
- Smart tool result truncation
- Evaluation hooks
- **Milestone:** Full Aletheia capability, no OpenClaw code

### Phase 4: Cleanup
- Remove OpenClaw runtime directory
- Update all references
- Final licensing
- **Milestone:** Clean repo, proprietary codebase

---

## Open Questions

1. **Signal as core or plugin?** If Signal is the only channel, baking it into core is simpler. Plugin architecture adds indirection for no current benefit. But if we ever add a second channel...
2. **Session format?** Keep JSONL or move to SQLite? JSONL is simple but doesn't support efficient queries.
3. **Web UI scope?** Current control UI is basic. Do we invest here or keep Signal as the primary interface?
4. **Prompt caching specifics?** Need to investigate Anthropic's exact cache header behavior to maximize cache hits on bootstrap content.

---

*This document defines WHAT to build. The audit (separate doc) validates what we're replacing. Claude Code implements to this spec.*
