# Codebase Validation: Skills System & CC Execution Layer

Research document validating architectural assumptions from the skills system and CC execution layer planning docs against the actual codebase.

---

## 1. Validation Summary

| # | Assumption | Status | Notes |
|---|-----------|--------|-------|
| 1a | PackManifest extends cleanly to skill metadata | Partially confirmed | YAML schema is flexible, but no structured metadata fields exist — skills would need content-only or manifest extension |
| 1b | load_packs() can be adapted for mneme backends | Partially confirmed | Tightly coupled to filesystem; needs trait extraction for multi-backend |
| 1c | ShellToolExecutor is reusable for skill scripts | Confirmed | Fully generic subprocess executor; works for any packaged script |
| 1d | Thesauros can abstract to multiple backends | Partially confirmed | Data types are clean; I/O layer needs refactoring |
| 2a | assemble_with_extra() is the skill injection point | Confirmed | Already accepts Vec<BootstrapSection>, priority-sorted with workspace files |
| 2b | Enough token budget for skills after workspace+packs | Confirmed (with constraints) | 40K budget, ~15K consumed by 9 workspace files = ~25K available |
| 2c | Truncation respects priority | Confirmed | Stable sort by SectionPriority; truncatable flag respected |
| 2d | Selective loading can happen during bootstrap | Needs research | Requires mneme query latency benchmarking |
| 3a | Extraction runs post-session as parallel pass | Partially confirmed | Runs per-turn (not post-session), async background task |
| 3b | ExtractionProvider can support skill extraction | Confirmed | Trait is simple (complete(system, user) → String); parallel implementations trivial |
| 3c | Extraction is async/non-blocking | Confirmed | tokio::spawn, fire-and-forget, never blocks response |
| 4a | SpawnService trait supports CC bridge replacement | Confirmed | Clean trait boundary; single method, Result-based |
| 4b | Spawn caller expects async result with timeout | Confirmed | Future-based, tokio::time::timeout wrapped |
| 4c | CC bridge can be a drop-in replacement | Confirmed | Same trait, different impl; ToolServices.spawn is Arc<dyn SpawnService> |
| 5a | Tool registry has observation points | Confirmed | execute() captures name, duration, status via tracing spans + Prometheus |
| 5b | Existing metrics support sequence reconstruction | Partially confirmed | Per-call metrics exist, but no sequence tracking; TurnResult.tool_calls has ordered sequence |
| 5c | Per-call data sufficient for pattern capture | Confirmed | ToolCall has id, name, input (JSON), result, is_error, duration_ms |
| 6a | Facts can store skill metadata | Partially confirmed | content: String can hold JSON/YAML; no structured fields beyond string |
| 6b | SKILLED_IN relationship type exists | Confirmed | In controlled vocabulary (vocab.rs:42) with alias mapping |
| 6c | Vector search works for semantic skill matching | Confirmed | HNSW index on embeddings relation; cosine distance; kNN + hybrid search |
| 6d | Skill-specific queries feasible | Confirmed | fact_type filtering supported; CozoDB Datalog is flexible |
| 7a | Thesauros failure doesn't block bootstrap | Confirmed | load_packs() skips failures, returns partial results |
| 7b | Spawn service unavailability doesn't block sessions | Confirmed | Tool returns error message; Optional<Arc<dyn SpawnService>> pattern |
| 7c | Mneme down → skill loading fails open | Confirmed | Knowledge store is feature-gated; tools return "not configured" |
| 7d | No hidden hard dependencies | Confirmed | Only SessionStore and EmbeddingProvider are hard deps |

---

## 2. Detailed Analysis

### 2.1 Thesauros Extensibility

#### PackManifest structure (`crates/thesauros/src/manifest.rs:14-33`)

PackManifest has 6 fields: `name`, `version`, `description`, `context`, `tools`, `overlays`. All use `#[serde(default)]` for optional fields. The YAML schema is additive — new fields with `#[serde(default)]` won't break existing pack.yaml files.

**Skill metadata extension options:**

Option A — Add fields to PackManifest:
```rust
// New optional fields, backwards-compatible via serde(default)
pub usage_count: u32,
pub origin_agent: Option<String>,
pub confidence: f64,
pub domain_tags: Vec<String>,
pub skill_steps: Vec<SkillStep>,   // structured skill definition
```
Pro: Simple, single type. Con: Conflates packs and skills in one schema.

