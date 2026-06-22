# HUBS.md - Architectural Hub Index

Navigation index for concepts that touch many components.

---

## session

**Definition:** A conversation context bound to an agent, persisted across turns, with a lifecycle (Active, Archived, Distilled).

**Components:**
- `graphe::store::SessionStore` - fjall-backed persistence (single-writer tx)
- `nous::session::SessionState` - in-memory turn count and token estimate per actor
- `pylon::handlers::sessions` - HTTP CRUD and SSE streaming endpoints
- `theatron::skene::api::client` - UI client for session history and streaming
- `organon::builtins::agent` - `sessions_spawn` tool for programmatic session creation

**Contracts:**
- `SessionId` is a UUID newtype shared across all crates (`koina::id`)
- `SessionType` enum (Primary, Background, Ephemeral) is authoritative in `graphe::types` and consumed by `nous`
- Only `graphe::store` writes session data; all other crates read through `mneme` facade

**Known mismatches:** none

---

## memory

**Definition:** The persistent knowledge layer: facts, embeddings, and recall over agent experience.

**Components:**
- `mneme::knowledge_store::KnowledgeStore` - facade over `episteme` (optional `krites` + fjall backend)
- `eidos::knowledge` - canonical `Fact`, `Entity`, `Relationship`, `EpistemicTier` types
- `episteme::recall::RecallEngine` - 6-factor scoring for knowledge retrieval
- `organon::builtins::memory` - `memory_search`, `note`, `blackboard` tools
- `nous::working_memory` - per-turn working checkpoint injection into `<key_info>`
- `melete::flush::MemoryFlush` - persists critical context before distillation boundaries

**Contracts:**
- Facts carry bi-temporal timestamps (`valid_from`/`valid_to` and `recorded_at`) from `eidos`
- Facts carry `Visibility` and optional `MemoryScope`; recall and MCP/API surfaces must preserve those filters, scope reads to the requesting `nous_id`, and require explicit per-request ownership for writes (no fallback service authorship)
- Recall weights are configurable per-agent via `taxis` config and consumed by `nous`
- `memory_search` routes through `organon` → `mneme` → `episteme` with the same scoring
- Memory docs and comments use Krites/Datalog/Fjall for current architecture; CozoDB/SQLite/redb references are acceptable only in explicitly historical migration notes.

**Known mismatches:** none

---

## tool

**Definition:** Executable capability exposed to agents, dispatched from LLM tool-use blocks.

**Components:**
- `organon::registry::ToolRegistry` - name-based dispatch with `ToolDef` metadata
- `hermeneus::types` - `ContentBlock::ToolUse` and `StopReason::ToolUse` from LLM responses
- `nous::execute::dispatch` - sequential tool execution with loop detection and event logging
- `pylon::openapi` - exposes tool schemas via utoipa-derived OpenAPI spec
- `organon::receipts` - HMAC-SHA256 receipt signer and in-memory ledger

**Contracts:**
- Tools register with `name`, `description`, `schema`, typed tags, tool groups, and `auto_activate` flag (`organon::types`)
- Tool dispatch caps at `maxToolIterations` configured per agent (`nous::execute`)
- Tool results must be backed by receipts when the active pipeline is verifying tool-use integrity
- Tool results are encoded as `ContentBlock::ToolResult` and appended to turn history

**Known mismatches:** none

---

## provider

**Definition:** Pluggable backend abstraction for LLM inference and external channel messaging.

**Components:**
- `hermeneus::provider::LlmProvider` - trait for `complete()` / `complete_streaming()`
- `taxis::config::behavior::provider::LlmProviderConfig` - model list, deployment target, cache mode
- `agora::types::ChannelProvider` - trait for Signal and future channel integrations

**Contracts:**
- `LlmProvider` is object-safe via boxed futures; `ProviderRegistry` tracks per-model health
- `LlmProviderConfig` includes `deployment_target` (restricts accepted data classifications)
- `ChannelProvider` is object-safe via `Pin<Box<dyn Future>>` and stored as `Arc<dyn ChannelProvider>`

**Known mismatches:** `hermeneus` and `agora` both use "Provider" but are unrelated abstractions (LLM inference vs external messaging).

---

## fact

**Definition:** The canonical unit of knowledge: a structured assertion with provenance, lifecycle, and confidence.

**Components:**
- `eidos::knowledge::Fact` - type definition with `FactType`, `EpistemicTier`, `KnowledgeStage`
- `mneme::knowledge_store::KnowledgeStore` - persistence facade (re-export from `episteme`)
- `episteme::recall::RecallEngine` - retrieval with 6-factor weighted scoring
- `nous::pipeline::stages::run_recall_stage` - injects recalled facts into turn context

**Contracts:**
- `Fact` uses bi-temporal model: domain validity (`valid_from`/`valid_to`) separate from system time (`recorded_at`)
- `EpistemicTier` (Verified → Established → Inferred → Speculative) affects recall scoring multiplier
- Conflict classification (Contradiction, Supersession, Elaboration, Independent) is authoritative in `episteme`

**Known mismatches:** none

---

## turn

**Definition:** A single pass through the agent pipeline: guard → bootstrap → skills → recall → history → execute → finalize.

**Components:**
- `hermeneus::types::CompletionRequest` - LLM request assembled for the execute stage
- `nous::pipeline` - sequential stage runner with token budgeting and timeout guards
- `pylon::handlers::sessions::streaming` - SSE stream of turn events to clients

**Contracts:**
- Turns are processed sequentially per session by `NousActor` (tokio select! inbox pattern)
- Pipeline stages run in fixed order; each stage consumes token budget from previous stages
- pylon spawns each turn as a tokio task and converts `StreamEvent` to SSE lines with keep-alive

**Known mismatches:** none

---

## agent

**Definition:** A configured persona running as an isolated actor with its own tools, memory, and pipeline.

**Components:**
- `taxis::config::NousDefinition` - per-agent config: model, agency, tools, domains
- `nous::actor::NousActor` / `nous::manager::NousManager` - runtime actor and lifecycle manager
- `pylon::handlers::nous` - HTTP endpoints for listing and querying agents
- `diaporeia::tools` - MCP tools: `nous_list`, `nous_status`, `nous_tools`
- `agora::router::MessageRouter` - resolves inbound channel messages to target agents

**Contracts:**
- Agent config cascades through three tiers: `nous/{id}/` → `shared/` → `theke/` (`taxis::cascade`)
- Each agent has its own tokio actor with sequential turn processing and panic boundary
- Routing priority: exact group binding > exact source binding > channel wildcard > global default

**Known mismatches:** none
