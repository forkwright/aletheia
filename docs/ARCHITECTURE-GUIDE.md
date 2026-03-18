# Architecture walkthrough

A guided tour for new contributors. For the reference module map and dependency graph, see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## What aletheia is

Aletheia is a self-hosted multi-agent AI system. Multiple AI agents run as persistent actors, each with character, memory, and domain expertise. They communicate via Signal, HTTP API, or TUI, and persist understanding across sessions.

It is not a chatbot framework. It is a distributed cognition system: agents have identity (SOUL.md), evolve through use (MEMORY.md), and coordinate through shared infrastructure.

The entire system compiles to a single Rust binary. No Node.js, no Python sidecar, no external databases required.

---

## The binary

When you run `aletheia`, the `main.rs` entrypoint performs a sequential initialization:

1. **Oikos discovery**: finds the instance directory (`./instance`, `ALETHEIA_ROOT`, or `--instance-root`)
2. **Config cascade**: loads compiled defaults, then YAML file, then environment variables (taxis crate)
3. **Domain packs**: loads external knowledge packs declared in config (thesauros crate)
4. **Session store**: opens SQLite database at `instance/data/sessions.db` (mneme crate)
5. **JWT manager**: initializes authentication (symbolon crate)
6. **Provider registry**: registers LLM providers (Anthropic if `ANTHROPIC_API_KEY` is set) (hermeneus crate)
7. **Tool registry**: registers built-in tools: write, edit, exec, web_search, etc. (organon crate)
8. **Embedding provider**: creates local embedding engine for recall (mneme crate)
9. **Nous actors**: spawns one Tokio actor per configured agent (nous crate)
10. **Daemon**: starts background maintenance: trace rotation, drift detection, DB monitoring (oikonomos crate)
11. **Channel listeners**: starts Signal message polling if configured (agora crate)
12. **HTTP gateway**: starts Axum server on configured port (pylon crate)

Then it waits for SIGTERM or Ctrl+C and shuts down gracefully.

---

## What happens when a message arrives

### Via signal

```
Signal app → Signal servers (E2E encrypted) → signal-cli daemon (localhost JSON-RPC)
    → ChannelListener polls signal-cli → InboundMessage
    → MessageRouter matches bindings → routes to NousActor
```

### Via HTTP API

```
POST /api/v1/sessions/{id}/messages
    → Pylon handler → creates Turn → sends to NousActor inbox
```

### The pipeline

Once a message reaches a NousActor, it flows through sequential pipeline stages:

1. **Context assembly**: loads bootstrap files (SOUL.md, IDENTITY.md, GOALS.md) from the agent's workspace, assembles the system prompt within token budget
2. **Recall**: embeds the user message via candle, searches the knowledge store for relevant facts, injects them into context
3. **Execute**: calls the LLM (Anthropic API). If the model returns tool calls, executes them via the tool registry and feeds results back. Loops until the model stops requesting tools or hits the iteration limit.
4. **Finalize**: persists messages to SQLite, records token usage, extracts knowledge facts for future recall

The response flows back through the channel (Signal reply or HTTP response).

---

## The crate map

Crates are organized in layers. Lower layers know nothing about higher layers.

### Leaf (no workspace dependencies)

- **koina**: shared foundation: error types (snafu), tracing setup, filesystem utilities. Every other crate depends on this.

### Low (depends on koina)

- **taxis**: configuration and paths. Loads the YAML config cascade (figment), resolves the oikos instance directory structure.
- **hermeneus**: LLM provider abstraction. The Anthropic streaming client lives here. Handles retries, cost tracking, model routing.
- **symbolon**: authentication: JWT tokens, bearer validation, RBAC. Depends on koina; uses its own SQLite for token storage.
- **mneme**: memory engine. SQLite session store, embedded Datalog engine for knowledge graphs and vector search, candle for local embeddings, LLM-driven fact extraction.

### Mid (depends on lower layers)

- **melete**: distillation. Compresses conversation history when context windows fill up, flushes extracted knowledge to memory. Depends on koina + hermeneus.
- **organon**: tool system. The `ToolRegistry` and `ToolExecutor` trait, plus built-in tools (write, edit, exec, web_search). Depends on koina + hermeneus.
- **agora**: channel system. The `ChannelProvider` trait, Signal client (semeion), message routing via bindings. Depends on koina + taxis.
- **oikonomos** (daemon): background task runner. Cron scheduling, prosoche attention checks, trace rotation, drift detection, DB monitoring. Depends on koina.
- **dianoia**: planning orchestrator. Multi-phase project state machine, workspace persistence. Depends on koina.
- **thesauros**: domain pack loader. Reads `pack.toml` manifests, registers pack tools and context overlays. Depends on koina + organon.

### High (depends on multiple mid+low layers)

- **nous**: the agent pipeline. `NousManager` owns all agents. `NousActor` is a Tokio actor (Alice Ryhl pattern) that runs the pipeline stages. Bootstrap file loading, recall integration, tool execution loop, finalization.
- **pylon**: HTTP gateway. Axum router with versioned API (`/api/v1/`), SSE streaming, OpenAPI spec, Prometheus metrics, security middleware (CORS, CSRF, TLS, auth).

### Top

- **aletheia**: the binary. Wires everything together, CLI parsing (clap), graceful shutdown.
- **tui**: terminal dashboard. Separate workspace member at `tui/`.

---

## The oikos

The instance directory is the boundary between platform code (git-tracked) and deployment state (gitignored).

```
instance/
├── config/         Configuration (aletheia.toml, credentials, TLS certs)
├── data/           SQLite databases, backups
├── logs/           Trace files
├── nous/           Agent workspaces (SOUL.md, MEMORY.md per agent)
├── shared/         Shared tools, coordination, hooks (agent-only)
├── theke/          Human + agent collaborative space (projects, research)
├── signal/         signal-cli data
```

Three-tier cascade (most specific wins):
1. `nous/{id}/`: per-agent overrides
2. `shared/`: shared across all agents
3. `theke/`: human + agent collaborative space

---

## Where to start

| I want to... | Look at... |
|--------------|------------|
| Add a new tool | `crates/organon/src/builtins/` |
| Change config options | `crates/taxis/src/config.rs` |
| Modify the HTTP API | `crates/pylon/src/router.rs`, `crates/pylon/src/handlers/` |
| Change agent pipeline behavior | `crates/nous/src/pipeline/` |
| Add a new LLM provider | `crates/hermeneus/src/provider.rs` (implement `LlmProvider` trait) |
| Add a new messaging channel | `crates/agora/src/` (implement `ChannelProvider` trait) |
| Change how memory works | `crates/mneme/src/` |
| Add a background task | `crates/daemon/src/` (oikonomos) |
| Understand error handling | `crates/koina/src/error.rs` |
