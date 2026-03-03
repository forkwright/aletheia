# Aletheia — Project Plan

> Roadmap and current status for Aletheia's evolution from TypeScript prototype to Rust production system.
> For decisions see `docs/decisions/`, for standards see `docs/STANDARDS.md`, for triage see `.planning/DISPOSITION.md` (local).
> Last updated: 2026-03-05 — M0a/M0b/M1 complete, M2 core + M3 complete, pipeline wired end-to-end. 849 tests across 15 crates, ~27K lines Rust. mneme v2 Phases 9-11 done.

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

#### mneme v2: The Transformation Arc

v1 absorbed CozoDB into the workspace as `mneme-engine` — it compiles, tests pass, hybrid retrieval works. v2 transforms it from "CozoDB that we absorbed" into Aletheia's memory engine.

Three movements:

1. **Integration (current)** — Bug fixes, recall pipeline, typed query builder. The engine connects to agent cognition.
2. **Transformation** — Crate consolidation (`mneme-engine` + `graph-builder` → `mneme`), API reshaped around knowledge and memory (Fact, Association, Confidence, TemporalQuery — not DataValue, NamedRows), HNSW rewritten in-memory, CSV/JSON import restored as Rust-native, C/C++ dependencies evaluated for pure-Rust replacement.
3. **Intelligence** — Knowledge extraction from conversations, conflict resolution on write, temporal decay, Louvain-based consolidation. Framed as **recursive evolution**: the engine co-evolves with the operator and environment. Fitness is contextual, variation is preserved, ecological succession replaces aggressive pruning.

Design principles:
- **Reshape, don't shrink.** The 30+ graph algorithms, bi-temporal reasoning, Datalog optimizer, BM25 — all stay. We use 15% today but the roadmap reaches into the rest. Strip dead code (removed platform backends, FFI shims), not dormant capability.
- **Rust-native where possible.** Evaluate pure-Rust alternatives for every C/C++ dependency without sacrificing quality. RocksDB → redb/fjall if benchmarks hold.
- **API speaks our language.** The public surface thinks in knowledge, memory, recall, and confidence — not generic Datalog relations.

Detailed phase plans and requirements in `.planning/` (local-only, not tracked in repo).

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
| 4.2 | `daemon` | Per-nous cron, evolution, prosoche, graph maintenance, morning digest. **Includes recursive behavioral evolution:** tool call observation → pattern extraction → fitness-scored instincts (contextual, not abstract) → niche differentiation → ecological succession. Not self-improvement — co-evolution with operator and environment. |
| 4.3 | `dianoia` | Planning FSM from first principles. **Core redesign:** workspace model (not pipeline). 3 operating modes: full project (research→execute→verify), quick task (appetite-based, time-boxed), and autonomous background. Skip any phase that adds no value. State machine with exhaustive `match` on typed enums — every transition explicit. |
| 4.4 | Cross-nous coordination | Competence-aware routing, structured task handoff, priority queue |
| 4.5 | Tool observability + lifecycle hooks | Pre/post-tool hooks with structured tracing spans. **Lifecycle hook system:** PreToolUse, PostToolUse, SessionStart/End, PreCompact, TaskCompleted events. Three hook types: command (shell), prompt (LLM eval), inline (Rust callback). Oikos cascade for hook config (shared/hooks/ + nous/{id}/hooks/). Foundation for auto-format, pre-compaction memory safety, verification hooks, behavioral evolution. |