Option B — Parallel SkillManifest type:
```rust
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub steps: Vec<SkillStep>,
    pub tools_used: Vec<String>,
    pub domain_tags: Vec<String>,
    pub confidence: f64,
    pub origin_agent: Option<String>,
}
```
Pro: Clean separation. Con: Doesn't reuse thesauros loading infrastructure.

**Assessment:** Option A is simpler but muddies the abstraction. Option B is cleaner. The planning doc's Option C (skills as mneme facts + thesauros loader) is the best fit — keeps PackManifest pure for domain packs, stores skill data in mneme, and uses a new SkillLoader that queries mneme and produces `Vec<BootstrapSection>` directly.

#### load_packs() filesystem coupling (`crates/thesauros/src/loader.rs:80-175`)

The loading path is filesystem-dependent at three points:
1. `pack_root.is_dir()` check (`manifest.rs:131`)
2. `std::fs::read_to_string()` for manifest (`manifest.rs:145`)
3. `std::fs::read_to_string()` for context files (`loader.rs:156`)

To support mneme-backed loading, extract a trait:
```rust
pub trait PackSource: Send + Sync {
    fn load_manifest(&self, pack_id: &str) -> Result<PackManifest>;
    fn read_context(&self, pack_id: &str, path: &str) -> Result<String>;
}
```

The existing disk loader becomes `FsPackSource`. A mneme-backed loader becomes `MnemePackSource`. The `PackSection` and `BootstrapSection` conversion stays identical.

**Effort estimate:** Moderate — trait extraction, two implementations, update callsites in main.rs and manager.rs.

#### ShellToolExecutor reusability (`crates/thesauros/src/tools.rs:25-107`)

Fully reusable. The executor takes `command_path`, `pack_root`, and `timeout_ms` — all injectable. Skill-bundled scripts would work identically to pack-bundled scripts. The `register_pack_tools()` function (`tools.rs:113-154`) is generic enough to register tools from any source.

#### Path sandboxing (`tools.rs:198-226`)

`validate_command_path()` canonicalizes the command and checks it doesn't escape the pack root. This security measure works for any directory-based isolation, including skill directories.

---

### 2.2 Bootstrap Injection Capacity

#### Token budget (`crates/nous/src/budget.rs:50-67`, `config.rs:39-41`)

Default configuration:
- context_window: 200,000
- history_ratio: 0.6 → 120,000 reserved for conversation
- turn_reserve: 16,384 (max_output_tokens)
- bootstrap_cap: 40,000

**System budget = min(200,000 - 16,384 - 120,000, 40,000) = 40,000 tokens**

Token estimation uses `ceil(len/4)` (conservative 4 chars/token).

#### Workspace file consumption

9 workspace files with priorities (`bootstrap/mod.rs:84-131`):

| File | Priority | Truncatable | Typical Size |
|------|----------|-------------|-------------|
| SOUL.md | Required | No | 2-4K tokens |
| USER.md | Important | No | 1-2K tokens |
| AGENTS.md | Important | No | 1-2K tokens |
| GOALS.md | Important | Yes | 1-3K tokens |
| TOOLS.md | Important | Yes | 2-3K tokens |
| MEMORY.md | Flexible | Yes | 2-5K tokens |
| IDENTITY.md | Flexible | No | 0.5-1K tokens |
| PROSOCHE.md | Flexible | No | 0.5-1K tokens |
| CONTEXT.md | Flexible | Yes | 1-2K tokens |

**Estimated workspace consumption: 12-25K tokens** (varies by agent).

**Remaining for packs + skills: 15-28K tokens.** With typical domain packs consuming 3-8K, **7-25K tokens remain for skills**.

#### How skills would fit

At 500-1000 tokens per skill (typical SKILL.md), **7-25 injected skills** fit within budget. The planning doc's selective loading (3-5 relevant skills per session) is well within this range.

#### Truncation logic (`bootstrap/mod.rs:320-380`)

1. Sections sorted by priority (stable sort preserves declaration order within tier)
2. Required sections always included
3. Important sections included if budget allows
4. Flexible/Optional sections truncated or dropped under pressure
5. Truncation is markdown-aware: splits on `## ` headers, then falls back to line-by-line
6. Truncation marker: `... [truncated for token budget] ...`

Skills injected as Flexible or Optional priority would be truncated/dropped before workspace files. This is correct behavior — workspace identity should be preserved over skill suggestions.

