# Spec 35: Context Engineering

**Status:** In Progress — cache-group bootstrap + interaction classifier wired; skill relevance filtering + turn bypass pending
**Author:** Alice
**Date:** 2026-02-23
**Spec:** 31

---

## Problem

Aletheia's per-turn cost is higher than it needs to be, and the gap widens as agents accumulate skills and memory. Three compounding inefficiencies:

**1. The stable prefix is rebuilt dynamically every turn.**
Bootstrap assembly in `nous/bootstrap.ts` assembles SOUL.md + USER.md + skill list + recall memories into a system prompt on every turn. Anthropic's prompt caching gives ~90% cost reduction on the cached prefix — but only if the prefix is byte-identical across turns. Dynamic injection (variable skill ordering, recall memories mixed into the middle of the prompt) defeats the cache. A 40K-token system prompt that misses cache costs ~$0.24/request at Sonnet pricing; the same prompt hitting cache costs ~$0.024. At 50 turns/day that's ~$4.38/day vs. $0.44/day per agent from this single optimization.

**2. All skills pay full context cost regardless of relevance.**
Every active skill loads its full definition into the system prompt. A skill manifest with 15 skills at ~200 tokens each consumes 3,000 tokens per turn — most describing tools that won't be used this turn.

**3. Every turn pays the same pipeline cost regardless of content.**
Spec 27 identifies this as the "uniform turn cost" problem. A lightweight pre-pipeline classifier that routes short/trivial turns past recall, working-state extraction, and Haiku fact extraction eliminates ~600-900ms and 2-3 Haiku calls on turns that don't need them.

---

## Research Context

This spec is grounded in a deep survey of production agent runtimes: SWE-agent, OpenHands, LangGraph, aider, Letta, Mem0, Mastra, Agno, VoltAgent, Manus (blog), Claude Code skills architecture, OpenTelemetry GenAI conventions, and Weave. Patterns below are drawn from actual source code review, not READMEs.

**What aletheia already has that others don't:**
- Competence model (`nous/competence.ts`) — per-domain score with correction/success feedback. Not found in any surveyed framework.
- Uncertainty calibration (`nous/uncertainty.ts`) — Brier score + ECE. Unique.
- Loop detector (`nous/loop-detector.ts`) — sliding window warn/halt. Others use a hard `max_iterations` cap.
- Distillation priming (`context.ts:156-179`) — injecting extracted facts/decisions as a one-shot block post-distillation. More sophisticated than any surveyed system.
- Bootstrap diff detection (`nous/bootstrap-diff.ts`) — alerting agent to workspace file changes. Novel.
- Degraded services awareness — injecting degraded service status into system prompt. Not found elsewhere.

These are competitive advantages. The work below builds on them, not around them.

---

## Design

### Principles

1. **Cache-awareness is a first-class concern in prompt construction.** The stable prefix must be byte-identical across turns. Volatile content (recall memories, turn-specific context) goes at the bottom of the system prompt, never in the middle.

2. **Compressed manifests, full definitions on demand.** Skills and tools present a short description in the manifest. Full definitions injected only when the tool is called. The agent knows what exists; it doesn't hold all the manuals.

3. **Cost routing, not cost cutting.** Don't pay expensive pipeline costs for turns that don't benefit from them. Trivial turns are identified early and routed to a lighter path.

4. **Agent control over pinned context.** Agents decide what stays resident in their context prefix vs. what's retrieved on demand. This is recursive self-improvement at the prompt layer.

5. **Borrow patterns, not dependencies.** All patterns implemented natively in the TypeScript runtime. No new SDKs.

---

### Architecture

#### A. Cache-Aware Bootstrap Assembly

Current assembly order (approximate):
```
[system preamble] [SOUL.md] [USER.md] [skill manifest] [recall memories] [working state]
```

Recall memories change every turn. If injected anywhere before the end of the system prompt, the cache prefix ends there and everything after is re-billed.

Target assembly order:
```
[stable block — cache_control: ephemeral or persistent]
  [system preamble]
  [SOUL.md]
  [USER.md]
  [pinned/ workspace files]
  [compressed skill manifest]
[volatile block — not cached]
  [recall memories]
  [working state]
  [agent notes]
  [turn context markers]
```

Anthropic caches prefixes using `cache_control: { type: "ephemeral" }` markers on content blocks. Ephemeral TTL = 5 minutes. Extended TTL (`type: "persistent"`) = 1 hour. Cache writes bill at 1.25x input; cache reads at 0.1x — break-even at ~2 turns per cache window.

