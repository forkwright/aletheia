# Aletheia — Project Plan

> The single source of truth for Aletheia's evolution from TypeScript prototype to Rust production system.
> Every spec, issue, idea, and design decision consolidated here.
> Last updated: 2026-03-02 — M0a/M0b/M1 complete, M2 core + M3.1 complete, CozoDB absorption in progress (GSD). 331 tests across 9 crates, ~14K lines Rust.

---

## Vision

Aletheia is a distributed cognition system — a team of AI agents (nous) that operate as cognitive extensions of their human operator. Always-on, Signal-native, self-improving. The current TypeScript + Python implementation works but carries inherited decisions from its OpenClaw fork lineage. This project is a clean rewrite in Rust with full hindsight, driven by four pressures:

1. **Single static binary** — `scp + systemctl`. No Node, no Python venv, no npm, no bundling.
2. **Portable by default** — Runs on any Linux (Ubuntu, Fedora, NixOS, Alpine) and macOS. No OS-specific dependencies in the core.
3. **True parallelism** — Multiple nous run simultaneously on Tokio threads, not interleaved on one event loop.
4. **No inherited debt** — Every decision deliberate, nothing carried forward unexamined.
5. **Correct primitives** — No event loop blocking, no GC pauses, no per-request DB connections.

The home deployment (Syn, Akron, Syl, Demiurge) is the core use case. The always-on ambient model — Signal-native, independent routines, family access, autonomous background cycles — shapes every design decision.

---

## Architecture

### The Binary

```
aletheia
├── koina         — errors, tracing, safe wrappers, fs utils
├── taxis         — config, path resolution, oikos hierarchy, secret refs
├── mneme         — unified memory (CozoDB embedded + fastembed-rs + extraction)
│   ├── store     — CozoDB: vectors, graph, relations, bi-temporal facts — single embedded DB
│   ├── embed     — EmbeddingProvider trait: fastembed-rs (local default) | HTTP API (Voyage, optional)
│   ├── extract   — LLM-driven fact extraction, entity resolution, contradiction detection
│   └── recall    — hybrid retrieval (vector + graph + BM25), MMR diversity, recollection-as-memory
├── hermeneus     — Anthropic client, model routing, credentials, provider trait
├── organon       — tool registry + built-in tools
├── nous          — agent pipeline, bootstrap, recall, finalize, actor model
│   └── roles     — tekton, theoros, zetetes, kritikos, ergates
├── dianoia       — planning / project orchestration
├── pylon         — Axum HTTP gateway, SSE, static UI serving
├── symbolon      — JWT auth, sessions, RBAC
├── agora         — channel registry + ChannelProvider trait
│   ├── semeion   — Signal (signal-cli subprocess)
│   └── slack     — Slack (raw API + WebSocket)
├── daemon        — per-nous background tasks, cron, evolution, prosoche
├── prostheke     — WASM plugin host (wasmtime)
├── melete        — distillation, reflection, memory flush, consolidation
└── autarkeia     — agent export/import
```

### The Oikos (Instance Structure)

Platform (tracked) vs. instance (gitignored). One directory, one boundary.

```
aletheia/                          # git root — the platform
├── crates/                        # Rust workspace
├── ui/                            # Svelte 5 frontend (unchanged)
├── docs/                          # platform docs, specs, gnomon
│
├── instance/                      # ← GITIGNORED — all instance state
│   ├── theke/                     # Tier 0: human + nous collaborative space
│   │   ├── USER.md               #   Canonical user profile (one copy)
│   │   ├── AGENTS.md             #   Team topology
│   │   ├── tools/                #   Tools for human + all nous
│   │   ├── research/             #   Shared research
│   │   ├── deliberations/        #   Multi-agent deliberations
│   │   └── projects/             #   Active work products
│   │
│   ├── shared/                   # Tier 1: nous-only shared
│   │   ├── tools/                #   Nous-only tools
│   │   ├── skills/               #   Extracted skill patterns
│   │   ├── hooks/                #   Global event hooks
│   │   ├── templates/            #   System prompt templates
│   │   └── coordination/         #   Blackboard, task state
│   │
│   ├── nous/                     # Tier 2: individual nous workspaces
│   │   ├── syn/
│   │   │   ├── SOUL.md           #   Identity (operator-owned)
│   │   │   ├── TELOS.md          #   Goals (operator-owned)
│   │   │   ├── MNEME.md          #   Memory (agent-writable)
│   │   │   ├── tools/            #   Nous-specific tools
│   │   │   ├── hooks/            #   Nous-specific hooks
│   │   │   └── memory/           #   Daily memory files
│   │   ├── demiurge/
│   │   ├── syl/
│   │   └── akron/
│   │
│   ├── config/                   # Deployment config
│   │   ├── aletheia.yaml
│   │   ├── credentials/
│   │   └── bindings.yaml
│   │
│   ├── data/                     # Runtime stores
│   │   ├── sessions.db
│   │   └── cozo/                 #   CozoDB persistent storage (embedded)
│   │
│   └── signal/                   # signal-cli data
│
└── instance.example/              # ← TRACKED — scaffold template
```

Three-tier cascading resolution: nous/{id} → shared → theke. Most specific wins. Presence is declaration — drop a file in the right directory, it's discovered.

### Technology Decisions

