# Unified Gap Analysis: Aletheia vs the Field

## Context

Eight systems compared to identify features worth adopting into Aletheia. Deep dives on Claude Code, OpenClaw, Letta, CrewAI, and AgentScope. Targeted reviews of memU (memory patterns), LangGraph (state management), and OwnPilot (security architecture).

**Aletheia** — Self-hosted multi-agent system. 6 persistent nous + ephemeral spawns. Signal + webchat + MCP channels. Mem0 + Qdrant + Neo4j + sqlite-vec memory. Prosoche daemon (proactive attention). Self-observation (competence model, uncertainty tracker, calibration). Distillation pipeline. 28 built-in tools + dynamic loading. Event bus (15 events). Cron, watchdog. Gateway with 50+ endpoints. ~328KB runtime. Stack: Hono, better-sqlite3, @anthropic-ai/sdk, Zod, Commander.

**Claude Code** — Anthropic's CLI agent (Apache 2.0). Single agent + ephemeral subagents. Terminal-only. Declarative hook system (15+ events). Plugin marketplace.

**OpenClaw** — Multi-channel agent framework (MIT). Single agent, multi-session. 17+ channels. 38 extension plugins. Docker sandbox. ACP IDE bridge. Auth profile rotation. ~406K LOC TypeScript.

**Letta** (MemGPT) — Stateful agent platform (Apache 2.0). Self-editing memory (agents modify their own memory blocks via tools). Sleep-time compute. Agent File (.af) serialization. Three-tier memory (core/archival/recall). ~40k stars.

**CrewAI** — Multi-agent orchestration (MIT). Role-based agents with Crews (autonomous teams) + Flows (deterministic workflows). Unified memory with composite scoring. A2A protocol. 40+ event types. ~57k stars.

**AgentScope** — Production agent framework (Apache 2.0, Alibaba). "Agent as API" pattern. A2A + MCP dual protocol. Pub/Sub MsgHub. Dual-layer memory. Realtime voice. Finetuning/RL. ~12k stars.

**memU** — Memory infrastructure for always-on agents. Hierarchical filesystem memory (Category → Item → Resource). Salience scoring. Tiered retrieval with sufficiency gates. Rust + Python hybrid.

**LangGraph** — State graph orchestration (MIT, LangChain). Checkpointing with version-based triggers. Time-travel debugging. Reducer pattern. Human-in-the-loop interrupts.

**OwnPilot** — Privacy-first personal AI assistant. AES-256-GCM encrypted memory. Tamper-evident hash chain audit. PII detection/redaction. 4-layer sandboxed execution.

**Constraints:**
- Channels: Signal + webchat only (no new channel integrations)
- Tools: ALL agents get ALL tools (persistent nous — no per-agent restrictions)
- Evaluate: Docker sandbox, ACP/IDE integration, Android app as webui port, sleep-time compute, A2A protocol, deterministic workflows, full security hardening
- Skip: WhatsApp, Telegram, Discord, Slack, Matrix, iOS/macOS apps, voice/realtime agents

---

## Architecture Comparison

### Core Architecture

| Dimension | Claude Code | OpenClaw | Letta | CrewAI | AgentScope | Aletheia |
|-----------|-------------|----------|-------|--------|------------|----------|
| **Deployment** | Local CLI | Server + CLI + mobile | Server (FastAPI) | Library/Framework | Library/Framework | Self-hosted server |
| **Agent model** | Single + subagents | Single, multi-session | Multi-agent groups | Role-based crews | Agent-as-API | 6 persistent nous + ephemeral |
| **Language** | TypeScript | TypeScript | Python | Python | Python | TypeScript |
| **Memory** | Session JSONL | sqlite-vec + MMR | Core blocks + archival + recall | Unified (semantic+recency+importance) | Working + long-term (Mem0/ReMe) | Mem0 + Qdrant + Neo4j + sqlite-vec |
| **Proactivity** | None | Heartbeat + wake | Sleep-time compute | None | None | Prosoche daemon + cron + foresight |
| **Self-observation** | None | None | None | None | None | Competence model, calibration, signals |
| **Sandbox** | None | Docker | Tool execution sandbox | None | None | None |
| **Protocols** | None | None | MCP client | A2A + MCP | A2A + MCP | MCP server |

### Memory Systems