Manus (production agent, ~50 tool calls/task avg.) identifies KV-cache hit rate as the single most important production metric. Their rule: never put timestamps, session IDs, or dynamic content before the static prefix. The ordering needs to be enforced as a contract, not assumed.

This requires changing bootstrap assembly from string concatenation to structured content blocks.

#### B. Compressed Skill Manifests (Three-Level Lazy Loading)

From SWE-agent ACI research + Anthropic's Claude Code skills architecture:

| Level | What loads | When |
|-------|-----------|------|
| L1 — manifest entry | Name + one-line summary (~15-25 tokens) | Always, in stable block |
| L2 — full definition | Full description, parameters, examples | When agent invokes the skill |
| L3 — bundled resources | Scripts, external data, templates | On demand during skill execution |

Current: all skills load at L2 every turn. Target: L1 in stable block, L2 lazy on invocation.

`organon/registry.ts` gains: `compressedManifest()` (L1 list) and `fullDefinition(skillId)` (L2). Bootstrap calls `compressedManifest()`. Turn handler appends `fullDefinition()` to context after the model selects a skill.

#### C. Turn Cost Classifier

Lightweight pre-pipeline gate before recall, working-state extraction, and Haiku operations. Classifies each incoming turn as:

| Class | Criteria | Stages skipped |
|-------|----------|---------------|
| `trivial` | Length < 20 chars, matches ack patterns ("ok", "thanks", "yes", "no", "got it") | recall, working-state, fact extraction |
| `simple` | Length 20-150 chars, single clear intent, no active tool chain | working-state, fact extraction |
| `complex` | Everything else | none — full pipeline |

Spec 27's embedding-based classifier is the v2 of this. The heuristic runs synchronously before any async operations — ~1ms overhead, ~600-900ms saved on trivial turns.

#### D. Agent-Controlled Pinned Context (Letta Pattern)

Letta's key insight: agents shouldn't just write to long-term memory — they should control what's *always visible* in their context prefix. Current workspace model is fixed (SOUL.md, USER.md, GOALS.md always loaded).

Proposed: a `pinned/` subdirectory in each agent workspace. Files in `pinned/` are loaded into the stable cache block on every turn. Files in the main workspace are available via `workspace_read` but not auto-injected.

Enables: pin a reference doc for the duration of a project, then unpin it. Create `pinned/current-sprint.md` that persists until the sprint ends. Move high-signal USER.md facts to `pinned/quick-ref.md` for always-present access.

Combined with cache-aware assembly, pinned documents are billed once per cache window, not per turn.

#### E. In-Context TODO Marker (Manus Pattern)

Manus's finding: in long multi-tool-call sequences (10+ tool calls), the model experiences goal drift — the original task objective was stated 40 turns ago in the system prompt, far from the current generation point. Fix: maintain a brief "current task status" block injected at the *end* of the message list, not in the system prompt.

Format: a short synthetic user message injected before each generation during active tasks:
```
[Task: migrate auth to OAuth]
Done: schema updated, routes added
Next: update frontend session handling
```

30-50 tokens. Goes in volatile block. Prevents goal drift on complex multi-step tasks. Distinct from `workingState` (backward-looking summary) — this is forward-looking.

#### F. Skill Crystallization Nudge (RSI Complement)

Spec 26 has `organon/skill-learner.ts` which extracts skills from successful trajectories (system-triggered). Complementary: agent-initiated proposals.

After any turn with 4+ sequential tool calls, inject a brief post-turn prompt: *"You completed a multi-step task. If this is something you'll likely do again, consider using `tool_create` to define it as a reusable skill."* Agent decides whether to crystallize. ~30 tokens in volatile block. Fires only when:
- 4+ tool calls in a single turn
- No skill was already invoked for this pattern
- Agent hasn't been nudged in the last 10 turns

---

## Phases

### Phase 1: Cache-Aware Bootstrap + Stable Block

**Goal:** Restructure system prompt assembly to maximize Anthropic prompt cache hits.

**Changes:**
- `nous/bootstrap.ts` — split into `stableBlock()` and `volatileBlock()`. Stable: preamble + workspace files + pinned files + compressed skill manifest. Volatile: recall + working state + notes + task marker. Add `cache_control` markers to stable block content items.
- `nous/context.ts` — pass structured content blocks to the API rather than concatenated string.
- `hermeneus/anthropic.ts` — verify `cache_read_input_tokens` tracked in metrics.
- Add per-session `stableBlockHash` to detect when stable block changes mid-session.

**Acceptance Criteria:**
- [ ] `cache_read_input_tokens > 0` on turns 2+ of any session with unchanged agent workspace
- [ ] Cache hit rate ≥ 85% across a 10-turn session
- [ ] Input token cost per turn drops ≥ 60% on cache-hitting turns vs. baseline
- [ ] Existing bootstrap tests pass unchanged