| Layer | Choice | Replaces | Rationale |
|-------|--------|----------|-----------|
| Language | Rust | TypeScript + Python | No GC, single binary, true concurrency |
| Async | Tokio | Node.js event loop | Real threads, Axum built on it |
| HTTP | Axum | Hono | SSE built-in, cleaner middleware |
| HTTP client | reqwest | node-fetch | Async, connection pooling, Anthropic + channel calls |
| Anthropic API | Own client (~600 LOC) | @anthropic-ai/sdk | Stable API, reqwest + SSE, adaptive thinking, Tool Search Tool |
| Unified store | cozo (CozoDB) | Qdrant + Neo4j | Rust-native embedded, Datalog, HNSW vectors + graph + relations in one DB. Zero external services. `StorageProvider` trait boundary for risk mitigation. |
| Embeddings | fastembed-rs + `EmbeddingProvider` trait | Python fastembed | Default: local ONNX (nomic-embed-text-v1.5). Optional: Voyage-4-large via HTTP API. Per-instance config. |
| Memory | Direct (no abstraction) | mem0 | ~50 LOC replaces the library |
| Sessions | rusqlite + bundled | better-sqlite3 | WAL mode, no native addon |
| Encryption | XChaCha20Poly1305 | None (plaintext) | Per-message encryption at rest, ~700ns overhead, zero plaintext on disk |
| Config | figment + serde + validator | Zod | figment handles oikos cascade natively (YAML + env + CLI, hierarchical merge). By Rocket author. |
| IDs | ulid + uuid | uuid | ulid for time-sorted data (sessions, messages, memories) — lexicographic sort = natural CozoDB ordering. uuid v4 for non-temporal. |
| Errors | snafu + anyhow + miette | AletheiaError hierarchy | snafu for library/mid-level enums (context wrapping, Location-based virtual stack traces, multiple variants from same source type — GreptimeDB pattern). anyhow for application entry. miette for diagnostics. |
| Logging | tracing + Langfuse | tslog | Spans, layers, OpenTelemetry. Langfuse for LLM-specific traces. |
| CLI | clap | Commander | Compile-time validation |
| JSON | sonic-rs | serde_json | SIMD-accelerated, 2-3x faster parse/serialize |
| Hashing | blake3 + foldhash | crypto.createHash / ahash | blake3 for content hashing (dedup, loop detection). foldhash for HashMap keys. |
| Secrets | secrecy | None | `SecretString` / `SecretVec` — zeroizes on drop, redacts in Debug |
| Hot reload | arc-swap + notify | SIGUSR1 | arc-swap for zero-downtime config swap. notify for file watching. |
| Strings | compact_str | String | 24-byte inline for short strings (agent names, tool names, domain tags) |
| Enums | strum | None | Derive Display, EnumString, EnumIter for all typed enums |
| Git | gix | simple-git (npm) | Rust-native git for workspace auto-commit. No subprocess. |
| Password | argon2 | bcrypt | Password hashing for symbolon. Memory-hard. |
| Cron | cron + jiff | cron (npm) + luxon | cron for schedule parsing. jiff for time/date (by BurntSushi). |
| Concurrent maps | papaya | DashMap | Lock-free, better scaling under contention |
| Seccomp | extrasafe | None | Declarative syscall filtering for sandboxed tools |
| HTML | dom_smoothie | scraper | Readability extraction + HTML→Markdown, single pass |
| Browser | chromiumoxide | None | CDP wrapper for headless Chromium, indexed DOM |
| Event bus | tokio::sync::broadcast | EventEmitter | Typed, backpressure-aware. `watch` for latest-value status. |
| Plugins | WASM (wasmtime) | Dynamic JS import() | Sandboxed, portable, any-language |
| MCP | rmcp (pin version) | @modelcontextprotocol/sdk | Pre-1.0 — pin exact, wrap in trait |
| Testing | bolero + proptest + cargo-llvm-cov | vitest | Unified fuzz/property/coverage |
| UI | Svelte 5 (unchanged) | — | No reason to change |

### Dependency Policy

**~55 direct crates** across the workspace. Each crate uses 5–15. Lean for the scope.

**Pinning rules:**
- **Unstable crates** (pre-1.0, aggressive releases): pin exact version. Wrap in trait.
  - `wasmtime` — monthly major versions. Pin exact.
  - `rmcp` — 5 minor releases in 6 weeks. Pin exact. `McpProvider` trait.
  - `cozo` — pre-1.0, single maintainer. Pin exact. `StorageProvider` trait boundary.
  - `fastembed` — active development. Pin minor.
  - `chromiumoxide` — niche. Pin exact.
- **Stable crates** (1.0+): pin minor (`"1.49"` not `"=1.49.0"`).
- **Never vendor** unless forced by platform issues. Cargo.lock suffices.

**Corrections from audit:**
- `serde_yaml` is deprecated — use `serde_yml` (maintained fork)
- `async-trait` crate is unnecessary — use native `async fn in trait` (Rust 1.75+)
- `thiserror` replaced by `snafu` for library crates (GreptimeDB pattern)

**Cross-compilation notes:**
- `fastembed` (ONNX): may need special builds for aarch64. Feature-gate behind `embed-local`.
- `sonic-rs` (SIMD): aarch64 NEON supported but verify. Fallback to `serde_json` via feature flag.
- `chromiumoxide`: requires Chromium on host. Feature-gate behind `browser`.
- `extrasafe` (seccomp): Linux-only. Feature-gate behind `sandbox-seccomp`.

### Crate-to-Module Mapping

| Crate | Key Dependencies |
|-------|-----------------|
| **koina** | snafu, tracing, tracing-subscriber, miette |
| **taxis** | koina, figment, serde, serde_yml, validator, secrecy, dirs |
| **mneme** | koina, taxis, cozo, fastembed, reqwest (HTTP embedding), ulid, blake3 |
| **hermeneus** | koina, taxis, reqwest, reqwest-eventsource, sonic-rs, tokio, secrecy |
| **organon** | koina, taxis, hermeneus, tokio, gix, extrasafe, chromiumoxide |
| **nous** | koina, taxis, mneme, hermeneus, organon, melete, tokio, ulid, compact_str |
| **dianoia** | koina, taxis, mneme, hermeneus, nous, rusqlite |
| **pylon** | koina, taxis, nous, axum, tower, tower-http, symbolon, sonic-rs, chacha20poly1305 |
| **symbolon** | koina, taxis, rusqlite, jsonwebtoken, argon2 |
| **agora** | koina, taxis, nous, tokio (semeion: tokio::process, slack: tokio-tungstenite) |
| **daemon** | koina, taxis, nous, mneme, cron, notify, arc-swap |
| **melete** | koina, taxis, mneme, hermeneus, nous |
| **prostheke** | koina, taxis, wasmtime |
| **autarkeia** | koina, taxis, mneme, nous, flate2 |

### Release Profile

```toml
[profile.release]
strip = true
lto = "thin"
opt-level = "z"    # optimize for size — single static binary
codegen-units = 1

[profile.dev.package."*"]
opt-level = 2      # optimize deps even in dev — faster iteration
```

---

## Module Design Notes

Implementation details for key modules. Design decisions, not specs — they'll evolve during implementation.

### NousActor (nous)

Each nous is a Tokio actor: independently-running task with owned state, own inbox, own background cycles.

```rust
struct NousActor {
    id: NousId,
    config: NousConfig,
    inbox: mpsc::Receiver<NousMessage>,
    state: NousState,
    cron: JoinHandle<()>,
    prosoche: JoinHandle<()>,
    channel_listener: JoinHandle<()>,
}
```

Lifecycle states: **Active** (processing turn, API calls, costs tokens) → **Idle** (background tasks, no API) → **Dormant** (paused, wakes on message/schedule). A dormant nous costs ~KB memory, zero tokens.

Per-nous daemon (not global): evolution cron, distillation schedule, graph maintenance, prosoche collection, morning digest.

### Memory (mneme)

mem0 replacement in ~50 lines:

```rust
async fn extract_facts(text: &str) -> Result<Vec<Fact>>;
async fn decide_action(fact: &Fact, similar: &[Memory]) -> MemoryAction;
enum MemoryAction { Add, Update(MemoryId), Delete(MemoryId), NoOp }
```

CozoDB provides all three storage layers:
- **Vectors:** HNSW with cosine/L2/IP. Per-nous scoping via `nous_id`.
- **Graph:** Datalog with stratified negation. PageRank, community detection.
- **Relations:** Stored relations for entity metadata, bi-temporal facts.