**Absorbed ideas:**
- **Spec 27 Phase 4 (Cross-Agent Semantic Routing):** Route messages to the correct nous by comparing message embedding to each agent's memory cluster centroid. Replaces config-label domain matching with embedding-space proximity — foundational to correct multi-nous coordination. (Moved from M6 per G-05: without this, M4 ships with inherited string-matching patterns.)
- **Spec 39 (Autonomy Gradient):** 4-level configurable autonomy (confirm-all → confirm-never) in dianoia. Per-agent default, per-project override (G-12). Auto-advance on read-only phases.
- **Spec 42, Gaps 1-3 (Feedback Loops):** Competence scores influence routing. Kritikos flags feed back to competence model. Prosoche creates draft projects for high-urgency signals (auto-expire 48hr, per G-14).
- **Spec 42, Gap 2 (Task Handoff):** Structured lightweight schema: `{id, from, to, type, context, status, created, updated}`. State machine: created → assigned → in-progress → review → done. Context travels with handoff. Informal `sessions_send` remains for quick coordination (G-13).
- **Issue #313 (Prosoche signals):** Activity tracking, HEARTBEAT_OK dedup, work signals. Built into daemon.
- **Issue #239 (Graph maintenance):** Automated CozoDB graph QA, vector dedup, orphan purge. Per-nous cron schedule.
- **ECC Instinct System (INST-01..11, evolution-reframed):** Tool call observation → pattern extraction → fitness-scored instincts (contextual, not abstract) with niche identification and per-nous scoping → ecological succession + speciation + co-evolutionary recalibration. Reframed from self-improvement (optimize toward fixed target) to recursive evolution (co-evolve with operator and environment). Fitness through use, not scoring. Variation preserved, not pruned. Source: everything-claude-code continuous-learning-v2, reframed.
- **ECC Tool Observability (OBS-01..04):** Pre/post-tool hook points, tool call tracing spans, async hook registration API. Source: everything-claude-code hooks architecture.
- **Rowboat Knowledge Ingestion (KI-01..05):** Source monitoring + incremental change detection (mtime+hash) + batch extraction pipeline + entity deduplication + strictness levels. Informs Phase 15 knowledge lifecycle. Source: rowboat build_graph, graph_state, strictness_analyzer.
- **Rowboat Background Scheduling (SCHED-01..03):** Cron + window + once scheduling for autonomous agent runs. Informs daemon cron capabilities. Source: rowboat agent-schedule/runner.
- **Orbital Agent Safety (ASAFE-01..02):** Tool repetition detection (loop breaker) + checkpoint/rollback for destructive ops. Source: Orbital ToolRepetitionDetector, checkpoint services.
- **Orbital Distillation Optimization (DIST-01..02):** Model downshift for cheaper distillation + structured condensing prompt template. Source: Orbital condense/index.ts, custom condensing handler.
- **Orbital Eval Schema (EVAL-05..06):** Concrete data model: runs→tasks→metrics→toolErrors. CozoDB storage. Source: Orbital packages/evals DB schema.
- **Orbital Manual Skills (SKILL-01):** Static skill files in workspace, loadable on demand. Bridges gap before instinct speciation produces stable behavioral patterns. Source: Orbital .agent/skills/ pattern.
- **Claude Cowork Lifecycle Hooks (HOOK-01..07):** Event-driven hook registry for tool and session lifecycle. PostToolUse on Write/Edit enables auto-format/lint. PreCompact guarantees memory file write before distillation (solves amnesia). TaskCompleted enables verification assertions. Oikos cascade for hook configuration. Source: Claude Cowork plugin hooks architecture.
- **Ralph Wiggum Execution Resilience (EXEC-01..04):** Stuck detection (same error twice → escalate, not retry), completion assertions (sub-agents state what they achieved), max iteration caps with blocker documentation, opt-in re-injection on Stop for iterative tasks. Source: Ralph Wiggum iterative pattern, Claude Cowork Stop hook.
- **Claude Cowork Skill Modularity (LOOP-03):** Decompose monolithic SOUL.md/AGENTS.md into composable skill files discovered via oikos cascade. assemble-context loads them dynamically. Source: Claude Cowork SKILL.md pattern.

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
- **Spec 40 (Testing Strategy):** Coverage targets, integration patterns, contract tests adapted for `cargo test`. CI enforcement. **Includes behavioral eval framework (EVAL-01..04):** capability evals, recall quality scoring (precision@k, recall@k), model-graded response quality, regression tracking per-commit. Source: ECC eval-harness.
- **Spec 41 (Observability):** tracing crate with spans, layers, journald integration. Prometheus/OpenTelemetry metrics. Structured query: "why was that turn slow?" **Tool call spans (OBS-03)** wired as tracing spans — queryable tool performance and error analysis.
- **ECC Plugin Architecture (PLUG-01..03):** Plugin manifest schema (TOML), lifecycle events (SessionStart/End, Pre/PostTool, Pre/PostDistillation), WASM isolation with capability-based permissions. Informs prostheke design.

**Success criteria:** All existing functionality works. Test suite passes. Binary deploys via `scp + systemctl`.

**Exit gate:** One full week of production operation with zero TS runtime fallback.

**Post-cutover:** Remove `infrastructure/runtime/` and `infrastructure/memory/`. All state under `instance/`.

---

### M6: Platform Extensions (Post-Rewrite)

**Goal:** Capabilities that require a stable Rust platform to build on.

| Phase | Work | Origin |
|-------|------|--------|
| 6.1 | Theatron — composable operations system, widget engine, nous-typed views, agent-authored dashboards | Spec 45 (absorbs Specs 30, 43b) |
| 6.2 | Interop — A2A protocol, workflow engine, IDE integration | Spec 22 |
| 6.3 | Aletheia Linux — eBPF/DBus sensing, NixOS module | Spec 24, Issue #332 |
| 6.4 | Embedding space intelligence — JEPA goal vectors, collapse prevention, embedding health monitoring | Spec 27 (phases 5-6) |
| 6.5 | UI layout overhaul | Spec 29, Issue #328 |
| 6.6 | TUI enhancements — fuzzy filter, F2 overlay, OSC 8, plan widget | Issue #326 |
| 6.7 | LSP integration — rust-analyzer/pyright in agent loop for real-time diagnostics | LOOP-01 |
| 6.8 | Path display normalization — translate workspace paths to user-friendly relative paths in webchat | LOOP-02 |