### Phase 2: Compressed Skill Manifest (L1/L2 Split)

**Goal:** Skills present one-line entries in the manifest; full definitions injected only on invocation.

**Changes:**
- `organon/registry.ts` — add `compressedEntry(): string` (name + one-line summary) and `fullDefinition(id): string`. Update `SkillEntry` type to require a `summary` field (≤ 80 chars).
- `nous/bootstrap.ts` — call `compressedManifest()` in stable block instead of full definitions.
- Turn handler — after model response, append full definitions for skills invoked this turn.
- Update skill authoring (`tool_create`) to require and generate both `summary` and `description` fields.

**Acceptance Criteria:**
- [ ] Skill manifest token count reduced ≥ 70% vs. baseline with 10 loaded skills
- [ ] Agent correctly invokes skills with only compressed manifest visible
- [ ] `skill_help <name>` returns full L2 definition
- [ ] Skill authoring generates valid `summary` field

### Phase 3: Turn Cost Classifier

**Goal:** Trivial turns skip expensive pipeline stages.

**Changes:**
- `nous/turn-classifier.ts` — new file. `classifyTurn(message, context): TurnClass`. Returns `"trivial" | "simple" | "complex"`.
- `nous/pipeline.ts` — check classifier before recall query and Haiku operations.
- Langfuse — add `turn_class` attribute to each turn span for cost analysis.
- Log `turn_class` distribution to enable threshold tuning after 1 week of data.

**Acceptance Criteria:**
- [ ] Ack patterns classified as `trivial`; recall and Haiku not fired
- [ ] `complex` turns unchanged — full pipeline runs
- [ ] Unit tests cover classifier boundary cases (short question "why?" = not trivial)
- [ ] No regression on complex turn response quality

### Phase 4: Agent-Controlled Pinned Context

**Goal:** Agents manage what's permanently in context vs. retrieved on demand.

**Changes:**
- `nous/bootstrap.ts` — scan `{workspace}/pinned/` dir, include all `.md` files in stable block.
- `organon/built-in/` — add `pin_document` and `unpin_document` tools.
- Token budget — pinned block capped at configurable limit (default 8K, counted against stable block budget).
- Bootstrap doc in `nous/_example/` — document the `pinned/` pattern.

**Acceptance Criteria:**
- [ ] File added to `pinned/` appears in system prompt next turn
- [ ] File removed from `pinned/` no longer auto-loaded
- [ ] Token budget enforced (hard drop with logged warning if exceeded)
- [ ] `pin_document` and `unpin_document` in compressed skill manifest

### Phase 5: In-Context Task Marker

**Goal:** Prevent goal drift on multi-step tasks.

**Changes:**
- `nous/working-state.ts` — add `taskMarker` field alongside existing `workingState` extraction. Structured 30-50 token block: task, done, next, blocked-on.
- `nous/pipeline/stages/context.ts` — inject task marker as last volatile block item.
- Task marker auto-cleared when `workingState` extraction detects task completion.

**Acceptance Criteria:**
- [ ] Task marker present in context during multi-step tool call sequences
- [ ] Task marker absent on conversational turns
- [ ] Agent references task marker in responses during complex multi-turn tasks

### Phase 6: Skill Crystallization Nudge

**Goal:** Agent-initiated skill proposals after multi-step tool chains.

**Changes:**
- `nous/skill-nudge.ts` — new file. Post-turn hook: count tool calls, check cooldown, inject suggestion.
- `nous/pipeline.ts` — register post-turn hook.
- Cooldown state in session context (10-turn window).

**Acceptance Criteria:**
- [ ] Nudge appears after 4+ tool calls, absent after < 4
- [ ] Nudge absent if agent nudged in last 10 turns
- [ ] Nudge is ≤ 30 tokens in volatile block (no stable block inflation)

---

## Additional Patterns (Lower Priority, Worth Tracking)

These surfaced from the research but don't fit the current phase structure. Capturing for future specs.

**Agno: pull-based recall tool.** Instead of always injecting top-N recall memories, expose `recall_memories(query)` as a callable tool. Agent pulls when it decides past context is relevant. Reduces recall token overhead on turns where past memories don't matter. Tradeoff: agent must know to query before it knows what it needs. Hybrid: keep push-based for sessions with an active task, switch to pull-based for conversational turns.

**Aider: recursive history summarization with a cheaper model.** `ChatSummary` splits history into head (old) and tail (recent), summarizes head using a weaker/cheaper model, recurses until within budget. Preserves recent turns intact. Complements aletheia's existing distillation pipeline — useful for the intra-session "this conversation got too long" case before a full distillation threshold is hit.