#### Selective loading latency concern

`assemble_with_extra()` (`bootstrap/mod.rs:186-253`) accepts pre-resolved `Vec<BootstrapSection>`. If skill selection requires a mneme query (embedding search + scoring), this adds latency to session start.

**Latency chain:** embed task description → HNSW kNN search → score results → convert to BootstrapSection

The embedding step dominates (fastembed-rs dimension 384). With a warm model, embed + search should be <100ms. **Needs benchmarking** — if latency exceeds 500ms, pre-resolve skills asynchronously or cache recent results.

---

### 2.3 Extraction Pipeline Capacity

#### Trigger mechanism (`crates/nous/src/actor.rs:396-432`)

Extraction triggers **per-turn** (not post-session), as a background task after pipeline finalize. Guard conditions:
1. ExtractionConfig must be present and enabled
2. Combined message length must exceed min_message_length (default 50 chars)

**Correction to planning doc:** The skills planning doc says "extraction pipeline triggers post-session." It actually triggers per-turn, with each turn's extraction running independently. This is better for skill capture — patterns can be detected incrementally rather than requiring session completion.

#### ExtractionProvider trait (`crates/mneme/src/extract.rs:128-130`)

```rust
pub trait ExtractionProvider: Send + Sync {
    fn complete(&self, system: &str, user_message: &str) -> Result<String, ExtractionError>;
}
```

Minimal interface. Skill extraction would implement the same trait with a different system prompt and response schema. The existing `HermeneusExtractionProvider` (`crates/nous/src/extraction.rs:1-67`) wraps the provider registry and could be reused with different prompts.

#### Parallel extraction feasibility

Current extraction spawns one background task per turn (`tokio::spawn`, fire-and-forget, `actor.rs:418-431`). Adding a parallel skill extraction pass is straightforward:

```rust
// In maybe_spawn_extraction():
self.spawn_fact_extraction(&content, &turn_result);    // existing
self.spawn_skill_extraction(&turn_result.tool_calls);  // new, parallel
```

Both tasks are independent — fact extraction analyzes conversation content, skill extraction analyzes tool call sequences. No shared mutable state.

#### Latency budget

Extraction uses Haiku by default (`extract.rs:105`). A typical extraction call: 2-5K input tokens, ~500 output tokens. At Haiku speeds, ~1-3 seconds. Since it's background (fire-and-forget), latency doesn't affect user experience.

**Integration point for skill extraction:** Add a parallel `tokio::spawn` in `maybe_spawn_extraction()` that receives `turn_result.tool_calls` and runs heuristic filtering followed by optional LLM extraction. No changes to the existing extraction path.

---

### 2.4 Spawn Service → CC Bridge Feasibility

#### SpawnService trait (`crates/organon/src/types.rs:1206-1234`)

```rust
pub trait SpawnService: Send + Sync {
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>;
}
```

Clean trait boundary. `SpawnRequest` has 5 fields: role, task, model (optional), allowed_tools (optional), timeout_secs. `SpawnResult` has 4 fields: content, is_error, input_tokens, output_tokens.

#### Current implementation (`crates/nous/src/spawn_svc.rs:23-150`)

`SpawnServiceImpl` creates a minimal NousActor with:
- 4K bootstrap budget (vs 40K for core agents)
- Single SOUL.md: "You are an ephemeral {role} sub-agent"
- No USER.md, GOALS.md, MEMORY.md
- Model selection: Haiku for explorer/runner, Sonnet for coder/reviewer/researcher
- Single-turn execution with timeout
- Fire-and-forget cleanup after completion

#### CC bridge as drop-in replacement

A `CcBridgeSpawnService` implementing the same trait would:

```rust
impl SpawnService for CcBridgeSpawnService {
    fn spawn_and_run(&self, request: SpawnRequest, parent_nous_id: &str)
        -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>
    {
        Box::pin(async move {
            // 1. Select CC agent profile from request.role
            // 2. Prepare worktree if needed
            // 3. Inject relevant skills from mneme
            // 4. Execute: claude -p "task" --output-format json --allowedTools [...]
            // 5. Parse JSON result { result, session_id, usage }
            // 6. Map to SpawnResult { content, is_error, input_tokens, output_tokens }
        })
    }
}
```

**No changes needed to callers.** The `sessions_spawn` and `sessions_dispatch` tool executors (`organon/builtins/agent.rs:23-170`) use `Arc<dyn SpawnService>` from ToolServices — any implementation works.