Custom (first principles): bi-temporal knowledge graph, graph extraction, entity resolution, 6-factor recall scoring, cross-nous scoring, recollection-as-memory.

### Anthropic Client (hermeneus)

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

Streaming state machine as async stream consumer. `tokio::sync::mpsc` between stream and tool executor. Tool results injected back.

### Plugins (prostheke)

```wit
interface aletheia-plugin {
  record turn-context {
    nous-id: string, response-text: string,
    tool-calls: u32, input-tokens: u32, output-tokens: u32,
  }
  on-start: func() -> result<_, string>;
  on-turn-complete: func(ctx: turn-context) -> result<_, string>;
  on-shutdown: func() -> result<_, string>;
}
```

Host-granted capabilities: tool registration, tracing, config read, mneme API. First-party plugins are internal Rust traits — no WASM overhead for core system.

### Channels (agora)

```rust
pub trait ChannelProvider: Send + Sync {
    fn id(&self) -> &str;
    async fn send(&self, msg: OutboundMessage) -> Result<()>;
    fn stream_inbound(&self) -> BoxStream<'_, Result<InboundMessage>>;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}
```

Signal: signal-cli subprocess, tokio::process, JSON-RPC, SSE stream.
Slack: raw API, reqwest + WebSocket Socket mode.

### Home Deployment Constraints

- Signal is the primary UX. WebUI secondary. All functionality must work via Signal alone.
- Nous independence: Syn runs continuous background. Syl wakes on Signal message. Akron sleeps until needed. NousActor handles all three.
- DBus/eBPF are designed-in prosoche collection points, not afterthoughts.
- NixOS module is the target deployment form for home server.

---

## Milestones

### M0a: Oikos Migration (TypeScript)

**Goal:** Migrate current deployment to oikos instance structure. Validates the design before Rust implementation.

| Phase | Work | What It Proves |
|-------|------|----------------|
| 0a.1 | Instance structure | Create `instance/` layout, migrate current deployment |
| 0a.2 | Tool resolution | Tools discovered by cascade — drop YAML, it's a tool |
| 0a.3 | Context assembly | Bootstrap reads from cascade, no hardcoded file lists |
| 0a.4 | Config cascade | `defaults.yaml` + per-nous `overrides.yaml`, deep merge |

**Success criteria:** Live deployment uses `instance/` layout. All existing functionality preserved.

**Exit gate:** All current agent configs, tools, hooks, and context files live in the oikos hierarchy. Zero hardcoded paths.

---

### M0b: Foundation Crates (Rust)

**Goal:** First Rust code. Error types, config resolution, path handling.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 0b.1 | `koina` | Error types (snafu), tracing setup, safe wrappers compile and test |
| 0b.2 | `taxis` | figment-based config cascade (nous → shared → theke), oikos 3-tier path resolution, SecretRef resolver |

**Success criteria:** `koina` and `taxis` compile, test, and correctly resolve paths through the oikos hierarchy.

**Exit gate:** `taxis` Rust path resolution produces identical results to TS implementation for all current paths.

---

### M1: Memory + LLM Client

**Goal:** The two hardest integrations — prove Anthropic streaming and merged memory work in Rust before committing to the full rewrite.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 1.1 | `hermeneus` (partial) | Anthropic streaming: tool use, adaptive thinking (effort param), prompt caching (two-breakpoint), Tool Search Tool |
| 1.2 | `mneme` (CozoDB) | Unified embedded store: HNSW vectors + graph + relations + bi-temporal facts — single DB, zero external services |
| 1.2a | `mneme` (CozoDB validation) | **Gate:** HNSW recall quality matches Qdrant baseline, Datalog query perf acceptable for graph traversal, concurrent read/write under realistic load. If fails → `StorageProvider` trait lets us pivot to rusqlite + custom graph. |
| 1.3 | `mneme` (embedding) | `EmbeddingProvider` trait: fastembed-rs local default, optional Voyage-4-large. JEPA-informed: embed once, reuse for shift detection, recall, classification |
| 1.4 | `mneme` (recall) | Hybrid retrieval (vector + graph + BM25), MMR diversity, temporal decay, recollection-as-memory |
| 1.5 | `hermeneus` (complete) | Multi-credential routing, OAuth auto-refresh, `trait LlmProvider`, token counting (`/v1/messages/count_tokens`), batch API integration (50% discount for async ops: distillation, extraction, cron), server-side compaction as emergency fallback, citations for memory-grounded responses |

**Absorbed ideas:**
- **Spec 27 (Embedding Space Intelligence):** Semantic shift detection, embedding-space similarity (replacing Jaccard), predictive context assembly. The mneme crate implements these natively rather than bolting them onto text heuristics.
- **Spec 38 (Provider Adapters):** `trait LlmProvider` with Anthropic first, extensible to OpenAI/Ollama. Per-agent model config via oikos cascade.
- **Issue #327 (OAuth auto-refresh):** Built into hermeneus from day one — no manual credential rotation.

**Success criteria:** Can stream a full multi-tool conversation through hermeneus. Memory recall quality measurably improves over current implementation (MMR diversity, temporal decay, hybrid BM25+vector).

**Exit gate:** hermeneus + mneme integration test passes a full turn cycle: embed message → recall memories → stream to Anthropic → extract facts → store.

---

### M2: Agent Core

**Goal:** A single nous can run a conversation end-to-end through the Rust pipeline.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 2.1 | `organon` | Tool registry + all built-in tools ported |
| 2.2 | `nous` | NousActor model, 6-stage pipeline, bootstrap assembly, streaming state machine |
| 2.3 | `melete` | Distillation, reflection, memory flush, pressure-triggered consolidation |
| 2.4 | Agent-writable workspace | MNEME.md append, CONTEXT.md update, protected SOUL.md/TELOS.md |

**Absorbed ideas:**
- **Spec 35 (Context Engineering):** Cache-group bootstrap with stable prefix, skill relevance filtering, turn bypass classifier. Built into nous::bootstrap. Token budget: current TS system uses ~31,500–46,500 tokens/turn for system prompt. Target savings: ~7,700 tokens/turn via two-tier tool descriptions (proc macro short/extended, -4K), semantic skill retrieval (top-5 not all 130, -2K), tool consolidation (sessions_spawn+dispatch→1, sessions_send+ask→1, 5 memory mutation→2, -800), auto-activate tools by domain (-900).
- **Spec 42, Gap 5 (Pressure-Triggered Consolidation):** Sleep agent spawned on three triggers: turn count (20), session idle (2hr), token pressure (75%). Token pressure fires *before* distillation — consolidation promotes knowledge to long-term storage while distillation compresses conversation. Complementary, not competing (G-08). Haiku-tier, async, outputs MNEME updates.
- **Spec 42, Gap 6 (Workspace Hygiene):** Session-start check for TELOS staleness, MNEME bloat, orphaned files. Local reads only, zero LLM cost.
- **Spec 42, Gap 7 (Agent-Writable Files):** Binary model — file is writable or not (G-09). SOUL.md, TELOS.md, USER.md, IDENTITY.md = operator-owned. MNEME.md, CONTEXT.md = agent-writable. `workspace_note` tool with size limits and audit trail. Agent self-knowledge goes in MNEME, not IDENTITY.
- **Spec 42, Gap 4 (Epistemic Confidence):** Behavioral norm in AGENTS.md template — `[verified]`, `[inferred]`, `[assumed]` markers. No code dependency.
- **Issue #338 (Coding tool quality):** Per-call `cwd` parameter, per-nous `workingDir` config via oikos cascade, 120s default timeout, glob tool.