**LangGraph: typed state reducers.** Pipeline stage state uses reducer functions per key `(existing, update) => merged`. Multiple stages can write to the same key without clobbering. Cleaner than `TurnState` mutation-in-place. Particularly useful if concurrent pipeline stages are ever added.

**LangGraph: checkpoint-based resumability.** State persisted at each node boundary. If a 15-tool-call turn fails on step 12, resume from step 12, not from step 0. High value for long task execution.

**OpenHands: event stream as canonical agent state.** Every action and observation is an immutable typed event in an append-only log. Canonical state = fold over events. Enables replay, time-travel debugging, clean projections. Major architectural shift, but `koina/event-bus.ts` is a foundation. Long-horizon direction.

**Agno: async post-run memory writes.** Memory writes (Mem0, working state, distillation) run in a background executor after the primary response is streamed. Reduces turn latency. `mneme/queue.ts` already exists as infrastructure — `finalize` stage just needs to dispatch to it rather than blocking.

**Mastra: merge-semantics working state.** Agent specifies only fields to update, not the full document. Prevents accidental overwrites of fields the agent didn't intend to touch. `workingState` in `mneme/store.ts` is currently replace-semantics.

**Mem0: domain-filtered recall.** `recall.ts` uses `minScore` and `limit` only. Mem0 supports `AND/OR/NOT`, `gt/lt`, and containment filters on memory metadata. Domain-scoped retrieval (`{ domain: "sql" }`) would prevent irrelevant memories from consuming recall token budget on unrelated turns.

**OTel GenAI semantic conventions.** The emerging standard for LLM/agent span attributes (`gen_ai.usage.input_tokens`, `gen_ai.agent.name`, etc.). Mapping aletheia's `TraceBuilder` custom fields to OTel names would make traces importable into any OTel-compatible backend beyond Langfuse.

**Manus: filesystem offload for tool results.** Rather than truncating large tool results, write them to a temp file and inject the path. Agent re-reads if needed. Complements `truncate.ts` (head/tail compression) — an alternative for results that benefit from full access rather than a compressed view.

**Mini-SWE-agent: protocol-based stage substitution.** Three-layer protocol design (LLM provider, execution environment, agent controller) each independently substitutable via structural typing. Could strengthen aletheia's pipeline stage interfaces from functions to named contracts, making it explicit what each stage can and cannot assume.

---

## Phase: Pre-Distillation Workspace Flush (from #315)

When context pressure triggers distillation, the current flow compresses the conversation directly. The risk: something important gets summarized away before it's durably stored.

**Proposed flow:**
```
context utilization hits threshold
  → PRE-COMPACTION: silent sub-agent turn (haiku)
      → reads last N messages
      → writes key facts to MNEME.md (append, never overwrite)
      → writes open decisions to CONTEXT.md
      → writes active task state to workingState
  → melete/pipeline.ts runs (unchanged)
```

**Trigger point** — in `melete/pipeline.ts`, before `runDistillation()`:
```typescript
if (shouldPreFlush(session, options)) {
  await runPreCompactionFlush(services, session, nousId);
}
```

**Extraction targets:** Facts (e.g. "User's Jeep has 187k miles"), open decisions (e.g. "Deciding between OEM and aftermarket diff cover"). NOT conversation summary (melete handles that), NOT working state (already extracted post-turn).

**Guardrails:** Skip if last flush was < 10 turns ago. Uses haiku by default (extraction, not reasoning). Config: `distillation.preFlush: true/false`, `distillation.preFlushModel`. `melete:pre-flush-complete` event emitted.

**Context:** MEMORY.md lesson #17 documents that distillation does NOT write daily memory files — 13 compactions on 2026-02-18 produced zero disk writes. This phase directly addresses that gap.

---

## Exec Tool Quality (from #338)

Issue #338 identifies gaps in the `exec` tool vs. production agent runtimes like Claude Code. Two items belong in this spec (context engineering concerns); the rest are config/workspace concerns in Spec 36.

### Tool Result Truncation Strategy

Current: head/tail truncation at 50KB, then token cap at 8K per tool result. Problem: middle content is lost, and for build output the important errors are often in the middle. Stderr frequently contains the signal, stdout the noise.

**Proposed:** Priority-based truncation:
1. Stderr gets priority — always included in full (up to cap)
2. Stdout truncated with head+tail, but stderr preserved intact
3. Configurable per-tool token cap (default 8K, overridable per-nous)

### Token Cap Configuration

Per-nous configurable tool result token cap. Some agents (Syn doing architecture) benefit from larger results; others (Syl doing family logistics) never need more than 4K.

