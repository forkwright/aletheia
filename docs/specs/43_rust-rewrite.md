# Spec 43: Rust Rewrite

**Status:** Planned
**Supersedes:** N/A
**Related:** #349 (research), #332 (OS integration), #338 (coding tool quality), #339–346 (bugs closed by completion)

---

## Context

Aletheia began as an OpenClaw fork, was rewritten once, and has been built up since. The stack works but carries inherited decisions: Python sidecar, mem0 abstraction, Node.js single-threaded event loop, per-request database connections, a deploy process broken in production. None are catastrophic. All are wrong for what Aletheia is.

This is a clean rewrite with full hindsight. The docs (ARCHITECTURE.md, STANDARDS.md, specs 01–42, semantic invariants) are the specification. The code is a reference — not the target. Every module is reviewed as it is touched and implemented from first principles against the docs, not ported from the existing code.

**All four design pressures apply equally:**
- Single static binary — `scp + systemctl`. No Node, no Python venv, no npm, no bundling.
- Truly parallel nous execution — multiple nous run simultaneously on real Tokio threads, not interleaved on one event loop.
- No inherited debt — every decision deliberate, nothing carried from the OpenClaw lineage.
- Correct primitives — no accidental event loop blocking, no GC pauses, no per-request connections.

The home deployment (Syn, Akron, Syl, craft nous) is the core use case. The always-on ambient model — Signal-native, nous with independent routines, family access, autonomous background cycles — shapes specific design decisions throughout.

---

## Core Technology Decisions

| Layer | Choice | Replaces | Rationale |
|-------|--------|----------|-----------|
| Language | Rust | TypeScript + Python | No GC, no event loop, single binary, true concurrency |
| Async runtime | Tokio | Node.js event loop | Industry standard, Axum is built on it |
| HTTP server | Axum | Hono | Tokio team, SSE built-in, cleaner middleware |
| HTTP client | reqwest | node-fetch | Async, connection pooling, used for Anthropic + channel calls |
| Anthropic API | Own client (~400 LOC) | @anthropic-ai/sdk | Stable API, reqwest + SSE, we own it |
| Vector store | qdrant-client (official) | mem0 → Qdrant | First-party Rust client, full async |
| Graph store | neo4rs (Neo4j Labs) | mem0 → Neo4j | Adequate for Cypher, all we need |
| Embeddings | fastembed-rs | Python fastembed | Same team, ONNX-based, local, HIPAA-safe |
| Memory abstraction | None (mem0 dropped) | mem0 | ~50 lines of LLM loop replaces the entire library |
| Session store | rusqlite + bundled | better-sqlite3 | No native addon pain, WAL mode |
| Config | serde + validator | Zod | serde is the gold standard |
| Errors | thiserror enums | AletheiaError hierarchy | Compile-time exhaustive matching |
| Logging | tracing | tslog | Spans, layers, OpenTelemetry, journald |
| CLI | clap | Commander | Compile-time validation, derive macros |
| JWT | jsonwebtoken crate | jsonwebtoken (npm) | Direct equivalent |
| Event bus | tokio::sync::broadcast | EventEmitter | Typed, backpressure-aware |
| Plugin system | WASM via wasmtime | Dynamic JS import() | Sandboxed, portable, any-language plugins, OSS-ready |
| MCP | rmcp v0.17 (official) | @modelcontextprotocol/sdk | Same org maintains both |
| Signal | signal-cli subprocess (unchanged) | Same | Keep the JVM binary, rewrite the Rust glue |
| Channel layer | ChannelProvider trait | agora TS | Signal + Slack first, trait handles future channels |
| UI | Svelte 5 (unchanged) | — | No reason to change |
| TUI | Rust/ratatui (unchanged) | — | Already there, already right |

---

## Philosophy: Docs Are the Spec

When implementing each module:

1. Read the relevant spec in `docs/specs/`
2. Read ARCHITECTURE.md for boundary rules
3. Read STANDARDS.md for invariants
4. Implement from those documents
5. Consult the TS/Python code only to understand intent, not to copy implementation

Every module is reviewed when touched. Known-wrong patterns (per-request DB connections, execSync, appendFileSync, mem0 monkey-patching) do not carry forward.

---

## Architecture: Single Binary

**Current:** Node.js runtime (port 18789) + Python sidecar (port 8230) + Rust TUI
**Target:** Single `aletheia` binary. Runtime + memory + CLI unified. TUI remains separate (different UX lifecycle) but shares crates.