#### What the caller expects

- **Async result:** Future-based, awaited in tool executor
- **Timeout:** `SpawnRequest.timeout_secs` — bridge must enforce this (CC supports `maxTurns` and process-level timeout)
- **Token accounting:** `SpawnResult.input_tokens/output_tokens` — CC's `--output-format json` returns usage metadata
- **Error handling:** String-based errors, mapped from CC exit codes
- **No streaming:** Single response text, not incremental

#### sessions_dispatch parallel execution (`organon/builtins/agent.rs:71-170`)

Dispatches up to 10 concurrent spawn requests via `futures::future::join_all`. A CC bridge would naturally support this — each CC instance is a separate process.

---

### 2.5 Tool Observation for Pattern Capture

#### Observation points

Three levels of tool observation exist:

**Level 1: Tracing spans** (`crates/organon/src/registry.rs:83-102`)
- `info_span!("tool_execute")` with fields: `tool.name`, `tool.duration_ms`, `tool.status`
- Recorded on every tool execution via `execute()` method

**Level 2: Prometheus metrics** (`crates/organon/src/metrics.rs:9-45`)
- `aletheia_tool_invocations_total{tool_name, status}` — counter
- `aletheia_tool_duration_seconds{tool_name}` — histogram with buckets [0.01...120.0]
- Aggregated, not per-session — useful for trends, not sequence reconstruction

**Level 3: Pipeline aggregation** (`crates/nous/src/pipeline.rs:199-229`, `execute.rs:91-177`)
- `TurnResult.tool_calls: Vec<ToolCall>` — ordered list of all calls in a turn
- Each `ToolCall`: id, name, input (full JSON), result (summary), is_error, duration_ms
- `TurnResult.signals: Vec<InteractionSignal>` — classified signal types
- `TurnResult.usage: TurnUsage` — token accounting

#### Sequence reconstruction from existing data

`TurnResult.tool_calls` provides a complete ordered sequence per turn with:
- Tool name (for pattern matching)
- Full input arguments (for parameterization)
- Result summary (for success/failure assessment)
- Duration (for latency analysis)
- Error status (for quality scoring)

**This is sufficient for skill auto-capture.** The pattern detection pipeline from the research doc can consume `Vec<ToolCall>` directly — no new instrumentation needed.

#### Cross-turn sequence tracking

Current design: tool sequences are per-turn within `TurnResult`. For multi-turn patterns, the session history (stored in SessionStore) provides turn-by-turn reconstruction. The extraction background task already receives turn content — it could also receive tool call data.

#### Integration point

The cleanest hook is in `actor.rs:maybe_spawn_extraction()` — it already has access to `TurnResult` (which contains `tool_calls`). Adding skill pattern analysis alongside fact extraction:

```rust
fn maybe_spawn_skill_analysis(&self, turn_result: &TurnResult) {
    if turn_result.tool_calls.len() >= 3 {  // minimum complexity
        let tool_calls = turn_result.tool_calls.clone();
        tokio::spawn(async move {
            analyze_tool_pattern(tool_calls).await;
        }.instrument(span));
    }
}
```

#### What data is available per tool call

| Field | Type | Source | Available |
|-------|------|--------|-----------|
| Tool name | String | ToolInput.name | Yes |
| Arguments | serde_json::Value | ToolInput.arguments | Yes (full JSON) |
| Result text | String | ToolResult.content (summarized) | Yes |
| Success/failure | bool | ToolResult.is_error | Yes |
| Duration | u64 (ms) | Instant::now() in dispatch_tools | Yes |
| Tool use ID | String | LLM tool_use block ID | Yes |
| Sequence order | implicit | Vec index in tool_calls | Yes |

---

### 2.6 Mneme Skill Storage Feasibility

#### Fact type for skills (`crates/mneme/src/knowledge.rs:15-50`, `178-189`)

The `Fact` type already recognizes `"skill"` as a fact_type with 2190-hour stability (91 days). Key fields:

- `content: String` — can hold JSON/YAML serialized skill definition
- `confidence: f64` — maps to skill confidence scoring
- `fact_type: "skill"` — distinguishes from other facts
- `stability_hours: 2190.0` — 91-day default decay
- `access_count: u32` — tracks usage frequency
- `last_accessed_at` — recency signal
- `nous_id` — agent attribution
- `source_session_id` — extraction provenance
- `is_forgotten` — soft deletion for deprecated skills