**Actor model critical pitfalls (must address in M2.2):**
1. **Channel sizing** — bounded channels with backpressure. Unbounded = memory leak. Default: 32, tune empirically.
2. **Shutdown signaling** — when all `NousHandle` clones drop, mpsc closes and actor exits. No separate shutdown unless cleanup needed.
3. **Backpressure** — actor can't keep up → `send()` blocks (bounded) or caller handles `TrySendError::Full`. Never silently drop messages.
4. **Task spawning** — spawned tasks outlive the actor. Use `JoinHandle` tracking or `CancellationToken`.
5. **State ownership** — actor owns ALL mutable state. Handle is a thin `mpsc::Sender` wrapper. No `Arc<Mutex<_>>` between them.

**Cancellation safety constraints:**
- Cancel-SAFE: `sleep()`, `Receiver::recv()`, `Sender::reserve()`, reads into owned buffers
- Cancel-UNSAFE: `Sender::send(msg)` (message lost), `write_all()` (partial write), mutex guard across `.await`
- All `select!` branches must be cancel-safe or use reserve-then-send pattern
- Spawn request handlers as tasks, not inline futures, so disconnection doesn't cancel processing

**Success criteria:** One nous (Syn) can handle a full conversation: receive message → assemble context → recall memories → stream response → extract facts → distill when needed. All quality features (circuit breakers, loop detection, narration filtering) operational.

**Exit gate:** Rust pipeline produces functionally equivalent responses to TS pipeline for a set of 20 reference conversations. Token usage per turn measurably lower than TS baseline.

---

### M3: Gateway + Auth + Channels

**Goal:** The system is externally reachable — web UI, Signal, Slack all work.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 3.1 | `pylon` + `symbolon` | Axum gateway, SSE streaming, JWT auth (15min access + 7day refresh, G-11), delivery queue, static/dev UI serving (G-10) |
| 3.2 | `agora` + `semeion` | ChannelProvider trait, Signal impl (signal-cli subprocess glue) |
| 3.3 | `agora` + slack | Slack Socket Mode, streaming, reactions, DM pairing, access control |
| 3.4 | Delivery reliability | Outbound retry queue with exponential backoff for failed sends |

**Absorbed ideas:**
- **Issue #256 (Delivery retry):** Built into pylon/agora from day one.
- **Spec 34 (Agora) decisions:** ChannelProvider trait, per-channel config via oikos cascade, message normalization.

**Success criteria:** Svelte UI connects via SSE, Signal messages route correctly, Slack integration works. All channels use the same ChannelProvider trait.

**Exit gate:** Full session through web UI + Signal simultaneously with no message loss.

---

### M4: Multi-Nous + Background

**Goal:** All four nous running concurrently with independent background cycles.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 4.1 | `nous` (multi-actor) | Multiple NousActors on real Tokio threads, independent inboxes |
| 4.2 | `daemon` | Per-nous cron, evolution, prosoche, graph maintenance, morning digest |
| 4.3 | `dianoia` | Planning FSM from first principles. **Core redesign:** workspace model (not pipeline). 3 operating modes: full project (research→execute→verify), quick task (appetite-based, time-boxed), and autonomous background. Skip any phase that adds no value. State machine with exhaustive `match` on typed enums — every transition explicit. |
| 4.4 | Cross-nous coordination | Competence-aware routing, structured task handoff, priority queue |

**Absorbed ideas:**
- **Spec 27 Phase 4 (Cross-Agent Semantic Routing):** Route messages to the correct nous by comparing message embedding to each agent's memory cluster centroid. Replaces config-label domain matching with embedding-space proximity — foundational to correct multi-nous coordination. (Moved from M6 per G-05: without this, M4 ships with inherited string-matching patterns.)
- **Spec 39 (Autonomy Gradient):** 4-level configurable autonomy (confirm-all → confirm-never) in dianoia. Per-agent default, per-project override (G-12). Auto-advance on read-only phases.
- **Spec 42, Gaps 1-3 (Feedback Loops):** Competence scores influence routing. Kritikos flags feed back to competence model. Prosoche creates draft projects for high-urgency signals (auto-expire 48hr, per G-14).
- **Spec 42, Gap 2 (Task Handoff):** Structured lightweight schema: `{id, from, to, type, context, status, created, updated}`. State machine: created → assigned → in-progress → review → done. Context travels with handoff. Informal `sessions_send` remains for quick coordination (G-13).
- **Issue #313 (Prosoche signals):** Activity tracking, HEARTBEAT_OK dedup, work signals. Built into daemon.
- **Issue #239 (Graph maintenance):** Automated Neo4j QA, Qdrant dedup, orphan purge. Per-nous cron schedule.

**Success criteria:** Syn, Akron, Syl, Demiurge all running simultaneously. Background tasks execute independently. Cross-nous task handoff works without operator intervention. Semantic routing correctly directs messages to domain-appropriate nous without config labels.

**Exit gate:** 24-hour soak test with all four nous — zero crashes, zero message loss, background cycles complete on schedule. Semantic routing accuracy ≥90% against labeled test set.

---

### M5: Plugins + Portability + Cutover

**Goal:** Feature parity with TS runtime. Production cutover.

| Phase | Crate | What It Proves |
|-------|-------|----------------|
| 5.1 | `prostheke` | wasmtime host, WASM plugin loading, lifecycle dispatch |
| 5.2 | `autarkeia` | Agent export/import (AgentFile format) |
| 5.3 | Integration testing | Full test suite adapted from TS, coverage targets met |
| 5.4 | Cutover | TS runtime retired, Rust binary takes production |

**Absorbed ideas:**
- **Spec 40 (Testing Strategy):** Coverage targets, integration patterns, contract tests adapted for `cargo test`. CI enforcement.
- **Spec 41 (Observability):** tracing crate with spans, layers, journald integration. Prometheus/OpenTelemetry metrics. Structured query: "why was that turn slow?"

**Success criteria:** All existing functionality works. Test suite passes. Binary deploys via `scp + systemctl`.

**Exit gate:** One full week of production operation with zero TS runtime fallback.