These are not sequenced — they can be worked in parallel once the platform is stable. Each is a self-contained project.

---

## Project Tracking

### How to Track Progress

This document is the source of truth. Each milestone section includes:
- **Phases** with concrete deliverables
- **Success criteria** — what "done" looks like
- **Exit gates** — what must pass before moving to the next milestone

Progress updates go here as milestones complete. Daily work tracked in `memory/YYYY-MM-DD.md` as always.

### Current Status

Last updated: 2026-03-04

| Milestone | Status | Notes |
|-----------|--------|-------|
| M0a | ✅ **Complete** | Oikos TS migration — PRs #355, #356, #359 merged. Migration script ready (`scripts/migrate-to-oikos.sh`), pending live server execution. |
| M0b | ✅ **Complete** | koina + taxis Rust crates — PR #358 merged. Newtypes, snafu errors, tracing, oikos cascade, path resolution. 30 tests. |
| M1.1 | ✅ **Complete** | mneme SQLite sessions — rusqlite, WAL mode, wire-compatible with TS sessions.db. 19 tests. |
| M1.2a | ✅ **Complete** | CozoDB validation gate — all 5 bench tests pass (relations, HNSW, graph, concurrent R/W, bi-temporal). |
| M1.2 | ⚠️ **Types only** | Knowledge types + Datalog schema templates written. CozoDB integration **deferred** — 3 upstream bugs. See #363 for purpose-built alternative evaluation. |
| M1.3 | ✅ **Complete** | EmbeddingProvider trait + MockEmbeddingProvider + FastEmbedProvider (BAAI/bge-small-en-v1.5, feature-gated). 85 mneme tests. PR #374. |
| M1.4 | ✅ **Complete** | 6-factor recall scoring engine — vector similarity, recency, relevance, epistemic tier, graph proximity, access frequency. 22 tests. |
| M1.5 | ✅ **Complete** | hermeneus LLM provider trait — CompletionRequest/Response, ToolUse/ToolResult, ThinkingConfig, ProviderRegistry. 13 tests. |
| M1.6 | ✅ **Complete** | hermeneus Anthropic Messages API — streaming SSE parser, retry w/ backoff + jitter, rate-limit Retry-After, thinking + tool_use blocks. 17 tests. PR #367. |
| M2.1a | ✅ **Complete** | organon tool registry — ToolDef, InputSchema, ToolExecutor trait, ToolRegistry + 4 workspace executors (read/write/edit/exec) with path traversal protection. 14 tests. PRs #366, #375. |
| M2.1b | ✅ **Complete** | nous pipeline skeleton — SessionState, SessionManager, PipelineContext, LoopDetector, GuardResult, TurnResult. 18 tests. |
| M2.1c | ✅ **Complete** | CozoDB absorption analysis — research doc: module deps, FTS feasibility, graph algo inventory, 42 unsafe sites, integration plan. PR #364. |
| M2.1d | ✅ **Complete** | Test expansion — 79 new tests across koina, mneme, nous, taxis + integration tests. PR #365. |
| M2.2 | ✅ **Complete** | Context bootstrap — BootstrapAssembler (oikos cascade), TokenBudget (system/history/turn zones), CharEstimator, SectionPriority (Required > Important > Flexible > Optional), section-aware truncation, tool summary tiers. 14 tests. PR #369. |
| M2.3 | ✅ **Complete** | CozoDB absorption — mneme-engine crate with forked CozoDB, 3 compile bugs patched, unsafe sites isolated in graph-builder. Phases 1-9 complete (PRs #378, #407, #422). Hybrid retrieval (vector + graph + BM25), idiom migration, clippy clean. |
| M2.4 | ✅ **Complete** | taxis config loading — figment-based YAML cascade, AletheiaConfig structs, resolve_nous() merger, env overrides, camelCase compat. 37 tests. PR #379. |
| M2.5 | ✅ **Complete** | nous execute stage — core LLM call + tool dispatch loop, LoopDetector integration, signal classification, `run_pipeline` entry point. ThinkingConfig in hermeneus. 78 tests. PR #381. |
| M3.1a | ✅ **Complete** | symbolon (auth) — JWT sessions (access+refresh), API keys (ale_ format, blake3), argon2id passwords, RBAC (Operator/Agent/Readonly), AuthStore (SQLite), 50 tests. PR #368. |
| M3.1b | ✅ **Complete** | pylon (Axum gateway) — session CRUD, SSE streaming, health check, JWT auth middleware via symbolon (Bearer required except /health), claims extraction. 25 tests. PRs #370, #376. |
| M3.2 | ✅ **Complete** | agora channel registry — ChannelProvider trait, ChannelRegistry with typed routing, Signal JSON-RPC client (SignalClient), SignalProvider implementation. 19 tests. PR #380. |
| M3.3a | ✅ **Complete** | nous NousActor — Tokio actor model with inbox, lifecycle states (Active/Idle/Dormant), NousManager, message dispatch. 33 tests. PR #382. |
| M3.3b | ✅ **Complete** | melete distillation — context distillation engine, compression strategies, token budget management, session continuity. 34 tests. PR #383. |
| M3.3c | ✅ **Complete** | agora Signal listener — inbound message routing, SignalListener with JSON-RPC subscribe, message-to-nous dispatch. 41 tests. PR #384. |
| M3.4a | ✅ **Complete** | nous history stage — budget-aware conversation history loading, system/tool message filtering, truncation marking. 7 tests. PR #419. |
| M3.4b | ✅ **Complete** | nous finalize stage — persist user/assistant/tool messages, token usage recording, non-fatal error handling. 6 tests. PR #420. |
| M3.4c | ✅ **Complete** | agora inbound routing — MessageRouter with 5-priority binding resolution (group→source→channel→global→none), session key template expansion, background dispatch task. PR #421. |
| M3.4d | ✅ **Complete** | End-to-end integration tests — HTTP→pipeline→provider→persistence round-trip with mock providers, JWT auth validation, bootstrap assembly verification. 7 tests. PR #418. |
| M3.4e | ✅ **Complete** | mneme hybrid search bug fixes — graph score aggregation (`graph_raw` + `sum()`), RRF rank encoding (0→-1), empty seed_entities handling. 4 tests. PR #422. (Completes mneme v2 Phase 9.) |
| M3.5a | ✅ **Complete** | KnowledgeVectorSearch bridge — feature-gated `VectorSearch` trait adapter for recall pipeline. 4 tests. PR #444. |
| M3.5b | ✅ **Complete** | Recall pipeline wiring — `EmbeddingSettings` in taxis config, embedding provider creation in `main.rs`, wired to `NousManager`. 2 tests. PR #446. (mneme v2 Phase 10 partial.) |
| M3.5c | ✅ **Complete** | Iterative recall — 2-cycle retrieval with terminology discovery, gap detection, `LoopDetector` ring buffer. Tool repetition detection (`ASAFE-01`). 8+ tests. PR #443. (Completes mneme v2 Phase 10.) |
| M3.5d | ✅ **Complete** | Typed Datalog query builder — field enums, fluent API (`QueryBuilder`/`PutBuilder`/`SelectBuilder`), ~10 queries migrated from string constants. Regression tests. PR #447. (Completes mneme v2 Phase 11.) |
| M3.5e | ✅ **Complete** | Execution resilience — `StuckDetector` with error pattern normalization, iteration caps, blocker file writing. 5 tests. PR #442. (Dianoia orchestrator.) |
| M3.6+ | Not started | Delivery reliability, cross-nous sessions, knowledge extraction (mneme v2 Phase 12+) |
| M4 | Not started | Multi-nous, roles, daemon, melete, dianoia |
| M5 | Not started | Plugins, portability, cutover |
| M6 | Backlog | Independent items, work anytime after M5 |

**Totals:** 15 crate directories (11 application + `aletheia` binary + `graph-builder` + `integration-tests` + `mneme-bench`), 849 tests (`#[test]` + `#[tokio::test]`), ~27K lines of Rust (+46K vendored CozoDB in mneme-engine).

---

## Related Documents

| Document | Purpose |
|----------|---------|
| `docs/ARCHITECTURE.md` | Module map, init order, dependency rules |
| `docs/STANDARDS.md` | Code standards + project governance (commit, deviation, research rules) |
| `docs/decisions/` | Architecture Decision Records — G-01 through G-20, gnomon audit, CozoDB absorption |
| `docs/LESSONS.md` | Operational lessons learned (16 rules earned through failure) |
| `.planning/DISPOSITION.md` | Spec & issue triage record (local-only) — what was absorbed, retained, or closed |
| `docs/gnomon.md` | Naming system and philosophy |
| `docs/specs/44_oikos.md` | Detailed oikos spec (directory structure, resolution rules, migration plan) |
| `docs/specs/40_testing-strategy.md` | Testing targets and patterns |
| `docs/specs/41_observability.md` | Logging, metrics, traces architecture |
| `docs/specs/archive/DECISIONS.md` | Archived spec decisions (33 specs) |
| `docs/research/cozo-absorption.md` | CozoDB absorption analysis and 7-phase plan |

### Additional References

| Document | Purpose |
|----------|---------|
| `docs/research.md` | Adopted frameworks, architecture repos, ecosystem watch, QA audit provenance |
| `.claude/rules/rust.md` | Coding rules + performance patterns for Claude Code |