| Dimension | OpenClaw | Letta | CrewAI | AgentScope | memU | Aletheia |
|-----------|----------|-------|--------|------------|------|----------|
| **Vector search** | sqlite-vec + MMR | Embedding + BM25 hybrid | LanceDB + composite scoring | Qdrant/Pinecone | SQLite + pgvector | Qdrant (Voyage-3-large) |
| **Graph** | None | None | Neo4j (entity) | Neo4j (ReMe) | None | Neo4j (28 rel types, PageRank) |
| **Self-editing** | None | Agent modifies own blocks | LLM-analyzed save/recall | None | None | Extraction via Haiku |
| **Diversity** | MMR re-ranking | None | LLM exploration rounds | None | Tiered sufficiency gates | Query rewriting |
| **Scoring** | Cosine + temporal decay | Cosine + BM25 | Semantic + recency + importance | Cosine | Similarity × reinforcement × decay | Cosine + dedup (0.92) |
| **Consolidation** | None | Summary on overflow | Auto-dedup (0.85 threshold) | None | Content hash + reinforcement | Consolidation daemon + decay |
| **Cross-agent** | None | Shared blocks in groups | Crew-level shared | MsgHub broadcast | None | Blackboard + shared Qdrant/Neo4j |

### Orchestration & Workflows

| Dimension | Claude Code | CrewAI | AgentScope | LangGraph | Aletheia |
|-----------|-------------|--------|------------|-----------|----------|
| **Workflow model** | None | Flows (event-driven, persistent) | MsgHub (pub/sub) + pipelines | State graph (checkpointed) | Event bus + cron |
| **State persistence** | None | SQLite (flow state) | JSON/Redis/SQLAlchemy sessions | Checkpointer (PG/SQLite/memory) | SQLite sessions.db |
| **Human-in-loop** | Permission modes | Flow pause + resume | Approval callbacks | Interrupt before/after nodes | Approval gate API |
| **Deterministic routing** | None | start → listen → router → branch | Sequential/fanout pipelines | Graph edges + conditions | Cron schedules |
| **Time-travel** | None | None | None | Checkpoint history browsing | None |
| **A2A protocol** | None | Remote agent delegation | A2A server + client | None | None |

### Security

| Dimension | OpenClaw | OwnPilot | Aletheia |
|-----------|----------|----------|----------|
| **Memory encryption** | None | AES-256-GCM at rest, PBKDF2 key derivation | None |
| **Audit trail** | None | Hash chain (SHA-256 linked events) | Audit log table (no integrity chain) |
| **PII handling** | None | 15+ detectors, confidence scoring, redaction modes | None |
| **Code sandbox** | Docker (config-hash, env sanitization, non-root) | 4-layer (patterns → permissions → approval → isolation) | None |
| **Path safety** | Symlink prevention, env sanitization | Directory traversal prevention, workspace isolation | SSRF guard only |

---

## Deconflicted Gaps

Where multiple systems have a feature Aletheia lacks, the best approach is selected.

### DECONF-1: Hook System

**Claude Code:** Declarative `hooks.json`, 15+ events, shell handlers via JSON stdin/stdout, exit code semantics.
**OpenClaw:** TypeScript `registerInternalHook()` with `type:action` naming, 5 lifecycle types.
**AgentScope:** 6 hook types on agent base class (pre/post reply/print/observe), class + instance registration.