**Post-cutover:** Remove `infrastructure/runtime/` and `infrastructure/memory/`. All state under `instance/`.

---

### M6: Platform Extensions (Post-Rewrite)

**Goal:** Capabilities that require a stable Rust platform to build on.

| Phase | Work | Origin |
|-------|------|--------|
| 6.1 | A2UI Live Canvas — agent-writable dynamic UI surface | Spec 43b, Issue #319 |
| 6.2 | Interop — A2A protocol, workflow engine, IDE integration | Spec 22 |
| 6.3 | Aletheia Linux — eBPF/DBus sensing, NixOS module | Spec 24, Issue #332 |
| 6.4 | Embedding space intelligence — JEPA goal vectors, collapse prevention, embedding health monitoring | Spec 27 (phases 5-6) |
| 6.5 | UI layout overhaul + homepage dashboard | Specs 29, 30, Issue #328 |
| 6.6 | TUI enhancements — fuzzy filter, F2 overlay, OSC 8, plan widget | Issue #326 |

These are not sequenced — they can be worked in parallel once the platform is stable. Each is a self-contained project.

---

## Implementation Standards

### Philosophy: Docs Are the Spec

When implementing each crate:

1. Read the relevant section of this document
2. Read `docs/ARCHITECTURE.md` for boundary rules
3. Read `docs/STANDARDS.md` for invariants
4. Read `docs/gnomon.md` for naming
5. Implement from those documents
6. Consult TS/Python code only to understand *intent*, not to copy implementation

Known-wrong patterns do not carry forward: per-request DB connections, `execSync`, `appendFileSync`, mem0 monkey-patching, silent catches, bare `throw new Error`.

### Rust Standards

| Rule | Detail |
|------|--------|
| **Error handling** | `snafu` enums per crate with `.context()` propagation and `Location` tracking (virtual stack traces). No `unwrap()` in library code. `anyhow` only in CLI entry points. Convention: `source` field = internal error (walk chain), `error` field = external (stop walking). Log where HANDLED, not where they occur. |
| **Async** | All I/O is async (Tokio). No `block_on` inside async context. Document cancellation safety for every public async method. In `select!`: reserve-then-send, cursor-tracked writes, never hold mutex guards across `.await`. |
| **Logging** | `tracing` with structured spans. `#[instrument]` on public functions. Spawned tasks MUST propagate spans via `.instrument()` or `.in_current_span()`. Never hold `span.enter()` across `.await`. |
| **Config** | `figment` for hierarchical cascade (YAML + env + CLI) + `validator` for constraints. All config declarative YAML, validated at load. |
| **Testing** | Unit tests in same file (`#[cfg(test)]`). Integration tests in `tests/`. Property tests for serialization roundtrips. |
| **Dependencies** | Minimal. Prefer std when adequate. Each new dependency must justify itself. |
| **Naming** | Gnomon system. Crate names = module names from architecture. |
| **Newtypes** | Domain IDs (`AgentId`, `SessionId`, `NousId`, `TurnId`, `ToolName`) are newtype wrappers, not bare `String`/`u64`. Zero-cost, compile-time safety against parameter swaps. |
| **Enums** | `#[non_exhaustive]` on all public enums that may grow. `#[expect(lint)]` over `#[allow(lint)]` (2024 edition — warns when suppression is no longer needed). `#[diagnostic::on_unimplemented]` on public traits (Tool, ChannelProvider, LlmProvider, StorageProvider) for clear error messages. |
| **Typestate** | Use typestate pattern for multi-step builders and connection lifecycle (e.g. `Connection<Disconnected>` → `Connection<Connected>`). Compile-time state validation over runtime checks. |
| **Unsafe** | Prohibited unless reviewed and documented. Zero unsafe in application code. |
| **Clippy** | `#[deny(clippy::all)]`. No suppression without comment. |

### Commit Standards