```
aletheia binary
├── pylon         — HTTP gateway, SSE, static UI serving
├── symbolon      — JWT session management, auth middleware
├── agora         — channel registry + ChannelProvider trait
│   ├── semeion   — Signal (signal-cli subprocess)
│   └── slack     — Slack (raw API + WebSocket)
├── nous          — agent pipeline, bootstrap, recall, finalize
│   └── roles     — tekton, theoros, zetetes, kritikos, ergates
├── hermeneus     — Anthropic client, model routing, credential management
├── organon       — tool registry + built-in tools
├── dianoia       — planning / project orchestration
├── mneme         — merged memory layer
│   ├── vector    — Qdrant (qdrant-client)
│   ├── graph     — Neo4j (neo4rs)
│   ├── embed     — fastembed-rs (local ONNX)
│   └── recall    — retrieval + scoring pipeline
├── daemon        — per-nous background tasks (see below)
├── taxis         — config, path resolution, secret refs, nous scaffold
├── koina         — errors, tracing, safe wrappers, fs utils
├── prostheke     — WASM plugin host (wasmtime)
└── autarkeia     — agent export/import
```

The gnomon naming system carries forward unchanged.

---

## Nous Actor Model

Each nous is a **Tokio actor**: an independently-running set of tasks with owned state, its own message inbox, and its own background cycles. One OS process, multiple actors.

```
NousActor {
    id: NousId,
    config: NousConfig,
    inbox: mpsc::Receiver<NousMessage>,
    state: NousState,
    // Background task handles (restarted on failure)
    cron: JoinHandle,
    prosoche: JoinHandle,
    channel_listener: JoinHandle,
}
```

**Lifecycle states:**
- **Active** — processing a turn, using the Anthropic API, costs tokens
- **Idle** — background tasks running (cron, prosoche, channel listening), no API calls
- **Dormant** — background tasks paused, wakes on incoming message or scheduled event

Token usage is determined by API calls, not process/task model. A dormant nous costs ~KB of memory and zero tokens.