#### Structured metadata limitation

`content: String` is the only data field. Skills need structured metadata (steps, tools_used, domain_tags, origin_agent, etc.). Two approaches:

**Option A: JSON in content field**
```json
{
  "name": "diagnose-lint-errors",
  "description": "...",
  "steps": ["Run clippy", "Read source", "Fix", "Verify"],
  "tools_used": ["exec", "read", "edit"],
  "domain_tags": ["rust", "linting"],
  "skill_md": "## Steps\n\n1. Run `cargo clippy`..."
}
```
Pro: No schema changes. Con: Queries on nested fields require JSON extraction in Datalog (cumbersome).

**Option B: Dedicated CozoDB relation**
```
skills { id: String =>
    name: String, description: String,
    steps: String, tools_used: String,
    domain_tags: String, origin_agent: String,
    confidence: Float, usage_count: Int,
    created_at: String, updated_at: String }
```
Pro: Direct field queries. Con: Parallel schema to facts relation.

**Recommendation:** Option A for v1 (minimal changes, content field holds JSON), Option B when skill volume justifies dedicated queries.

#### SKILLED_IN relationship (`crates/mneme/src/vocab.rs:42, 118`)

Already in controlled vocabulary. `normalize_relation("skilled_in")` returns `Valid("SKILLED_IN")`. Agent-skill associations can be modeled as:
```
agent_entity -[SKILLED_IN]-> skill_entity
```

This enables graph queries: "Which agents are skilled in Rust linting?" → traverse SKILLED_IN edges from skill entity.

#### Vector search for semantic matching (`crates/mneme/src/knowledge_store.rs:104-114, 418-448`)

HNSW index exists on embeddings relation:
- Dimension: configurable (default 384 via fastembed-rs)
- Distance: Cosine
- Parameters: m=16, ef_construction=200

Skill descriptions embedded via the existing `EmbeddingProvider` pipeline feed directly into semantic search. Query flow:

1. Embed task description → query vector
2. `search_vectors(query_vec, k=10, ef=100)` → nearest skill embeddings
3. Filter by nous_id/domain
4. Return ranked skills for bootstrap injection

The `search_hybrid()` method (`knowledge_store.rs:536-566`) fuses BM25 + HNSW + graph signals via ReciprocalRankFusion — this could combine keyword matching ("rust clippy") with semantic similarity and graph proximity for skill selection.

#### Skill-specific queries via fact_type

CozoDB Datalog supports direct filtering:
```datalog
?[id, content, confidence] :=
    *facts{id, content, confidence, fact_type, nous_id, is_forgotten},
    fact_type = "skill",
    nous_id = $nous_id,
    is_forgotten = false
```

For structured queries on JSON content (e.g., domain tag filtering), Option A requires JSON extraction functions. CozoDB supports `json_get()` operations, but they're slower than direct column queries. This is acceptable for v1 volumes (dozens of skills, not thousands).

---

### 2.7 Graceful Degradation

#### Thesauros failure → bootstrap continues

`load_packs()` (`crates/thesauros/src/loader.rs:84-114`) logs warnings and skips failed packs. Returns `Vec::new()` if all fail. Bootstrap proceeds with workspace files only. Tool registration failures are similarly non-fatal (`main.rs:547-549`).

#### Spawn service unavailable → sessions work

`ToolServices.spawn` is `Option<Arc<dyn SpawnService>>` (`organon/types.rs:298-330`). Tool executors check for None and return error messages (`organon/builtins/agent.rs:30-35`). Core sessions are unaffected — spawn tools simply report unavailability.

#### Mneme down → fail open

Knowledge store is feature-gated (`#[cfg(feature = "recall")]`). Without the feature, `knowledge_store = None` (`main.rs:628-631`). Memory tools return "not configured" errors (`organon/builtins/memory.rs:49-50`). Extraction background tasks check for knowledge store and skip persistence if absent.

**For skill loading specifically:** If mneme is unavailable during bootstrap, skill resolution returns empty. `assemble_with_extra()` receives `Vec::new()` as extra sections — same as having no skills. Session proceeds with workspace files and domain packs only.

#### Hard dependencies (2 only)

| Dependency | Location | Impact |
|-----------|----------|--------|
| SessionStore (SQLite) | `main.rs:532-535` | Bootstrap fails — cannot persist sessions |
| EmbeddingProvider | `main.rs:561-562` | Bootstrap fails — cannot embed for recall |