**Recommendation: Hybrid (CC protocol + OC naming + AgentScope granularity).** CC's JSON stdin/stdout protocol for shell interop. OC's `noun:verb` naming (matches Aletheia's existing convention). AgentScope's pre/post pattern for per-stage hooks. Wire into existing event bus.

---

### DECONF-2: Command Definitions

**Claude Code:** `.md` files with YAML frontmatter, `$ARGUMENTS` substitution.
**OpenClaw:** Code-registered via `registerCommand()`.
**CrewAI:** Decorator-based (`@agent`, `@task`, `@crew`) with YAML config files.

**Recommendation: Claude Code.** `.md` with frontmatter fits Aletheia's workspace Markdown patterns. Extend existing `CommandRegistry` to scan `shared/commands/*.md`.

---

### DECONF-3: Tool Restrictions

**Claude Code:** Glob patterns per command/agent.
**OpenClaw:** `ownerOnly` + `ActionGate`.
**CrewAI:** Per-task tool lists, agent-level tool binding.

**Recommendation: Claude Code globs, ephemeral-only.** Wire `EphemeralSpec.tools` through pipeline resolve. Never apply to persistent nous.

---

### DECONF-4: Memory Search Quality

**OpenClaw:** MMR re-ranking with Jaccard overlap, temporal decay.
**CrewAI:** Composite scoring (semantic 0.5 + recency 0.3 + importance 0.2), LLM-driven exploration rounds, consolidation.
**memU:** Tiered retrieval with sufficiency gates, salience scoring (similarity × reinforcement × recency_decay), content hash dedup.
**Letta:** Hybrid semantic + BM25 full-text, tag filtering, temporal range queries.

**Recommendation: Hybrid (CrewAI scoring + memU gates + OpenClaw MMR).**
1. **Composite scoring** (CrewAI) — add recency and importance weights to Mem0 results. ~80 LOC in sidecar.
2. **Sufficiency gates** (memU) — stop searching after category-level results if enough context. ~50 LOC.
3. **MMR diversity** (OpenClaw) — re-rank final results for diversity. ~120 LOC.
4. **Content hash dedup** (memU) — prevent near-identical extraction at write time. ~30 LOC.

---

### DECONF-5: Multi-Agent Coordination Patterns

**CrewAI:** Crews (autonomous teams) + Flows (deterministic event-driven workflows with persistence).
**AgentScope:** MsgHub (pub/sub auto-broadcast) + sequential/fanout pipelines.
**LangGraph:** State graph with checkpointing, reducers for concurrent writes, time-travel debugging.
**Letta:** Sleep-time multi-agent groups (background participant processing).

**Recommendation: CrewAI Flows pattern adapted to Aletheia's event bus.** Aletheia already has the event bus and cron. What's missing is declarative workflow definitions with state persistence and conditional routing. Implement as a `WorkflowEngine` that reads workflow definitions (YAML or JSON) and maps steps to event bus listeners. LangGraph's reducer pattern is valuable for merging parallel agent outputs — add to `sessions_dispatch`. Time-travel is interesting but low-priority.

---

### DECONF-6: Security Hardening

**OpenClaw:** Docker sandbox (exec isolation), non-root containers, env sanitization.
**OwnPilot:** AES-256-GCM memory encryption, hash chain audit, PII detection, 4-layer sandbox.

**Recommendation: OwnPilot's approach is more comprehensive.** Adopt all three security layers:
1. **Encrypted memory** — AES-256-GCM for Mem0 vectors and Neo4j properties. ~200 LOC.
2. **Hash chain audit** — Add checksum + previousChecksum to audit log table. ~100 LOC.
3. **PII detection** — Run detector on memories before storage and on Signal outbound. ~300 LOC.

Docker sandbox (OpenClaw) is complementary — addresses execution isolation, not data protection.

---

### DECONF-7: Agent State Serialization

**Letta:** Agent File (.af) — JSON export of agent + memory blocks + message history + tools + MCP configs. ID remapping on import.
**LangGraph:** Checkpointing with thread_id + namespace hierarchy. Fork conversations.
**AgentScope:** `state_dict()` / `load_state_dict()` pattern (PyTorch-inspired).

**Recommendation: Letta's Agent File adapted for nous.** Export a nous as a portable JSON file containing: identity, SOUL.md, memory blocks (from Neo4j + Qdrant), tool configs, workspace files, recent session history. Import creates a new nous with remapped IDs. Enables backup, migration, and cloning. ~300 LOC.

---

## New Gaps by Source

### From Letta

#### LT-1: Sleep-Time Compute (HIGH — evaluate)

**What:** Background agents reprocess conversation history during idle time, rewriting their memory blocks. Multi-agent group where participant agents get transcripts of recent main agent messages and run `.step()` async.

**Why it matters:** Prosoche monitors external signals (calendar, tasks, health) but doesn't reprocess conversations for deeper memory extraction. Sleep-time compute would let nous refine their understanding of past interactions, consolidate conflicting memories, and extract patterns that weren't obvious in real-time.

**Approach:** Extend the consolidation cron job. Currently it runs nightly for distillation. Add a second phase: for each nous, re-send the last N undistilled messages through Haiku for deeper memory extraction, fact revision, and pattern detection. Use existing `mneme` pipeline. ~200 LOC.

---

#### LT-2: Self-Editing Memory Blocks (MEDIUM)

**What:** Agents call `core_memory_replace` / `core_memory_append` to modify their own persistent memory (persona, facts, preferences). Memory is compiled into the system prompt each turn.

**Why it matters:** Aletheia's memory extraction is done by Haiku externally (Mem0 sidecar). The agent itself doesn't explicitly decide what to remember or forget. Self-editing gives agents agency over their own knowledge.

**Approach:** Add `memory_update` and `memory_forget` tools that let a nous directly modify its workspace memory files or Neo4j entities. Guard with confirmation for high-impact changes. ~150 LOC.

---

#### LT-3: Agent File Export/Import (MEDIUM)

**What:** Portable JSON format serializing agent state: identity, memory blocks, message history, tools, MCP configs. ID remapping on import.

**Why it matters:** No way to backup, clone, or migrate a nous today. Git-tracked workspaces capture files but not memory state (Qdrant vectors, Neo4j graph, session history).

**Approach:** `aletheia export <nous-id>` → JSON with identity, workspace files, Neo4j subgraph, Qdrant vectors, recent sessions. `aletheia import <file>` creates new nous with remapped IDs. ~300 LOC.

---

#### LT-4: Tool Rules System (LOW)

**What:** Declarative rules governing tool availability per agent step. `TerminalToolRule` (ends chain), `InitToolRule` (forces first call), `ContinueToolRule` (requests next step).

**Why it matters:** Aletheia's dynamic tool loading uses essential/available categories with 5-turn expiry. Tool rules would add finer control over tool sequencing (e.g., "always call `context_check` first").

**Approach:** Add optional `toolRules` to config schema. Evaluate in pipeline resolve stage. ~100 LOC.

---

### From CrewAI

#### CR-1: Deterministic Workflow Engine (MEDIUM — evaluate)

**What:** Flows — event-driven workflows with `@start()`, `@listen()`, `@router()` decorators. State persistence (SQLite). Conditional routing (`or_()`, `and_()`). Human feedback (pause/resume).

**Why it matters:** Aletheia's cron handles scheduled triggers. Event bus handles reactive events. But there's no way to define "if Chiron detects a health anomaly AND Syn confirms priority > 0.8, THEN wake user via Signal." Flows would add deterministic multi-step coordination.

**Approach:** Implement workflow definitions as JSON/YAML files in `shared/workflows/`. Engine reads definition, registers event bus listeners for triggers, manages state in SQLite. Start simple: trigger → condition → action. ~400 LOC.

---

#### CR-2: Composite Memory Scoring (MEDIUM)

**What:** Unified scoring: `semantic_weight (0.5) × similarity + recency_weight (0.3) × decay + importance_weight (0.2) × priority`. LLM-driven exploration rounds when confidence is low.

**Why it matters:** Mem0 returns results ranked by cosine similarity only. A memory from 6 months ago with 0.95 cosine beats a memory from yesterday with 0.90. Composite scoring balances relevance, freshness, and importance.

**Approach:** Add scoring layer to sidecar `/search` endpoint. Configurable weights. Recency from memory timestamps, importance from reinforcement count. ~80 LOC.

---

#### CR-3: A2A Protocol Support (MEDIUM)

**What:** Agent-to-Agent protocol (Google/Linux Foundation standard). JSON-RPC 2.0 over HTTP. Agent cards describe capabilities. Delegation with reasoning. Polling/push/streaming update mechanisms.

**Why it matters:** Aletheia's nous talk to each other via `sessions_send`/`sessions_ask` (internal). A2A would let them communicate with external agent systems — other Aletheia instances, CrewAI agents, AgentScope agents. Forward-looking interop standard.

**Approach:** Expose each nous as an A2A agent card via the gateway. Implement A2A client for outbound delegation. Map A2A messages to/from Aletheia's `InboundMessage`. ~400 LOC.

---

#### CR-4: Event Bus Dependency Ordering (LOW)

**What:** Event handlers declare dependencies on other handlers. Graph validation prevents circular deps. Ordered execution.

**Why it matters:** Aletheia's event bus fires handlers in registration order. If handler B depends on handler A's side effects, ordering is fragile.

**Approach:** Add optional `after: string[]` field to event listener registration. Topological sort before dispatch. ~50 LOC.

---

### From AgentScope

#### AS-1: Pub/Sub MsgHub Pattern (LOW)

**What:** Agents subscribe to hubs. When one agent replies, message auto-broadcasts to all subscribers. Dynamic add/remove participants.

**Why it matters:** Aletheia's cross-agent messaging is explicit (`sessions_send`). A pub/sub model would simplify group coordination — e.g., all nous automatically see Syn's announcements.

**Approach:** Add `broadcast` channel to blackboard or event bus. Agents subscribe by topic. ~100 LOC.

---

#### AS-2: Agent Hooks (pre/post reply/observe) (LOW)

**What:** 6 hook types on agent base class. Class-level (all instances) or instance-level (single agent). Hooks can modify args or output.

**Why it matters:** Aletheia's plugin hooks (`onBeforeTurn`/`onAfterTurn`) apply globally. Per-nous hooks would enable agent-specific behavior (e.g., Eiron always cites sources).

**Approach:** Add optional `hooks` to per-agent config. Evaluate in pipeline stages. ~100 LOC.

---

### From memU

#### MU-1: Tiered Retrieval with Sufficiency Gates (MEDIUM)

**What:** Query categories first. If sufficient context gathered (LLM check), stop. Otherwise fetch items. If still insufficient, fetch full resources. Early termination saves tokens.

**Why it matters:** Aletheia's `mem0-search` always returns N results regardless. For simple queries, category-level summaries might be enough. For complex queries, full resources are needed. Adaptive depth reduces cost.

**Approach:** Add sufficiency check to sidecar `/search_enhanced`. First pass: return category summaries. If LLM deems insufficient, second pass with full items. ~100 LOC.

---

#### MU-2: Tool Memory (LOW)

**What:** Tracks tool calls, success rates, "when_to_use" hints extracted from trajectories. Agent learns which tools work for which tasks.

**Why it matters:** Aletheia's skill learner extracts SKILL.md from trajectories. Tool Memory is complementary — tracking per-tool success/failure rates to inform future tool selection.

**Approach:** Add `tool_usage_stats` table (tool_name, success_count, failure_count, avg_duration, last_used). Update after each tool execution. Inject stats into `enable_tool` suggestions. ~80 LOC.

---

### From LangGraph

#### LG-1: Checkpoint-Based Time-Travel (LOW — evaluate)

**What:** Every agent step creates a checkpoint. User can browse history and resume from any past state. Fork conversations from historical points.

**Why it matters:** When a nous makes a wrong turn, the only recovery is manual. Time-travel would let you "rewind" a session to before the mistake and try again.

**Approach:** Aletheia already stores full session history in SQLite. Add `aletheia replay <session-id> --from <message-id>` to create a new session branching from a historical point. ~150 LOC.

---

#### LG-2: Reducer Pattern for Parallel Outputs (LOW)

**What:** When multiple agents write to the same state key concurrently, a reducer function aggregates results. `Annotated[list, operator.add]` concatenates, custom reducers for complex merging.

**Why it matters:** `sessions_dispatch` runs parallel sub-agents. Currently results are concatenated. Reducer pattern would allow structured merging (e.g., dedup, priority-based selection, voting).

**Approach:** Add optional `reducer` field to `sessions_dispatch` tool schema. Built-in reducers: `concat`, `vote`, `best_score`. ~80 LOC.

---

### From OwnPilot

#### OP-1: Encrypted Memory at Rest (HIGH)

**What:** AES-256-GCM encryption for all stored memories. PBKDF2 key derivation (600K iterations). Unique IV per entry. Master key in OS keychain. `secureClear()` wipes memory.

**Why it matters:** Mem0 vectors, Neo4j graph, and sqlite-vec all store plaintext. If the server is compromised, all memories are readable. Encryption at rest is a baseline security expectation.

**Approach:** Encrypt Qdrant payloads and Neo4j property values before storage. Decrypt on read. Master key stored in `/home/syn/.aletheia/keychain`. Key rotation support. ~200 LOC in sidecar + ~100 LOC in runtime.

---

#### OP-2: Tamper-Evident Audit Trail (HIGH)

**What:** Append-only JSONL. Each event includes SHA-256 hash of the previous event. Chain verification detects any modification.

**Why it matters:** Aletheia has an audit log table but no integrity verification. A compromised agent could alter its own audit trail. Hash chain makes tampering detectable.

**Approach:** Add `checksum` and `previous_checksum` columns to audit log. Compute SHA-256 of event JSON (excluding checksum fields) + previous checksum. `aletheia audit verify` walks the chain. ~100 LOC.

---

#### OP-3: PII Detection & Redaction (HIGH)

**What:** 15+ pattern categories (SSN, credit cards, emails, phone, IP, API keys). Confidence scoring with validators (Luhn for cards). Severity levels. Redaction modes: mask, label, remove.

**Why it matters:** Agents process personal conversations. Memories may contain sensitive data. Signal messages go through Aletheia unfiltered. PII detection protects against accidental leakage.

**Approach:** PII detector module in runtime (`koina/pii.ts`). Run on: (1) memories before Mem0 storage, (2) Signal outbound messages, (3) LLM context if containing user data. Configurable severity threshold. ~300 LOC.

---

#### OP-4: 4-Layer Execution Sandbox (MEDIUM)

**What:** Layer 1: Critical pattern blocking (100+ regex). Layer 2: Permission matrix per category. Layer 3: Real-time user approval. Layer 4: Docker/VM isolation with resource limits.

**Why it matters:** Complements OC-1 (Docker sandbox). OwnPilot's approach adds pre-execution pattern screening before even reaching Docker. The 4-layer model is defense-in-depth.

**Approach:** Layer 1 (pattern blocking) is ~50 LOC. Layer 2 (permission matrix) maps to existing approval gate. Layer 3 (approval callback) exists for webchat. Layer 4 (Docker) is OC-1. Implement Layer 1 first as the highest-value, lowest-effort addition.

---

## What Aletheia Has That No One Else Has

Updated to reflect all 8 comparisons.

### Persistent Named Agents with Domain Specialization
No other system has persistent agents with individual identities, workspaces, domain expertise, and memory that persists across sessions. CrewAI has role-based agents but they're ephemeral per-task. Letta has persistent state but single-purpose agents. Aletheia's 6 nous each have distinct domains (health, academic, work, creative, technical, general).

### Proactive Attention (Prosoche)
Letta's sleep-time compute is the closest analogue but is simpler (reprocess history during idle). Prosoche monitors external signals (calendar, tasks, health), scores attention needs on 60s intervals, predicts activity patterns, and generates dynamic workspace files. No other system approaches this.

### Self-Observation & Calibration
None of the 7 comparison systems has competence models, uncertainty tracking (Brier score, ECE), interaction signal classification, or mid-session eval feedback injection.

### Multi-Phase Distillation
All systems have some form of context management (compaction, summarization). Only Aletheia extracts structured facts/decisions/open items with similarity pruning.

### Skill Learning from Trajectories
CrewAI has training/replay. AgentScope has finetuning. But neither auto-extracts SKILL.md files from successful multi-tool-call trajectories at runtime.

### Cross-Agent Blackboard
SQLite-based shared state with TTL expiry. No other system has a lightweight cross-agent coordination primitive. CrewAI's crew-level memory is closest but is task-scoped, not persistent.

### Circuit Breakers
Input quality + response quality circuit breakers. No other system pre-screens inputs before LLM or post-screens responses.

### Reversibility Tagging
Tools tagged as reversible/irreversible for informed approval decisions. Unique.

---

## Canonical Prioritized Feature Set

### Tier 1: HIGH Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-1 | Spawn depth limits | ~15 | OpenClaw | None | Add `maxSpawnDepth` to config, check in `sessions_spawn` |
| F-2 | User-facing hooks | ~400 | Hybrid CC+OC+AS | None | `koina/hooks.ts`, JSON stdin/stdout, `noun:verb` events |
| F-3 | Hot-reload config | ~150 | Claude Code | None | `taxis/watcher.ts`, fs.watch, diff + hot-swap safe fields |
| F-4 | Docker sandbox for exec | ~300 | OpenClaw+OwnPilot | Docker | Sandbox runner + critical pattern pre-screen (OP Layer 1) |
| F-5 | Wire loop detection into guard | ~30 | OpenClaw | None | LoopDetector exists, wire into guard.ts |
| F-6 | Encrypted memory at rest | ~300 | OwnPilot | None | AES-256-GCM for Qdrant payloads + Neo4j properties |
| F-7 | Tamper-evident audit trail | ~100 | OwnPilot | None | SHA-256 hash chain on audit log events |
| F-8 | PII detection & redaction | ~300 | OwnPilot | None | `koina/pii.ts`, run on memories + Signal outbound + LLM context |

### Tier 2: MEDIUM Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-9 | Composite memory scoring | ~80 | CrewAI+memU | None | Semantic + recency + importance weights in sidecar `/search` |
| F-10 | MMR diversity re-ranking | ~120 | OpenClaw | None | Post-processing on Mem0 search results |
| F-11 | Sleep-time compute | ~200 | Letta | None | Extend consolidation cron: re-send undistilled messages through Haiku for deeper extraction |
| F-12 | Markdown command definitions | ~200 | Claude Code | None | Extend `CommandRegistry` to scan `shared/commands/*.md` |
| F-13 | Tool restrictions for ephemeral | ~100 | Hybrid CC+OC | None | Wire `EphemeralSpec.tools` through pipeline, glob patterns |
| F-14 | Tiered retrieval with sufficiency gates | ~100 | memU | None | Category summaries first, full items only if needed |
| F-15 | Agent File export/import | ~300 | Letta | None | `aletheia export/import <nous-id>`, JSON with memory + sessions |
| F-16 | A2A protocol support | ~400 | CrewAI+AgentScope | None | Expose nous as A2A agent cards, implement A2A client |
| F-17 | Deterministic workflow engine | ~400 | CrewAI | F-2 | YAML workflow definitions, event bus listeners, state persistence |
| F-18 | Prompt cache stability audit | ~50 | OpenClaw | None | Audit bootstrap-diff for cache invalidation |
| F-19 | Stream preview / typing indicators | ~100 | OpenClaw | None | Signal placeholder, webchat SSE already works |
| F-20 | ACP / IDE integration | ~300 | OpenClaw | F-2 | ACP adapter translating to gateway sessions |
| F-21 | Doctor with --fix | ~100 | OpenClaw | None | Fixable action tuples, execute with --fix flag |

### Tier 3: LOW Priority

| ID | Feature | LOC | Source | Deps | Approach |
|----|---------|-----|--------|------|----------|
| F-22 | Self-editing memory tools | ~150 | Letta | None | `memory_update`/`memory_forget` tools for nous |
| F-23 | Auth credential failover | ~100 | OpenClaw | None | Fallback credentials on 429/5xx |
| F-24 | Plugin standard layout | ~100 | Claude Code | None | Standard dirs, auto-discover, `ALETHEIA_PLUGIN_ROOT` |
| F-25 | Plugin path safety | ~30 | OpenClaw+OwnPilot | None | `realpath()` validation in loader |
| F-26 | Onboarding wizard | ~150 | OpenClaw | None | `aletheia init` with prompts |
| F-27 | Self-referential loop pattern | ~50 | Claude Code | F-2 | Stop hook + state file |
| F-28 | Announcement idempotency | ~30 | OpenClaw | None | Content hash dedup in cross_agent_calls |
| F-29 | Parallel validation pattern | ~50 | Claude Code | None | Skill, not infrastructure |
| F-30 | Temporal decay in search | ~80 | OpenClaw+memU | None | Exponential decay on search scores |
| F-31 | Tool Memory (usage stats) | ~80 | memU | None | Track success/failure rates per tool |
| F-32 | Tool rules system | ~100 | Letta | None | Declarative tool sequencing rules |
| F-33 | Event bus dependency ordering | ~50 | CrewAI | None | `after: string[]` on listeners, topological sort |
| F-34 | Checkpoint time-travel | ~150 | LangGraph | None | Branch sessions from historical points |
| F-35 | Reducer for parallel outputs | ~80 | LangGraph | None | Structured merging for `sessions_dispatch` results |
| F-36 | Pub/Sub hub pattern | ~100 | AgentScope | None | Auto-broadcast topics for cross-agent coordination |
| F-37 | Per-nous hooks | ~100 | AgentScope | F-2 | Agent-specific hook configs |

### Evaluation Items

| ID | Question | Source | What to determine |
|----|----------|--------|-------------------|
| E-1 | Docker sandbox scope | OpenClaw | Which tools? Performance cost? Docker on worker-node? |
| E-2 | Android app as webui port | OpenClaw | Kotlin webview vs PWA? |
| E-3 | Browser automation upgrade | OpenClaw | CDP + downloads + session persistence scope? |
| E-4 | Skill installation from npm/git | OpenClaw | Worth the attack surface? |
| E-5 | Workflow engine complexity | CrewAI | YAML vs code definitions? How many workflow types needed? |
| E-6 | A2A adoption timeline | CrewAI+AgentScope | Is the spec stable enough to implement? |
| E-7 | Memory encryption performance | OwnPilot | Overhead on search latency? Key management complexity? |

### Recommended Implementation Order

**Phase 1 — Safety & foundations (quick wins):**
1. F-1 (spawn depth) — 15 min
2. F-5 (loop detection wiring) — 15 min
3. F-7 (hash chain audit) — 1-2 hours
4. F-3 (hot-reload config) — half day

**Phase 2 — Security hardening:**
5. F-8 (PII detection) — 1-2 days
6. F-6 (encrypted memory) — 1-2 days
7. F-4 (Docker sandbox + pattern pre-screen) — 1-2 days

**Phase 3 — Memory intelligence:**
8. F-9 (composite scoring) — half day
9. F-10 (MMR diversity) — half day
10. F-14 (sufficiency gates) — half day
11. F-11 (sleep-time compute) — 1 day

**Phase 4 — Extensibility:**
12. F-2 (user-facing hooks) — 1-2 days
13. F-12 (markdown commands) — half day
14. F-13 (ephemeral tool restrictions) — half day

**Phase 5 — Interop & workflows:**
15. F-16 (A2A protocol) — 2-3 days
16. F-17 (workflow engine) — 2-3 days
17. F-15 (agent file export) — 1-2 days

**Total estimated:** ~4,795 LOC across Tiers 1-2. ~1,300 LOC for Tier 3.

---

## Cross-Reference with Existing Specs

| Feature | Overlapping Spec | Relationship |
|---------|-----------------|--------------|
| F-3 (hot-reload) | IMPROVEMENTS.md | Direct overlap |
| F-4 (Docker sandbox) | Spec 13 (Sub-Agent Workforce) | Enhances ephemeral safety |
| F-9, F-10, F-14 (memory scoring) | Spec 07 (Knowledge Graph) | Extends vector search quality |
| F-11 (sleep-time) | Spec 12 (Memory Evolution) | Background memory improvement |
| F-13 (tool restrictions) | Spec 13 (Sub-Agent Workforce) | Mechanism for role-specific tools |
| F-16 (A2A) | None | New capability |
| F-17 (workflows) | Spec 14 (Development Workflow) | Generalizes workflow patterns |
| F-18 (cache stability) | Spec 16 (Efficiency) | Token economy optimization |
| F-19 (stream preview) | Spec 15 (UI Quality) | Webchat UX |
| F-20 (ACP) | None | New capability |

---

## Verification

- Cross-reference against source in: `openclaw-ref/`, `claude-code/`, `letta-ref/`, `crewai-ref/`, `agentscope-ref/`, `memu-ref/`, `langgraph-ref/`, `ownpilot-ref/`
- Cross-reference Aletheia capabilities against `infrastructure/runtime/src/`
- Validate no feature violates user directives
- Validate against IMPROVEMENTS.md and existing specs for overlap

### Key Implementation Files

| File | Relevant Features |
|------|-------------------|
| `src/nous/pipeline/stages/guard.ts` | F-1, F-5 |
| `src/koina/event-bus.ts` | F-2, F-33 |
| `src/koina/pii.ts` (new) | F-8 |
| `src/semeion/commands.ts` | F-12 |
| `src/taxis/schema.ts` | F-1, F-2, F-3, F-4, F-17 |
| `src/taxis/loader.ts` | F-3 |
| `src/taxis/watcher.ts` (new) | F-3 |
| `src/koina/hooks.ts` (new) | F-2 |
| `src/organon/registry.ts` | F-13, F-32 |
| `src/organon/built-in/sessions-spawn.ts` | F-1, F-13 |
| `src/organon/built-in/browser.ts` | E-3 |
| `src/hermeneus/anthropic.ts` | F-23 |
| `src/prostheke/loader.ts` | F-24, F-25 |
| `src/nous/bootstrap.ts` | F-18 |
| `src/pylon/server.ts` | F-16 (A2A endpoints), F-20 (ACP) |
| `entry.ts` | F-15, F-21, F-26 |
| `infrastructure/memory/sidecar/` | F-6, F-9, F-10, F-14, F-30 |

### Reference Implementations

| Feature | Reference Source | File |
|---------|-----------------|------|
| F-6 (encryption) | OwnPilot | `ownpilot-ref/packages/core/src/crypto/vault.ts` |
| F-7 (hash chain) | OwnPilot | `ownpilot-ref/packages/core/src/audit/logger.ts` |
| F-8 (PII) | OwnPilot | `ownpilot-ref/packages/core/src/privacy/detector.ts` |
| F-10 (MMR) | OpenClaw | `openclaw-ref/src/memory/mmr.ts` |
| F-11 (sleep-time) | Letta | `letta-ref/letta/groups/sleeptime_multi_agent.py` |
| F-15 (agent file) | Letta | `letta-ref/letta/schemas/agent_file.py` |
| F-16 (A2A) | AgentScope | `agentscope-ref/src/agentscope/a2a/` |
| F-17 (flows) | CrewAI | `crewai-ref/lib/crewai/src/crewai/flow/flow.py` |
| F-9 (composite) | CrewAI | `crewai-ref/lib/crewai/src/crewai/memory/unified_memory.py` |
| F-14 (sufficiency) | memU | `memu-ref/src/memu/app/retrieve.py` |