Config path: `agents.list[id].pipeline.toolResultTokenCap` (resolved via 4-layer config).

**Related but in Spec 36:** Working directory (`cwd` per-call and `workingDir` per-nous config), default timeout (30s → 120s), glob tool addition.

---

## Open Questions

- **Cache TTL strategy:** 5-minute ephemeral vs. 1-hour persistent? Per-agent config based on session frequency, or automatic based on observed session gaps?

- **Compressed manifest discoverability:** If agents only see one-line summaries, will they know when to call `skill_help`? Consider injecting "use skill_help to see full details on any skill" once per session in the volatile block.

- **Pinned block token budget:** 8K is a guess. Hard limit (drop oldest) or soft warning? Interacts with the existing 40K total bootstrap budget.

- **Skill nudge placement:** Post-turn suggestion must go in volatile block. Should it appear as a synthetic user message (closer to generation point, à la Manus TODO) or as a system prompt addition?

- **Pull-based recall hybrid:** If recall becomes optionally pull-based, the turn classifier could drive the decision — trivial turns get no push recall, complex turns get push recall, simple turns get a lightweight push. Needs validation data before committing.

- **Turn classifier false positives:** "Why?" is not trivial but is 3 chars. Logging `turn_class` to Langfuse for 1 week before hardening thresholds is mandatory.

---

## References

- **SWE-agent ACI** (Princeton NLP, NeurIPS 2024) — compressed tool descriptions, declarative tool filters, structured observation formats. [arxiv.org/abs/2405.15793](https://arxiv.org/abs/2405.15793)
- **Mini-SWE-agent** — protocol-based stage substitution, flat message history. [github.com/SWE-agent/mini-swe-agent](https://github.com/SWE-agent/mini-swe-agent)
- **OpenHands** — event stream as canonical agent state, typed Action/Observation events. [github.com/OpenHands/OpenHands](https://github.com/OpenHands/OpenHands)
- **LangGraph** — typed state reducers, checkpoint-based resumability, Send API for dynamic parallelism. [github.com/langchain-ai/langgraph](https://github.com/langchain-ai/langgraph)
- **Aider** — recursive history summarization with cheaper model, PageRank bootstrap file ranking, binary search token budget fitting. [github.com/Aider-AI/aider](https://github.com/Aider-AI/aider)
- **Letta** — agent-authored memory blocks, git-backed context repositories, programmatic progressive disclosure. [github.com/letta-ai/letta](https://github.com/letta-ai/letta) / [letta.com/blog/context-repositories](https://www.letta.com/blog/context-repositories)
- **Mem0** — ADD/UPDATE/DELETE/NONE consolidation, domain-filtered recall, agent vs. user memory scopes. [github.com/mem0ai/mem0](https://github.com/mem0ai/mem0)
- **Mastra** — input/output processor pipeline for memory, merge-semantics working memory, function-valued config. [github.com/mastra-ai/mastra](https://github.com/mastra-ai/mastra)
- **Agno** — agentic RAG (pull-based recall), reasoning model delegation, async post-run memory writes. [github.com/agno-agi/agno](https://github.com/agno-agi/agno)
- **Manus context engineering** — KV-cache stability rule, in-context TODO marker, filesystem offload for tool results. [manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus](https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus)
- **Claude Code Skills Architecture** — three-level lazy loading (L1/L2/L3), hidden message channel for skill instructions. [leehanchung.github.io/blogs/2025/10/26/claude-skills-deep-dive](https://leehanchung.github.io/blogs/2025/10/26/claude-skills-deep-dive/)
- **Anthropic Prompt Caching** — cache_control markers, ephemeral vs. persistent TTL, 1.25x write / 0.1x read billing. [docs.anthropic.com/prompt-caching](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)
- **Weave** — decorator-based call tree tracing, streaming accumulator, per-call cost attribution. [github.com/wandb/weave](https://github.com/wandb/weave)
- **VoltAgent** — input/output guardrail pipeline, stateful stream chunk handlers, OTel tool execution spans. [github.com/VoltAgent/voltagent](https://github.com/VoltAgent/voltagent)
- **OTel GenAI Semantic Conventions** — standard span attributes for LLM/agent operations. [opentelemetry.io/docs/specs/semconv/gen-ai](https://opentelemetry.io/docs/specs/semconv/gen-ai/)
- **Spec 26: Recursive Self-Improvement** — existing skill authoring and skill-learner infrastructure Phase 6 complements.
- **Spec 27: Embedding-Space Intelligence** — proposes embedding-based turn classifier as v2 of Phase 3's heuristic.