These are correct hard dependencies — sessions require persistence and recall requires embeddings. The CC execution layer and skills system introduce no new hard dependencies.

---

## 3. Integration Point Map

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Session Lifecycle                               │
│                                                                      │
│  Session Start                                                       │
│  ┌──────────────────────────────────┐                                │
│  │ BootstrapAssembler               │                                │
│  │  ├─ resolve_workspace_files()    │ ← SOUL.md, USER.md, etc.      │
│  │  ├─ pack_sections_to_bootstrap() │ ← thesauros domain packs      │
│  │  ├─ [NEW] resolve_skills()       │ ← mneme query → BootstrapSec  │
│  │  └─ assemble_with_extra()        │ ← priority sort + budget pack │
│  └──────────────────────────────────┘                                │
│                                                                      │
│  Turn Execution                                                      │
│  ┌──────────────────────────────────┐                                │
│  │ execute() loop                   │                                │
│  │  ├─ LLM call                     │                                │
│  │  ├─ dispatch_tools()             │                                │
│  │  │   └─ registry.execute()       │ ← tracing + metrics + timing  │
│  │  │       ├─ [spawn] ──────────── │ ─→ SpawnService.spawn_and_run  │
│  │  │       │                       │    └─ [NEW] CcBridgeSpawnSvc   │
│  │  │       └─ [other tools]        │                                │
│  │  └─ TurnResult{tool_calls, ...}  │ ← ordered sequence captured   │
│  └──────────────────────────────────┘                                │
│                                                                      │
│  Post-Turn (background)                                              │
│  ┌──────────────────────────────────┐                                │
│  │ maybe_spawn_extraction()         │                                │
│  │  ├─ fact extraction (existing)   │ ← entities, relationships      │
│  │  └─ [NEW] skill extraction       │ ← tool_calls → pattern detect │
│  │      ├─ heuristic filter         │                                │
│  │      ├─ candidate tracker        │ ← KnowledgeStore fact         │
│  │      └─ LLM extraction (≥3 hits) │ ← SKILL.md generation         │
│  └──────────────────────────────────┘                                │
│                                                                      │
│  CC Dispatch (new capability)                                        │
│  ┌──────────────────────────────────┐                                │
│  │ CcBridgeSpawnService             │                                │
│  │  ├─ map role → .claude/agents/   │                                │
│  │  ├─ query mneme for skills       │                                │
│  │  ├─ export .claude/skills/       │                                │
│  │  ├─ claude -p --output-format    │ ← headless CC execution       │
│  │  └─ parse result → SpawnResult   │                                │
│  └──────────────────────────────────┘                                │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 4. Identified Conflicts

### 4.1 Extraction timing mismatch

**Planning assumption:** Skill extraction runs "post-session" as a parallel pass alongside fact extraction.

**Reality:** Extraction runs **per-turn** (`actor.rs:217`), triggered after each successful pipeline completion. This is actually better for incremental pattern detection but means the extraction context is a single turn, not a full session.

**Resolution:** Skill pattern detection needs a two-phase approach:
1. Per-turn: Record tool call signature to candidate tracker (fast, heuristic)
2. Cross-session: Promote candidates with recurrence ≥ 3 (async job, not per-turn)

### 4.2 Content field limitation for structured skills

**Planning assumption:** Skills stored as mneme facts with structured metadata.

**Reality:** `Fact.content` is a single String. Structured queries on nested JSON fields are possible but slower than direct column access in CozoDB.

**Resolution:** For v1, JSON-in-content is sufficient (dozens of skills). Add a dedicated `skills` relation in mneme when skill volume grows.

### 4.3 Pack model vs skill model conceptual tension

**Planning assumption:** Thesauros can be "extended" to handle skills natively (Option A).

**Reality:** Thesauros is designed for **static domain packs** — directories of files loaded once at startup. Skills are **dynamic, agent-attributed, confidence-scored knowledge** that changes over time. Extending PackManifest with skill fields conflates two different lifecycle models.

**Resolution:** Option C from the planning doc is correct — skills as mneme facts, loaded via a new SkillLoader that produces `Vec<BootstrapSection>`. Thesauros stays pure for domain packs.

### 4.4 SpawnServiceImpl's single-turn limitation

**Planning assumption:** CC bridge replaces SpawnServiceImpl.

