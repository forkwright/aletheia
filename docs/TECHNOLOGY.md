# Technology Decisions

Technology decisions and dependency policy.

See [ARCHITECTURE.md](ARCHITECTURE.md) for module boundaries, [PROJECT.md](PROJECT.md) for the roadmap.

---

## Decision Table

| Layer | Choice | Replaces | Rationale |
|-------|--------|----------|-----------|
| Language | Rust | TypeScript + Python | No GC, single binary, true concurrency |
| Async | Tokio | Node.js event loop | Real threads, Axum built on it |
| HTTP | Axum | Hono | SSE built-in, cleaner middleware |
| HTTP client | reqwest | node-fetch | Async, connection pooling, Anthropic + channel calls |
| Anthropic API | Own client (~600 LOC) | @anthropic-ai/sdk | Stable API, reqwest + SSE, adaptive thinking, Tool Search Tool |
| Unified store | Embedded Datalog+HNSW engine | Qdrant + Neo4j | Rust-native embedded, Datalog, HNSW vectors + graph + relations in one DB. Zero external services. Located in `mneme/src/engine/`, gated behind `mneme-engine` feature. |
| Embeddings | candle + `EmbeddingProvider` trait | fastembed-rs (ONNX) | Pure Rust, no C++ deps. Default: BAAI/bge-small-en-v1.5 (384 dims). Feature-gated behind `embed-candle`. |
| Memory | Direct (no abstraction) | KnowledgeStore (embedded engine) | ~50 LOC replaces the library |
| Sessions | rusqlite + bundled | better-sqlite3 | WAL mode, no native addon |
| Encryption | XChaCha20Poly1305 | None (plaintext) | Per-message encryption at rest, ~700ns overhead, zero plaintext on disk |
| Config | figment + serde + validator | Zod | figment handles oikos cascade natively (YAML + env + CLI, hierarchical merge). By Rocket author. |
| IDs | ulid + uuid | uuid | ulid for time-sorted data (sessions, messages, memories) - lexicographic sort = natural ordering. uuid v4 for non-temporal. |
| Errors | snafu + anyhow | AletheiaError hierarchy | snafu for library/mid-level enums (context wrapping, Location-based virtual stack traces, multiple variants from same source type - GreptimeDB pattern). anyhow for application entry. |
| Logging | tracing + Langfuse | tslog | Spans, layers, OpenTelemetry. Langfuse for LLM-specific traces. |
| CLI | clap | Commander | Compile-time validation |
| JSON | serde_json | — | Standard Rust JSON; sonic-rs was evaluated but not adopted |
| Hashing | blake3 + foldhash | crypto.createHash / ahash | blake3 for content hashing (dedup, loop detection). foldhash for HashMap keys. |
| Secrets | secrecy | None | `SecretString` / `SecretVec` - zeroizes on drop, redacts in Debug |
| Hot reload | arc-swap + notify | SIGUSR1 | arc-swap for zero-downtime config swap. notify for file watching. |
| Strings | compact_str | String | 24-byte inline for short strings (agent names, tool names, domain tags) |
| Enums | strum | None | Derive Display, EnumString, EnumIter for all typed enums |
| Git | gix | — | Rust-native git for workspace auto-commit. No subprocess. |
| Password | argon2 | — | Password hashing for symbolon. Memory-hard. |
| Cron | cron + jiff | — | cron for schedule parsing. jiff for time/date (by BurntSushi). |
| Concurrent maps | papaya | DashMap | Lock-free, better scaling under contention |
| Seccomp | extrasafe | None | Declarative syscall filtering for sandboxed tools |
| HTML | dom_smoothie | scraper | Readability extraction + HTML-to-Markdown, single pass |
| Browser | chromiumoxide | None | CDP wrapper for headless Chromium, indexed DOM |
| Event bus | tokio::sync::broadcast | EventEmitter | Typed, backpressure-aware. `watch` for latest-value status. |
| Plugins | WASM (wasmtime) | Dynamic JS import() | Sandboxed, portable, any-language |
| MCP | rmcp 1.2.0 pinned exact | @modelcontextprotocol/sdk | MCP SDK for external AI agent integration via diaporeia. Pre-1.0 - pin exact, wrap in trait. |
| Testing | proptest + cargo-llvm-cov | vitest | Property-based testing and coverage |
| TUI | ratatui + crossterm | Svelte 5 web UI | Terminal-native, embedded in workspace, zero web dependencies |

---

## Dependency Policy

~55 direct crates across the workspace. Each crate uses 5-15. Lean for the scope.

### Pinning Rules

- **Unstable crates** (pre-1.0, aggressive releases): pin exact version. Wrap in trait.
  - `wasmtime` - monthly major versions. Pin exact.
  - `rmcp` - 5 minor releases in 6 weeks. Pin exact. `McpProvider` trait.
  - `candle-*` - pinned exact (`=0.9.2`). Pure Rust, no C++ deps.
  - `chromiumoxide` - niche. Pin exact.
- **Stable crates** (1.0+): pin minor (`"1.49"` not `"=1.49.0"`).
- **Never vendor** unless forced by platform issues. Cargo.lock suffices.

### Corrections from Audit

- `serde_yml` is banned (unsound unsafe) - use `serde_yaml` if YAML parsing is needed
- `async-trait` crate is unnecessary - use native `async fn in trait` (Rust 1.75+)
- `thiserror` replaced by `snafu` for library crates (GreptimeDB pattern)

### Cross-Compilation Notes

- `candle`: pure Rust, cross-compiles cleanly. Feature-gated behind `embed-candle`.
- `serde_json`: standard, no platform-specific concerns.
- `chromiumoxide`: requires Chromium on host. Feature-gate behind `browser`.
- `extrasafe` (seccomp): Linux-only. Feature-gate behind `sandbox-seccomp`.

---

## Crate-to-Module Mapping

| Crate | Key Dependencies |
|-------|-----------------|
| **koina** | snafu, tracing, tracing-subscriber |
| **taxis** | koina, figment, serde, serde_json, snafu, tracing |
| **mneme** | koina, snafu, serde, tracing, ulid, rusqlite (sqlite), candle-core/nn/transformers (embed-candle), jiff, hnsw_rs, fjall |
| **hermeneus** | koina, taxis, reqwest, reqwest-eventsource, serde_json, tokio, secrecy |
| **organon** | koina, taxis, hermeneus, tokio, gix, extrasafe, chromiumoxide |
| **nous** | koina, taxis, mneme, hermeneus, organon, melete, tokio, ulid, compact_str |
| **dianoia** | koina, taxis, mneme, hermeneus, nous, rusqlite |
| **pylon** | koina, taxis, nous, axum, tower, tower-http, symbolon, serde_json, chacha20poly1305 |
| **symbolon** | koina, taxis, rusqlite, jsonwebtoken, argon2 |
| **agora** | koina, taxis, nous, tokio (semeion: tokio::process, slack: tokio-tungstenite) |
| **daemon** | koina, taxis, nous, mneme, cron, notify, arc-swap |
| **melete** | koina, taxis, mneme, hermeneus, nous |
| **tui** | ratatui, crossterm, reqwest, tokio |