**Per-nous daemon:** Each nous owns its background cycles rather than sharing a global cron:
- Evolution cron (nightly, benchmarks pipeline variants)
- Distillation schedule (turn count + token threshold)
- Graph maintenance (weekly)
- Prosoche collection (per collector interval)
- Morning digest (scheduled, delivered to this nous's Signal group)

---

## Memory Architecture (Merged, No mem0)

The Python sidecar becomes the `mneme` module inside the runtime. Zero IPC — direct function calls.

**What mem0 did (reimplement in ~50 lines):**

```rust
// 1. Extract facts via LLM
async fn extract_facts(text: &str) -> Result<Vec<Fact>>;

// 2. For each fact: search + decide
async fn decide_action(fact: &Fact, similar: &[Memory]) -> MemoryAction;

// 3. Dispatch
enum MemoryAction { Add, Update(MemoryId), Delete(MemoryId), NoOp }
```

**What was already custom (implement from first principles):**
- Temporal logic — bi-temporal facts, invalidation, episodes (Neo4j Cypher)
- Graph extraction pipeline (neo4rs + LLM prompts)
- Entity resolution, deduplication, evolution, decay (Qdrant + Neo4j)
- 6-factor recall scoring: cosine → recency boost → exponential decay → access boost → noise penalty → domain rerank
- Cross-nous scoring (shared Neo4j graph, Qdrant scoped per-agent)

**Embeddings:** fastembed-rs. Same models. Same ONNX engine. Local, HIPAA-safe.

**Memory topology is unchanged:** per-nous at vector level (Qdrant agent_id), shared at graph level (Neo4j, no agent_id).

---

## Anthropic Client

~400–600 lines, owned. The API is stable.

```rust
pub enum MessageEvent {
    TextDelta { text: String },
    ToolUse(ToolUseBlock),
    ThinkingDelta { thinking: String },
    InputJsonDelta { partial_json: String },
    MessageStart { usage: Usage },
    MessageStop { stop_reason: StopReason, usage: Usage },
}

impl AnthropicClient {
    pub async fn stream_message(&self, req: MessageRequest)
        -> impl Stream<Item = Result<MessageEvent>>;
    pub async fn count_tokens(&self, req: &MessageRequest) -> Result<TokenCount>;
}
```

The streaming state machine becomes a Rust async stream consumer with `tokio::sync::mpsc` channels between stream and tool executor. Tool results are injected back into the stream.

Must implement: streaming + tool_use detection + result injection, prompt caching (two-breakpoint strategy), extended thinking, OAuth2 refresh, multi-credential routing, token counting.

---

## Plugin System (WASM via wasmtime)

Plugins are sandboxed WASM modules — no filesystem or network access except through host-provided capabilities. Compilable from any WASM-targeting language.

**WIT interface (shared with plugin authors):**
```wit
interface aletheia-plugin {
  record turn-context {
    nous-id: string,
    response-text: string,
    tool-calls: u32,
    input-tokens: u32,
    output-tokens: u32,
  }

  on-start: func() -> result<_, string>;
  on-turn-complete: func(ctx: turn-context) -> result<_, string>;
  on-shutdown: func() -> result<_, string>;
}
```

**Host-granted capabilities:** tool registration, tracing log, config read, mneme API calls.

Plugin discovery: directory + `manifest.json` + `.wasm` binary. First-party plugins are internal Rust trait implementations — no WASM overhead for the core system.

---

## Channel System (agora)

```rust
#[async_trait]
pub trait ChannelProvider: Send + Sync {
    fn id(&self) -> &str;
    async fn send(&self, msg: OutboundMessage) -> Result<()>;
    fn stream_inbound(&self) -> BoxStream<'_, Result<InboundMessage>>;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}
```

Signal: signal-cli subprocess, `tokio::process`, reqwest JSON-RPC, SSE stream.
Slack: raw Slack API, reqwest + WebSocket Socket mode.
Routing: each nous config declares which channel IDs are its portals; agora registry handles mapping.

---

## Home Deployment Design Implications

- **Signal is the primary UX.** WebUI is secondary. All nous interactions must be fully functional via Signal alone.
- **Nous independence.** Syn runs continuous background cycles. Syl wakes when her Signal group receives a message. Akron and craft nous sleep until needed. The NousActor model handles all three without special-casing.
- **DBus and eBPF** (per #332) are designed-in integration points in the prosoche collection interface, not afterthoughts. Per-nous ambient context: Syn can subscribe to dev DBus events, Akron doesn't.
- **NixOS module** (per #332 phase 2) is the target deployment form for the home server. Single static binary makes it trivial.

---

## What Does Not Change

- **Gnomon naming:** koina, taxis, mneme, hermeneus, organon, nous, semeion, pylon, prostheke, dianoia, daemon, symbolon, agora, autarkeia. Names are the design.
- **Agent workspace files:** SOUL.md, TELOS.md, MNEME.md, IDENTITY.md, AGENTS.md, TOOLS.md, CONTEXT.md, PROSOCHE.md, EVAL_FEEDBACK.md, STRATEGY.md — Rust runtime reads the same files from the same structure.
- **aletheia.json schema:** Same fields, serde instead of Zod. Existing configs remain valid.
- **HTTP/SSE API surface:** Same endpoints, same event format. Svelte UI and TUI work without modification.
- **Qdrant + Neo4j:** Correct databases. Kept. Connected correctly (once, not per-request).
- **signal-cli binary:** JVM process unchanged. Rust rewrites the glue.
- **6-cycle self-improvement loop:** Evolution cron, competence model, skill extraction, EVAL_FEEDBACK, distillation, sleep consolidation — all preserved, now per-nous rather than global.

---

## Migration Path

TS runtime and Rust rewrite run in parallel. Cutover when Rust passes the adapted test suite and handles a full working session.

| Phase | Module(s) | Proves |
|-------|-----------|--------|
| 1 | koina | Error types, tracing, safe wrappers, fs utils |
| 2 | taxis | serde schema, path resolution, secret resolver |
| 3 | hermeneus (partial) | Anthropic streaming client: tool use, thinking, caching |
| 4 | mneme | Merged memory: Qdrant + Neo4j + fastembed-rs + extraction loop |
| 5 | organon | Tool registry + all built-in tools |
| 6 | hermeneus (complete) | Multi-credential routing, OAuth refresh |
| 7 | nous | NousActor model, 6-stage pipeline, bootstrap assembly, streaming state machine |
| 8 | pylon + symbolon | Axum gateway, SSE, JWT auth, delivery queue, UI serving |
| 9 | agora + semeion | ChannelProvider trait, Signal impl, Slack impl |
| 10 | daemon | Per-nous background tasks, cron, evolution, prosoche |
| 11 | prostheke | wasmtime host, WASM loading, lifecycle dispatch |
| 12 | autarkeia | Agent export/import |
| 13 | dianoia | Planning FSM, reviewed from first principles |

**Locations:**
- `infrastructure/runtime/` — TS runtime (production until cutover)
- `infrastructure/runtime-rs/` — Rust rewrite (in development)

**Post-cutover:** remove `infrastructure/runtime/` and `infrastructure/memory/`.

---

## Deployment Target

```bash
scp aletheia server:/usr/local/bin/aletheia
aletheia init   # creates anchor.json, session.key, memory token
systemctl enable --now aletheia
```

~10–15MB static binary. Replaces: 1.2MB bundle + ~50MB Node.js + 254 npm packages + Python venv + uvicorn. Qdrant and Neo4j remain as containers — correct tools, connected correctly.

---

## Issues Closed by Completion

#339, #340, #341, #342, #343, #344, #345, #346

Informed by: #349

---

## Open Question

**Browser tool:** No Rust playwright equivalent. Options: (a) CDP wrapper around spawned Chromium, (b) accept reduced capability, (c) external process via JSON-RPC. Decide at phase 5 (organon).