**Reality:** `SpawnServiceImpl` is single-turn (`spawn_svc.rs:117`). Claude Code sessions are multi-turn with `--continue`/`--resume`. The SpawnService trait returns a single `SpawnResult` — it doesn't model multi-turn interactions.

**Resolution:** For simple dispatch (single prompt → result), the current trait works. For multi-turn CC sessions (monitoring, incremental results), the trait needs extension or a parallel `SessionService` trait.

### 4.5 No session-level tool sequence tracking

**Planning assumption:** Tool call sequences can be reconstructed for pattern detection.

**Reality:** `TurnResult.tool_calls` captures per-turn sequences. Session-level sequences require reconstructing from `SessionStore` message history across turns.

**Resolution:** Either (a) accumulate session-level tool sequences in the actor (in-memory, flushed on session close), or (b) query SessionStore for recent turns during the skill analysis pass.

---

## 5. Recommended Changes

### Existing crate modifications

| Crate | Change | Effort | Priority |
|-------|--------|--------|----------|
| **nous/bootstrap** | Add `resolve_skills()` method that queries mneme and returns `Vec<BootstrapSection>` | Low | High |
| **nous/actor** | Add `maybe_spawn_skill_analysis()` alongside existing extraction | Low | High |
| **nous/spawn_svc** | Add `CcBridgeSpawnService` implementing `SpawnService` trait | Medium | High |
| **mneme/knowledge_store** | Add skill-specific queries (by fact_type, domain, confidence) | Low | Medium |
| **mneme/extract** | Add `SkillExtractionEngine` parallel to `ExtractionEngine` | Medium | Medium |
| **organon/types** | Consider extending `SpawnService` for multi-turn sessions | Low | Low |
| **thesauros** | No changes needed — skills bypass thesauros entirely | None | N/A |

### New components

| Component | Location | Purpose | Effort |
|-----------|----------|---------|--------|
| SkillLoader | `crates/nous/src/skills/` or `crates/mneme/src/skills/` | Query mneme for relevant skills, convert to BootstrapSection | Medium |
| SkillHeuristics | `crates/mneme/src/skills/heuristics.rs` | Rule-based tool pattern scoring | Low |
| CcBridgeSpawnService | `crates/nous/src/cc_bridge.rs` | SpawnService impl wrapping `claude -p` CLI | Medium |
| CC skill exporter | `crates/nous/src/skills/export.rs` | Mneme facts → `.claude/skills/*/SKILL.md` | Low |

---

## 6. Latency and Performance Concerns

### Bootstrap skill resolution

**Path:** mneme embed query → HNSW search → recall scoring → section conversion

| Step | Estimated Latency | Notes |
|------|-------------------|-------|
| Embed task description | 10-50ms | fastembed-rs with warm model |
| HNSW kNN search (k=10) | 1-5ms | CozoDB in-memory, 384-dim, <1000 embeddings |
| Recall scoring | <1ms | 6-factor weighted sum, in-memory |
| Section conversion | <1ms | String formatting |
| **Total** | **12-57ms** | Acceptable for session start |

**Risk:** Cold model loading for fastembed-rs could add 1-2 seconds on first call. Mitigate with model preloading at startup.

### Skill extraction (background)

**Path:** heuristic filter → candidate lookup → (optional) LLM extraction

| Step | Estimated Latency | Notes |
|------|-------------------|-------|
| Heuristic filter | <1ms | In-memory pattern matching on Vec<ToolCall> |
| Candidate lookup/update | 5-20ms | CozoDB query + insert |
| LLM extraction (Haiku) | 1-3s | Only when candidate count ≥ 3; background task |

**No user impact** — all background, fire-and-forget.

### CC bridge dispatch

**Path:** prepare context → spawn CC process → wait for completion → parse result

| Step | Estimated Latency | Notes |
|------|-------------------|-------|
| Skill export to disk | 10-50ms | Write .claude/skills/ files |
| CC process spawn | 100-500ms | Process creation + CC initialization |
| CC execution | 5-300s | Depends on task complexity |
| Result parsing | <1ms | JSON parse |

**This replaces SpawnServiceImpl's 5-60s latency** (single Sonnet/Haiku turn). CC sessions may be slower to start but more capable.

### CozoDB query contention

Knowledge store uses `Arc<Db>` with CozoDB's internal concurrency. Multiple parallel queries (skill lookup + fact extraction + recall) may contend. CozoDB handles this via MVCC, but benchmark under concurrent load.

