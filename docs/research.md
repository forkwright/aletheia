# Research References

> Cognitive architecture, memory systems, and ecosystem research informing Aletheia's design.
> Separated from PROJECT.md to keep the build plan focused on execution.

---

## Adopted Frameworks

| Framework | Source | Aletheia Application |
|-----------|--------|---------------------|
| CoALA Memory Taxonomy | Princeton (arxiv 2309.02427) | Formalize working/episodic/semantic/procedural as distinct Rust types in mneme |
| Complementary Learning Systems | CLS Survey (arxiv 2512.23343) | Two-speed memory: fast episodic encoding (every turn) + slow semantic extraction (nightly cron = hippocampal replay) |
| Global Workspace Theory | Novel application | Prosoche IS a GWT implementation — formalize competition/selection/broadcast phases |
| ACT-R Activation Retrieval | Anderson 1993 | Memory activation = recency + frequency + contextual fit, not just embedding similarity. Decay over time, increase with use. |
| Recollection-as-Memory | OwlCore Exocortex | Every `recall()` creates association trace — self-improving retrieval. Already in M1.4. |

---

## Patterns to Track (M4+)

| Pattern | Source | When |
|---------|--------|------|
| Spacing-effect review | Cognitive science | Graduated review intervals for memory consolidation (nightly→weekly is coarse) |
| Transactive memory | Social cognition | Each nous maintains model of what other nous know — smarter routing |
| Stigmergic coordination | Pressure-field model (arxiv 2601.08129v2) | Compute "badness" per domain, agents gravitate to high-pressure areas. O(1) coordination. |
| Active Inference | VERSES AI / pymdp | Agents minimize surprise not maximize reward — natural curiosity + caution |
| Skill memory lifecycle | MemOS | Skills should evolve: usage tracking, refinement on outcomes, decay when unused |

---

## Architecture-Similar Repositories

Rust projects to study for patterns, not to copy.

| Repository | Relevance | Key Patterns to Study |
|-----------|-----------|----------------------|
| [qdrant/qdrant](https://github.com/qdrant/qdrant) | **Closest architectural match** — vector DB, Axum, Tokio, multi-crate workspace | Actor model, storage engine, API design |
| [quickwit-oss/quickwit](https://github.com/quickwit-oss/quickwit) | **Best actor model reference** — custom actor framework on Tokio | Pipeline architecture, actor lifecycle, message routing |
| [rust-lang/rust-analyzer](https://github.com/rust-lang/rust-analyzer) | **Best workspace organization** — 30+ flat crates | Incremental computation, crate dependency graph |
| [cozodb/cozo](https://github.com/cozodb/cozo) | Our embedded DB | Datalog engine internals, Rust API, storage backends |
| [tokio-rs/axum](https://github.com/tokio-rs/axum) | Our HTTP framework | SSE, WebSocket, state extraction, middleware, testing |
| [greptime/greptimedb](https://github.com/GreptimeTeam/greptimedb) | **Error handling model** — snafu + Location traces | Large workspace error layering pattern we're adopting |
| [influxdata/influxdb](https://github.com/influxdata/influxdb) | Large async workspace | Query engine, Arrow integration, async patterns at scale |

---

## Research Repositories

| Repository | Value |
|-----------|-------|
| [getzep/graphiti](https://github.com/getzep/graphiti) | Temporal knowledge graph — bi-temporal edges, episode-centric |
| [Arlodotexe/OwlCore.AI.Exocortex](https://github.com/Arlodotexe/OwlCore.AI.Exocortex) | Recollection-as-memory pattern — recall is a write op |
| [MemTensor/MemOS](https://github.com/MemTensor/MemOS) | Memory OS with skill lifecycle (MemCube, scheduling, multi-modal) |
| [OpenSPG/KAG](https://github.com/OpenSPG/KAG) | Knowledge-augmented generation — logical query decomposition |
| [EvoAgentX/EvoAgentX](https://github.com/EvoAgentX/EvoAgentX) | Self-evolving agent workflows |
| [infer-actively/pymdp](https://github.com/infer-actively/pymdp) | Active inference framework |

---

## Ecosystem Watch

Monitor monthly. Any of these could eliminate dependencies or open new capabilities.

- **presage** — Native Rust Signal client (AGPL). Could eliminate signal-cli JVM subprocess. Unstable API.
- **Wassette** (Microsoft) — WASM Components via MCP for sandboxed tool execution. Deny-by-default permissions. Aligns with our wasmtime prostheke design.
- **redb 3.0** — Pure Rust embedded KV (ACID+MVCC). Alternative to CozoDB for simpler storage needs.
- **Official MCP Rust SDK** — `modelcontextprotocol/rust-sdk`. Track alongside rmcp.

---

## QA Research Provenance

All findings from the 2026-02-28 QA audit (18 parallel agents, 3 rounds) have been integrated into permanent locations. Source docs preserved in agent workspace for deep dives.

| Document | Integrated Into |
|----------|----------------|
| 01 (Gap Analysis) | docs/PROJECT.md — milestones, surface inventories |
| 02 (Crates Audit) | docs/PROJECT.md — dependency policy, crate mapping, release profile |
| 03 (Reference Guide) | .claude/rules/rust.md — patterns + code examples |
| 04 (Pitfalls) | .claude/rules/rust.md — pitfalls + code examples |
| 05 (Raw Research) | docs/PROJECT.md + docs/research.md — frameworks, repos, ecosystem |
| 06 (Standards) | docs/STANDARDS.md — philosophy, universal rules, Rust section, CI |