```
<type>(<scope>): <imperative description, ≤72 chars>

<what and why, wrapped at 72 chars>
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `security`. Scopes: crate names + `ui`, `tui`, `cli`, `specs`, `ci`. Single author: `forkwright <alice@example.com>`. No `Co-authored-by` lines. No agent attribution.

### Deviation Rules

When implementing, deviations from this plan or existing specs follow escalation levels:

| Level | Scope | Action |
|-------|-------|--------|
| L1 | Bug fix — broken behavior, no design change | Auto-fix, document in commit |
| L2 | Critical addition — missing piece that blocks progress | Auto-add, document rationale in commit body |
| L3 | Blocker resolution — spec conflict or impossible requirement | Auto-fix, flag in next status update |
| L4 | Design change — different approach than what's specified | **STOP AND ASK.** No autonomous design changes. |

### Research Protocol

Claims about external systems, libraries, or protocols require evidence:

| Tier | Source | Example |
|------|--------|---------|
| S1 | Peer-reviewed / official docs | Tokio docs, Rust reference, RFC |
| S2 | Authoritative secondary | crates.io README, well-maintained blog |
| S3 | Community knowledge | GitHub issues, Stack Overflow with verification |
| S4 | Direct testing | "I ran this and observed..." |
| S5 | Our synthesis | Combining sources into a conclusion |

**Rules:** Inline-cite sources. Include counter-evidence when it exists. Never cite what you haven't read. "I don't know" is always acceptable; wrong is not.

### What Carries Forward Unchanged

- **Svelte 5 UI** — no reason to rewrite
- **Qdrant + Neo4j** — correct databases, connected correctly
- **signal-cli** — JVM process unchanged, Rust rewrites the glue
- **Agent workspace files** — SOUL.md, TELOS.md, MNEME.md, etc. Same files, oikos paths
- **HTTP/SSE API surface** — same endpoints, same events. UI works without modification
- **6-cycle self-improvement loop** — evolution, competence, skills, feedback, distillation, consolidation
- **Gnomon naming** — the naming system is the architecture

---

## Lessons Learned

Operational rules derived from 23 days of running the TS system. These are not theoretical — each was earned through failure or near-miss.

### Verification

1. **Check first, answer second.** Every agent hit the pattern of answering before verifying. Assume nothing about system state — read the actual file, query the actual service, run the actual test. (6-pattern audit, 2026-02-13)
2. **Verify output exists before reporting done.** "I wrote the file" means nothing if `ls` doesn't show it. "Tests pass" means nothing without the output. (Pattern #3)
3. **Physical verification > theoretical mapping.** Always check the actual system, not your model of it. Own docs are not evidence — that's circular reasoning.
4. **Never cite what you haven't read.** Applies to specs, docs, API references, and your own memory. If you're not sure, re-read it.

### Building

5. **Overbuild when it adds value.** Conservative scoping was a recurring failure — applying fixes narrowly, waiting for permission to expand scope. Apply broadly. The marginal cost of doing it right is almost always lower than the cost of doing it twice. (Pattern #4)
6. **Zero broken windows.** Pre-existing failures get fixed or deleted, never ignored. Broken infrastructure that stays broken becomes invisible — Letta was down 7 days before anyone noticed. (Pattern #6)
7. **First token is better than 100,000th.** Judgment degrades with context length. Split large work into focused sessions. Don't accumulate 100k+ tokens of mechanical work before the hard decisions.

### Architecture

8. **Co-primary file + DB.** Files and database are co-equal — files survive DB corruption, git-track decisions, enable handoff artifacts. DB provides query and index. Neither is subordinate.
9. **Snap changes > gradual ramps.** When a decision is made, scaffold it that day. Incremental migration plans create transition states that are harder to reason about than the before or after.
10. **Policy before implementation.** Document standards first, then build. Standards written after the code are rationalizations, not constraints.

### Planning

11. **Planning's primary value is stress-testing infrastructure.** The plan itself is secondary to the discovery that happens while planning — gaps in tooling, broken assumptions, missing capabilities.
12. **Success criteria must cover ALL requirements.** Partial criteria produce partial delivery. If a milestone has 6 requirements, the exit gate checks all 6.
13. **Structured decision artifacts over informal agreement.** Locked decisions, deferred ideas, and discretion zones — captured in writing, not conversation memory.

### Process

14. **Don't retry the same thing with minor variations.** If a command fails, understand why before trying again. One attempt, then adapt approach.
15. **Distillation does not write memory files.** The pipeline either doesn't trigger or bypasses the hook. Write to `memory/YYYY-MM-DD.md` manually during sessions — don't rely on automation.
16. **Record state before delivery, not after.** If delivery fails and state wasn't recorded, the system retries infinitely. Always persist state first. (Prosoche dedup fix, 2026-02-19)

---

## Resolved Design Decisions

All 20 grey areas resolved 2026-02-28. Reviewed through four frames: long-term best for Aletheia, alignment with operator philosophy, no corners cut, gnomon naming integrity.

### M0 (Foundation)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-01 | Hooks: supplement or override? | **Supplement with explicit `replaces:` opt-in.** | Default additive — shared `on_session_start` runs, then nous-specific runs too. A nous can declare `replaces: shared/hooks/on_session_start.yaml` to take over. Covers both cases without surprise. Aligns with oikos metaphor: household members can take on shared responsibilities, but must explicitly claim them. |
| G-02 | Config format: YAML or TOML? | **YAML.** | Agent-generated config has multi-line strings (system prompts, tool descriptions, context blocks). TOML's multi-line handling is awkward. serde handles both equally. Existing config is all YAML — zero migration cost. |
| G-03 | Template nous in `instance.example/`? | **Yes — `_template/` directory.** | `aletheia add-nous <name>` copies it. Starter SOUL.md with commented sections, empty TELOS.md, empty MNEME.md, .gitkeep in tools/ and hooks/. Without it, scaffold logic lives in Rust code instead of declarative files — worse. |

### M1 (Memory + LLM)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-04 | Voyage-4-large migration? | **Migrate during M1. Clean break.** | 2400 memories × Voyage-4 pricing ≈ $0.50. MoE architecture and shared embedding space are materially better. Start mneme on the right foundation. |
| G-05 | JEPA split across milestones? | **Phases 1-3 in M1. Phase 4 (cross-agent semantic routing) in M4. Phases 5-6 in M6.** | Phase 4 is foundational to multi-nous routing — comparing message embedding to agent memory cluster centroids replaces config-label domain matching. Without it in M4, we'd build multi-nous coordination on the same inherited string-matching pattern. That violates "no inherited debt." Phases 5-6 (goal vectors, collapse prevention) are optimization on a working system. |
| G-06 | Memory extraction: LLM or rules? | **LLM-based with rule-based pre-filter.** | Current quality issues aren't because LLM extraction is wrong — the prompt lets through noise. Tighter extraction prompt + NOISE_PATTERNS pre-filter (already built in Spec 23) gives best of both. Pure rule-based can't handle "is this fact worth remembering?" |

### M2 (Agent Core)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-07 | Browser tool approach? | **`chromiumoxide` CDP wrapper around spawned Chromium.** | Tokio-native, CDP protocol, actively maintained. We only need rendered page fetching and light interaction, not test automation. ~200 LOC wrapper. Falls back to JSON-RPC to external process if insufficient. |
| G-08 | Consolidation triggers? | **Three triggers: turn count (20) + session idle (2hr) + token pressure (75%).** | Token pressure fires consolidation *before* distillation kicks in. They're complementary, not competing: distillation compresses the *conversation* (context management), consolidation promotes *knowledge* to long-term storage (what the agent learned). Excluding token pressure means knowledge accumulated in long sessions gets compressed into distillation summaries instead of properly extracted. |
| G-09 | Agent-writable workspace guardrails? | **Binary: file is writable or not. IDENTITY.md stays operator-owned.** | SOUL.md = essential nature (operator commitment). TELOS.md = purpose (operator commitment). IDENTITY.md = εἶδος, visible form — stable, how others recognize you. If the agent can drift its own visible form, the operator loses identity assurance. Agent self-knowledge (evolving patterns, growth observations) belongs in MNEME.md — that's *memory*, exactly where learned self-understanding should live. IDENTITY is declaration, not discovery. The binary model isn't a shortcut — it's the correct ontological boundary. |

### M3 (Gateway + Channels)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-10 | Pylon: static files or vite proxy? | **Static in prod, vite proxy in dev. `ui.mode: static \| dev` in config.** | One config flag. Axum has both capabilities built in. Zero complexity. |
| G-11 | JWT model? | **15-minute access + 7-day refresh. Auto-refresh in UI.** | Short access tokens limit exposure. Refresh tokens in httpOnly cookies. UI intercepts 401, refreshes, retries. Standard and proven. Long-lived tokens are a known security gap. |

### M4 (Multi-Nous)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-12 | Autonomy gradient scope? | **Per-agent default with per-project override.** | Agent config in oikos sets baseline. Project creation can override. Two config points, clear precedence. Most work uses agent default. |
| G-13 | Task handoff protocol? | **Structured with lightweight schema.** | `{id, from, to, type, context, status, created, updated}` — 8 fields. State machine: created → assigned → in-progress → review → done. Informal `sessions_send` stays for quick coordination. Structured tasks for work that needs tracking. Don't force everything through the protocol. |
| G-14 | Prosoche auto-project creation? | **Draft project creation (auto-expires 48hr). Proactive suggestion with human gate.** | "Notification only" is too passive — contradicts "proactive, not reactive." But auto-creating from noisy signals creates cleanup work. Middle path: prosoche formulates the project as a draft, operator approves or lets it expire. The system does the work of scoping; the human decides whether to pursue. |

### M5 (Cutover)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-15 | Parallel operation period? | **1 week minimum, 2 weeks ideal. Automated comparison.** | Same input to both runtimes, diff outputs. Comparison framework: response quality, latency, recall accuracy, background task completion. Week 1 finds regressions, week 2 builds confidence. |
| G-16 | Rollback plan? | **`aletheia-ts` systemd service available for 30 days post-cutover.** | `systemctl start aletheia-ts` if critical. Costs nothing but disk space. Remove after 30 days with zero fallbacks. |

### M6 (Extensions)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-17 | A2A protocol? | **Server-side discovery only initially.** | Expose agent cards at `/.well-known/agent.json`. Don't build client-side delegation until protocol hits 1.0 and there's a real system to talk to. Low effort to expose, high risk to depend on. |
| G-18 | eBPF depth? | **Structured feeds from existing tools first.** | journald, ss, /proc, systemd — all accessible without kernel programming. eBPF for network packet inspection in later phase only if structured feeds prove insufficient. Prove value before investing in kernel complexity. |
| G-19 | NixOS packaging? | **Flake with module inside.** | Standard modern Nix pattern. `nix run github:forkwright/aletheia` works. Module provides `services.aletheia = { enable = true; ... }` for NixOS. Both, not either/or. |
| G-20 | A2UI component sandboxing? | **Structured data API for standard types + WASM-sandboxed custom components via prostheke.** | Standard types (table, chart, progress, kanban): agents emit typed data, UI renders with known-safe Svelte components. Novel visualizations: WASM component with defined input/output contract, sandboxed same as plugins. Same security model as prostheke — consistent architecture. No iframes, no arbitrary HTML, no XSS surface. |

### Gnomon Naming Audit

All 17 crate names verified against gnomon layer test (2026-02-28). Each name uncovers essential nature, not function:

| Crate | Greek | Uncovering |
|-------|-------|-----------|
| koina | κοινά — common things | The shared commons all crates draw from |
| taxis | τάξις — arrangement | The ordering principle of the system |
| mneme | μνήμη — memory | Accumulated knowing |
| hermeneus | ἑρμηνεύς — interpreter | Translation between human intent and model response |
| organon | ὄργανον — instrument | Aristotle's instruments of thought |
| nous | νοῦς — mind | Direct apprehension, the agent itself |
| dianoia | διάνοια — discursive reasoning | Thinking-through, step by step |
| pylon | πυλών — gateway | The entrance through which all communication passes |
| symbolon | σύμβολον — identity token | A broken token matched to prove identity — literally auth |
| agora | ἀγορά — gathering place | Where communication happens |
| semeion | σημεῖον — sign, signal | The signal itself |
| daemon | δαίμων — spirit | The ever-present background spirit |
| prostheke | προσθήκη — addition | Extensions added to the whole |
| melete | μελέτη — disciplined practice | Care, attention, the work of integration |
| autarkeia | αὐτάρκεια — self-sufficiency | Making a nous portable and complete |

Sub-agent roles: tekton (τέκτων, builder), theoros (θεωρός, observer), zetetes (ζητητής, seeker), kritikos (κριτικός, judge), ergates (ἐργάτης, worker). Each names a distinct epistemic stance toward work.

---

## Spec Disposition

Every existing spec has been accounted for. This table is the definitive record.

### Absorbed Into This Plan

| Spec | Title | Absorbed Into | Key Ideas Preserved |
|------|-------|--------------|---------------------|
| 27 | Embedding Space Intelligence | M1 (mneme) + M4 (semantic routing) + M6 (goal vectors) | Semantic shift detection, embedding-space ops, cross-agent routing, JEPA principles |
| 33 | Gnomon Alignment | Crate naming + M0 migration | Crate names = gnomon. TELOS/MNEME renames during oikos migration |
| 35 | Context Engineering | M2 (nous) | Cache-group bootstrap, skill relevance, turn bypass classifier |
| 36 | Config Taxis | M0 (Spec 44) | SecretRef retained in taxis. 4-layer → 3-tier oikos |
| 37 | Metadata Architecture | M0 (Spec 44) | Declarative cascade, convention-based discovery |
| 38 | Provider Adapters | M1 (hermeneus) | `trait LlmProvider`, multi-provider support |
| 39 | Autonomy Gradient | M4 (dianoia) | 4-level autonomy, configurable per-agent/project |
| 42 | Nous Team | M2 + M4 | Feedback loops, task handoff, consolidation, hygiene, epistemic tiers |
| 43 | Rust Rewrite | **Absorbed into this plan** | Unique content (NousActor, hermeneus, prostheke, agora, home deployment) merged into Module Design Notes. Spec file deleted. |
| 44 | Oikos | M0 | Instance structure, 3-tier hierarchy, cascading resolution |

### Retained Independently

| Spec | Title | Milestone | Status | Notes |
|------|-------|-----------|--------|-------|
| 22 | Interop & Workflows | M6 | Deferred | A2A, workflow engine, IDE integration — needs stable platform |
| 24 | Aletheia Linux | M6 | Deferred | eBPF/DBus, NixOS module — needs stable binary |
| 29 | UI Layout & Theming | M6 | In Progress | Svelte UI — independent of rewrite |
| 30 | Homepage Dashboard | M6 | Skeleton | Svelte UI — shared task board |
| 40 | Testing Strategy | M5 | Draft | Coverage targets adapted for cargo test |
| 41 | Observability | M5 | Draft | tracing crate, metrics, spans |
| 43b | A2UI Live Canvas | M6 | Draft | Agent-writable UI surface |

### Implemented and Archived

33 specs (01–25, 26, 28, 31, 32, 34) documented in `docs/specs/archive/DECISIONS.md`. Key decisions preserved, code is source of truth.

---

## Issue Disposition

| Issue | Title | Status | Disposition |
|-------|-------|--------|-------------|
| #352 | Rust rewrite tracking | Open | Meta-issue for this plan |
| #338 | Coding tool quality | Open | Absorbed → M2 (organon, per-nous workingDir via oikos) |
| #332 | OS-layer integration | Open | Retained → M6 (Spec 24) |
| #328 | Planning dashboard | Open | Retained → M6 (Spec 29) |
| #326 | TUI deferred items | Open | Retained → M6 |
| #319 | A2UI live canvas | Open | Retained → M6 (Spec 43b) |
| #349 | Evaluate Rust rewrite | Closed | Decision made — this plan |
| #339–346 | Various bugs | Closed | Resolved by rewrite (no Node, no sidecar, no shell scripts) |
| #327 | OAuth auto-refresh | Closed | Built into M1 (hermeneus) |
| #313 | Prosoche signals | Closed | Built into M4 (daemon) |
| #256 | Delivery retry | Closed | Built into M3 (pylon) |
| #239 | Graph maintenance | Closed | Built into M4 (daemon) |
| #250 | Memory recall quality | Closed | Built into M1 (mneme) |

---

## Project Tracking

### Hackathon Project (Dianoia)

The `proj_c3328a6e7874e4acbfa3bf4f` hackathon project burned down most of its 16 issues. Remaining actionable items (#328, #326, #338) are folded into this plan. The hackathon project can be closed.

### How to Track Progress

This document is the source of truth. Each milestone section includes:
- **Phases** with concrete deliverables
- **Success criteria** — what "done" looks like
- **Exit gates** — what must pass before moving to the next milestone

Progress updates go here as milestones complete. Daily work tracked in `memory/YYYY-MM-DD.md` as always.

### Current Status

Last updated: 2026-03-02

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0a | ✅ **Complete** | Oikos TS migration — PRs #355, #356, #359 merged. Migration script ready (`scripts/migrate-to-oikos.sh`), pending live server execution. |
| M0b | ✅ **Complete** | koina + taxis Rust crates — PR #358 merged. Newtypes, snafu errors, tracing, oikos cascade, path resolution. 30 tests. |
| M1.1 | ✅ **Complete** | mneme SQLite sessions — rusqlite, WAL mode, wire-compatible with TS sessions.db. 19 tests. |
| M1.2a | ✅ **Complete** | CozoDB validation gate — all 5 bench tests pass (relations, HNSW, graph, concurrent R/W, bi-temporal). |
| M1.2 | ⚠️ **Types only** | Knowledge types + Datalog schema templates written. CozoDB integration **deferred** — 3 upstream bugs. See #363 for purpose-built alternative evaluation. |
| M1.3 | ✅ **Complete** | EmbeddingProvider trait + MockEmbeddingProvider. fastembed-rs integration pending. 8 tests. |
| M1.4 | ✅ **Complete** | 6-factor recall scoring engine — vector similarity, recency, relevance, epistemic tier, graph proximity, access frequency. 22 tests. |
| M1.5 | ✅ **Complete** | hermeneus LLM provider trait — CompletionRequest/Response, ToolUse/ToolResult, ThinkingConfig, ProviderRegistry. 13 tests. |
| M1.6 | ✅ **Complete** | hermeneus Anthropic Messages API — streaming SSE parser, retry w/ backoff + jitter, rate-limit Retry-After, thinking + tool_use blocks. 17 tests. PR #367. |
| M2.1a | ✅ **Complete** | organon tool registry — ToolDef, InputSchema, ToolExecutor trait, ToolRegistry with category filtering + hermeneus wire conversion. 6 built-in stubs. 11 tests. PR #366. |
| M2.1b | ✅ **Complete** | nous pipeline skeleton — SessionState, SessionManager, PipelineContext, LoopDetector, GuardResult, TurnResult. 18 tests. |
| M2.1c | ✅ **Complete** | CozoDB absorption analysis — research doc: module deps, FTS feasibility, graph algo inventory, 42 unsafe sites, integration plan. PR #364. |
| M2.1d | ✅ **Complete** | Test expansion — 79 new tests across koina, mneme, nous, taxis + integration tests. PR #365. |
| M2.2 | ✅ **Complete** | Context bootstrap — BootstrapAssembler (oikos cascade), TokenBudget (system/history/turn zones), CharEstimator, SectionPriority (Required > Important > Flexible > Optional), section-aware truncation, tool summary tiers. 14 tests. PR #369. |
| M2.3 | **Next** | CozoDB absorption — fork, patch 3 compile bugs, strip bindings + unused backends, integrate as mneme-engine. GSD in progress (prompt 05). |
| M2.4+ | Not started | Execute stage, tool iteration, distillation, workspace files |
| M3.1a | ✅ **Complete** | symbolon (auth) — JWT sessions (access+refresh), API keys (ale_ format, blake3), argon2id passwords, RBAC (Operator/Agent/Readonly), AuthStore (SQLite), 50 tests. PR #368. |
| M3.1b | ✅ **Complete** | pylon (Axum gateway) — session CRUD, SSE streaming, health check, error→HTTP mapping, tower middleware, mock integration tests. PR #370. |
| M3.2+ | Not started | agora channels (Signal, Slack), delivery reliability |
| M4 | Not started | Blocked on M3 |
| M5 | Not started | Blocked on M4 |
| M6 | Backlog | Independent items, work anytime after M5 |

**Totals:** 9 Rust crates (+ integration-tests + mneme-bench), 331 workspace tests, ~14,000 lines of Rust.

### CozoDB Decision (2026-03-02)

**Decision:** Absorb CozoDB. Fork, patch, strip, integrate as `mneme-engine`.

**Why:** The absorption analysis (PR #364, 877 lines) proved that CozoDB's Datalog engine + integrated HNSW + graph algorithms deliver unified hybrid retrieval that can't be replicated by bolting standalone crates together. rusqlite + standalone HNSW covers ~70% of use cases — but the mandate is the best system we can build, not good enough.

**What we keep:** Datalog query engine, HNSW vector indexes, all 17 graph algorithms (PageRank, Louvain, shortest path, etc.), FTS/BM25 (Option A from analysis — extract tokenizer, strip Chinese-specific code), RocksDB backend, in-memory backend for tests.

**What we strip:** Language bindings (C/Java/Node/Python/Swift/WASM), HTTP server layer, Cangjie Chinese tokenizer (~21K lines of stopwords), 4 unused storage backends (legacy RocksDB, SQLite, Sled, TiKV), FFI wrappers.

**Compile bugs to patch (3):**
1. Unconditional `rayon::spawn` in `lib.rs` (not behind feature flag)
2. `graph_builder` crate broken with rayon 1.10 (`IntoIter`/`Iter` mismatch)
3. `nalgebra` type resolution failures (`OMatrix`, `Dynamic`, `U1`)

**Phased plan:** See `docs/research/cozo-absorption.md` for full 7-phase plan. Prompts 05+ implement it. GSD workflow for the massive phases.

**Risk:** Medium — absorbing 60K lines with 464 unwraps and 49 unsafe sites. Mitigated by phased approach: compile first, strip second, quality-improve third.

---

## Related Documents

| Document | Purpose |
|----------|---------|
| `docs/ARCHITECTURE.md` | Module map, init order, dependency rules |
| `docs/STANDARDS.md` | Code standards for current TS (adapt for Rust) |
| `docs/gnomon.md` | Naming system and philosophy |
| `docs/specs/archive/DECISIONS.md` | Archived spec decisions (33 specs) |
| ~~`docs/specs/43_rust-rewrite.md`~~ | Absorbed — content merged into MODULE DESIGN NOTES and MILESTONES sections above |
| `docs/specs/44_oikos.md` | Detailed oikos spec (directory structure, resolution rules, migration plan) |
| `docs/specs/40_testing-strategy.md` | Testing targets and patterns |
| `docs/specs/41_observability.md` | Logging, metrics, traces architecture |

### Additional References

| Document | Purpose |
|----------|---------|
| `docs/research.md` | Adopted frameworks, architecture repos, ecosystem watch, QA audit provenance |
| `.claude/rules/rust.md` | Coding rules + performance patterns for Claude Code |
