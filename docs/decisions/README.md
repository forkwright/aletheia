# Resolved Design Decisions

> Architecture Decision Records for the Aletheia Rust rewrite.
> All 20 grey areas resolved 2026-02-28. Reviewed through four frames: long-term best for Aletheia, alignment with operator philosophy, no corners cut, gnomon naming integrity.
> CozoDB absorption decision 2026-03-02.

---

## M0 (Foundation)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-01 | Hooks: supplement or override? | **Supplement with explicit `replaces:` opt-in.** | Default additive — shared `on_session_start` runs, then nous-specific runs too. A nous can declare `replaces: shared/hooks/on_session_start.yaml` to take over. Covers both cases without surprise. Aligns with oikos metaphor: household members can take on shared responsibilities, but must explicitly claim them. |
| G-02 | Config format: YAML or TOML? | **YAML.** | Agent-generated config has multi-line strings (system prompts, tool descriptions, context blocks). TOML's multi-line handling is awkward. serde handles both equally. Existing config is all YAML — zero migration cost. |
| G-03 | Template nous in `instance.example/`? | **Yes — `_template/` directory.** | `aletheia add-nous <name>` copies it. Starter SOUL.md with commented sections, empty TELOS.md, empty MNEME.md, .gitkeep in tools/ and hooks/. Without it, scaffold logic lives in Rust code instead of declarative files — worse. |

## M1 (Memory + LLM)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-04 | Voyage-4-large migration? | **Migrate during M1. Clean break.** | 2400 memories × Voyage-4 pricing ≈ $0.50. MoE architecture and shared embedding space are materially better. Start mneme on the right foundation. |
| G-05 | JEPA split across milestones? | **Phases 1-3 in M1. Phase 4 (cross-agent semantic routing) in M4. Phases 5-6 in M6.** | Phase 4 is foundational to multi-nous routing — comparing message embedding to agent memory cluster centroids replaces config-label domain matching. Without it in M4, we'd build multi-nous coordination on the same inherited string-matching pattern. That violates "no inherited debt." Phases 5-6 (goal vectors, collapse prevention) are optimization on a working system. |
| G-06 | Memory extraction: LLM or rules? | **LLM-based with rule-based pre-filter.** | Current quality issues aren't because LLM extraction is wrong — the prompt lets through noise. Tighter extraction prompt + NOISE_PATTERNS pre-filter (already built in Spec 23) gives best of both. Pure rule-based can't handle "is this fact worth remembering?" |

## M2 (Agent Core)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-07 | Browser tool approach? | **`chromiumoxide` CDP wrapper around spawned Chromium.** | Tokio-native, CDP protocol, actively maintained. We only need rendered page fetching and light interaction, not test automation. ~200 LOC wrapper. Falls back to JSON-RPC to external process if insufficient. |
| G-08 | Consolidation triggers? | **Three triggers: turn count (20) + session idle (2hr) + token pressure (75%).** | Token pressure fires consolidation *before* distillation kicks in. They're complementary, not competing: distillation compresses the *conversation* (context management), consolidation promotes *knowledge* to long-term storage (what the agent learned). Excluding token pressure means knowledge accumulated in long sessions gets compressed into distillation summaries instead of properly extracted. |
| G-09 | Agent-writable workspace guardrails? | **Binary: file is writable or not. IDENTITY.md stays operator-owned.** | SOUL.md = essential nature (operator commitment). TELOS.md = purpose (operator commitment). IDENTITY.md = εἶδος, visible form — stable, how others recognize you. If the agent can drift its own visible form, the operator loses identity assurance. Agent self-knowledge (evolving patterns, growth observations) belongs in MNEME.md — that's *memory*, exactly where learned self-understanding should live. IDENTITY is declaration, not discovery. The binary model isn't a shortcut — it's the correct ontological boundary. |

## M3 (Gateway + Channels)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-10 | Pylon: static files or vite proxy? | **Static in prod, vite proxy in dev. `ui.mode: static \| dev` in config.** | One config flag. Axum has both capabilities built in. Zero complexity. |
| G-11 | JWT model? | **15-minute access + 7-day refresh. Auto-refresh in UI.** | Short access tokens limit exposure. Refresh tokens in httpOnly cookies. UI intercepts 401, refreshes, retries. Standard and proven. Long-lived tokens are a known security gap. |

## M4 (Multi-Nous)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-12 | Autonomy gradient scope? | **Per-agent default with per-project override.** | Agent config in oikos sets baseline. Project creation can override. Two config points, clear precedence. Most work uses agent default. |
| G-13 | Task handoff protocol? | **Structured with lightweight schema.** | `{id, from, to, type, context, status, created, updated}` — 8 fields. State machine: created → assigned → in-progress → review → done. Informal `sessions_send` stays for quick coordination. Structured tasks for work that needs tracking. Don't force everything through the protocol. |
| G-14 | Prosoche auto-project creation? | **Draft project creation (auto-expires 48hr). Proactive suggestion with human gate.** | "Notification only" is too passive — contradicts "proactive, not reactive." But auto-creating from noisy signals creates cleanup work. Middle path: prosoche formulates the project as a draft, operator approves or lets it expire. The system does the work of scoping; the human decides whether to pursue. |

## M5 (Cutover)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-15 | Parallel operation period? | **1 week minimum, 2 weeks ideal. Automated comparison.** | Same input to both runtimes, diff outputs. Comparison framework: response quality, latency, recall accuracy, background task completion. Week 1 finds regressions, week 2 builds confidence. |
| G-16 | Rollback plan? | **`aletheia-ts` systemd service available for 30 days post-cutover.** | `systemctl start aletheia-ts` if critical. Costs nothing but disk space. Remove after 30 days with zero fallbacks. |

## M6 (Extensions)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| G-17 | A2A protocol? | **Server-side discovery only initially.** | Expose agent cards at `/.well-known/agent.json`. Don't build client-side delegation until protocol hits 1.0 and there's a real system to talk to. Low effort to expose, high risk to depend on. |
| G-18 | eBPF depth? | **Structured feeds from existing tools first.** | journald, ss, /proc, systemd — all accessible without kernel programming. eBPF for network packet inspection in later phase only if structured feeds prove insufficient. Prove value before investing in kernel complexity. |
| G-19 | NixOS packaging? | **Flake with module inside.** | Standard modern Nix pattern. `nix run github:forkwright/aletheia` works. Module provides `services.aletheia = { enable = true; ... }` for NixOS. Both, not either/or. |
| G-20 | A2UI component sandboxing? | **Structured data API for standard types + WASM-sandboxed custom components via prostheke.** | Standard types (table, chart, progress, kanban): agents emit typed data, UI renders with known-safe Svelte components. Novel visualizations: WASM component with defined input/output contract, sandboxed same as plugins. Same security model as prostheke — consistent architecture. No iframes, no arbitrary HTML, no XSS surface. |

---

## Gnomon Naming Audit

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

## CozoDB Absorption (2026-03-02)

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