---

## 7. Dependency Graph

```
Phase 0: No code changes needed
├── Validate assumptions (this document) ✓
└── Research outputs (106-109) ✓

Phase 1: Foundation (can be parallel)
├── 1a. Skill-specific mneme queries
│   └── Add query helpers for fact_type="skill" filtering
├── 1b. SkillLoader → BootstrapSection conversion
│   └── Depends on: 1a (mneme queries)
└── 1c. CcBridgeSpawnService shell
    └── Depends on: nothing (implements existing trait)

Phase 2: Skill Loading (depends on 1a, 1b)
├── 2a. resolve_skills() in BootstrapAssembler
│   └── Depends on: 1b (SkillLoader)
├── 2b. Wire into pipeline.assemble_context_with_extra()
│   └── Depends on: 2a
└── 2c. Embedding pipeline for skill descriptions
    └── Depends on: 1a (skill facts in mneme)

Phase 3: Skill Capture (depends on 1a)
├── 3a. Heuristic filter for tool patterns
│   └── Depends on: nothing (uses TurnResult.tool_calls)
├── 3b. Candidate tracking in mneme
│   └── Depends on: 1a (fact storage)
├── 3c. Background skill analysis task in actor
│   └── Depends on: 3a, 3b
└── 3d. LLM-based skill extraction (v2)
    └── Depends on: 3c (candidates with count ≥ 3)

Phase 4: CC Integration (depends on 1c)
├── 4a. CC bridge skill export (.claude/skills/)
│   └── Depends on: 1a (skill query), CC format knowledge
├── 4b. CC bridge worktree management
│   └── Depends on: 1c (bridge shell)
├── 4c. CC bridge result parsing
│   └── Depends on: 1c
└── 4d. Wire into ToolServices.spawn
    └── Depends on: 4a, 4b, 4c

Phase 5: Quality & Evolution (depends on 2, 3)
├── 5a. Skill usage tracking (access_count on load)
│   └── Depends on: 2a (skills are loaded)
├── 5b. Skill confidence scoring
│   └── Depends on: 5a (usage data)
└── 5c. Skill decay and pruning
    └── Depends on: 5b (confidence signals)
```

**Critical path:** 1a → 1b → 2a → 2b (skill loading works end-to-end)
**Parallel track:** 1c → 4a-4d (CC bridge, independent of skill loading)
**Parallel track:** 3a → 3b → 3c (skill capture, independent of CC bridge)

---

## 8. Cross-Reference with Research Outputs

### From cc-skill-format.md (prompt 106)

**Validated:** CC skill format is filesystem-based SKILL.md with YAML frontmatter. The export pipeline (Phase 4a) can produce this format from mneme facts. The `$ARGUMENTS` and `${CLAUDE_SKILL_DIR}` substitution variables are the only dynamic elements — Aletheia-generated skills should use static content for CC export.

**Key insight:** CC matching is pure LLM reasoning on descriptions, not embeddings. Aletheia's embedding-based matching is a genuine advantage — it scales better and doesn't consume context window budget for skill descriptions.

### From pattern-detection.md (prompt 108)

**Validated:** The recommended hybrid pipeline (heuristic filter → candidate tracker → LLM extraction) maps cleanly to the codebase:
- Heuristic filter consumes `TurnResult.tool_calls` (confirmed available)
- Candidate tracker uses `KnowledgeStore.insert_fact()` with `fact_type: "skill_candidate"` (confirmed feasible)
- LLM extraction uses `ExtractionProvider` trait (confirmed reusable)
- The Rule of Three (promote at count ≥ 3) aligns with the per-turn extraction model

### From optimal-prompting.md (prompt 107)

**Validated:** The 40K token bootstrap budget aligns with the research finding that CLAUDE.md-style instructions have diminishing returns beyond ~150 instructions. Skills as Flexible-priority sections that get truncated under pressure is the right design — workspace identity (Required/Important) should always win.

### From model-capability-audit.md (prompt 109)

**Validated:** The audit confirms SpawnService should be replaced with CC subagents for execution tasks. The existing trait boundary (`SpawnService`) is the exact interface needed for a CC bridge drop-in. The audit's recommendation to keep knowledge graph, identity, and cross-agent topology while replacing file tools and spawn service aligns perfectly with the CC execution layer proposal.
