# Architecture walkthrough

For new contributors. For the reference module map and dependency graph, see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## What aletheia is

Aletheia runs multiple AI agents as persistent actors. Each agent has character, memory, and domain expertise. They communicate via Signal, HTTP API, or TUI, and persist understanding across sessions.

It is not a chatbot framework. It is a distributed cognition system: each agent has persistent identity (SOUL.md) and memory (MEMORY.md), and coordinates with other agents through shared infrastructure.

The entire system compiles to a single Rust binary. No Node.js, no Python sidecar, no external databases required.

---

## The binary

When you run `aletheia`, the `main.rs` entrypoint performs a sequential initialization:

1. **Oikos discovery**: finds the instance directory (`./instance`, `ALETHEIA_ROOT`, or `--instance-root`)
2. **Config cascade**: loads compiled defaults, then TOML file, then environment variables (taxis crate)
3. **Domain packs**: loads external knowledge packs declared in config (thesauros crate)
4. **Session store**: opens fjall LSM-tree at `instance/data/sessions.db` (mneme crate; path name is historical). See [DATA.md](DATA.md) for storage details.
5. **JWT/Auth facade**: initializes authentication, RBAC, admin-token verification, and revocation (symbolon crate)
6. **Provider registry**: registers LLM providers and fallback chains (hermeneus crate)
7. **Tool registry**: registers built-in tools, tags, tool groups, and receipt verification services (organon crate)
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

1. **Guard**: applies session limits, loop checks, tool-group policy, and per-stage budget bookkeeping.
2. **Context assembly**: loads bootstrap files (SOUL.md, IDENTITY.md, GOALS.md), injects working-memory `<key_info>` when available, and packs the system prompt within token budget.
3. **Recall**: searches the knowledge store for relevant facts, preserving `Visibility` and `MemoryScope` filters, then injects results into context.
4. **Execute**: calls the active LLM provider. If the model returns tool calls, executes them through the tool registry, records HMAC-SHA256 receipts, and checks composite loop guards.
5. **Finalize**: persists messages to fjall, records token usage, extracts knowledge facts for future recall, and emits maintenance/audit signals.

The response flows back through the channel (Signal reply or HTTP response).

---

## The crate map

Crates are organized in layers. Lower layers know nothing about higher layers.

### Leaf (no workspace dependencies)

- **koina**: shared foundation: error types (snafu), tracing setup, filesystem utilities. Every other crate depends on this.

### Low (depends on koina)

- **taxis**: configuration and paths. Loads the TOML config cascade (owned loader: defaults → TOML → env), resolves the oikos instance directory structure.
- **hermeneus**: LLM provider abstraction. Provider registry, fallback chains, redaction, streaming, cost tracking, and loop-guard helpers live here.
- **symbolon**: authentication: JWT tokens, bearer validation, admin auth facade, and RBAC. Depends on koina; uses its own fjall store for token storage.
- **mneme**: memory engine. fjall session store, embedded Datalog engine for knowledge graphs and vector search, candle for local embeddings, LLM-driven fact extraction.

### Mid (depends on lower layers)

- **melete**: distillation. Compresses conversation history when context windows fill up, flushes extracted knowledge to memory. Depends on koina + hermeneus.
- **organon**: tool system. The `ToolRegistry` and `ToolExecutor` trait, typed tags/groups, HMAC receipts, plus built-in tools (write, edit, exec, web_search). Depends on koina + hermeneus.
- **agora**: channel system. The `ChannelProvider` trait, Signal client (semeion), message routing via bindings. Depends on koina + taxis.
- **oikonomos** (daemon): background task runner. Cron scheduling, prosoche attention checks, trace rotation, drift detection, DB monitoring. Depends on koina.
- **dianoia**: planning orchestrator. Multi-phase project state machine, workspace persistence. Depends on koina.
- **thesauros**: domain pack loader. Reads `pack.toml` manifests, registers pack tools and context overlays. Depends on koina + organon.

### High (depends on multiple mid+low layers)

- **nous**: the agent pipeline. `NousManager` owns all agents. `NousActor` is a Tokio actor (Alice Ryhl pattern) that runs the pipeline stages. Bootstrap file loading, working-memory injection, recall integration, tool execution loop, timeouts, and finalization live here.
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
├── data/           fjall stores, backups
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
