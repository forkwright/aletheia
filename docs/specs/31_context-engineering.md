# Spec 31: Context Engineering

**Status:** Draft
**Author:** Cody
**Date:** 2026-02-23
**Spec:** 31

---

## Problem

Aletheia's per-turn cost is higher than it needs to be, and the gap widens as agents accumulate skills and memory. Three compounding inefficiencies:

**1. The stable prefix is rebuilt dynamically every turn.**
Bootstrap assembly in `nous/bootstrap.ts` assembles SOUL.md + USER.md + skill list + recall memories into a system prompt on every turn. Anthropic's prompt caching gives ~90% cost reduction on the cached prefix — but only if the prefix is byte-identical across turns. Dynamic injection (interpolated timestamps, variable skill ordering, recall memories mixed into the middle of the prompt) defeats the cache. A 40K-token system prompt that misses cache costs ~$0.24/request at Sonnet pricing; the same prompt hitting cache costs ~$0.024. At 50 turns/day that's ~$4.38/day vs. $0.44/day per agent from this single optimization.

**2. All skills pay full context cost regardless of relevance.**
Every active skill loads its full definition into the system prompt. A skill manifest with 15 skills at ~200 tokens each consumes 3,000 tokens per turn — most of which describe tools that won't be used. SWE-agent's ACI research demonstrated that tool descriptions should have two modes: a compressed ~20-token "available" entry shown in the manifest, and the full definition only loaded when the tool is invoked. This mirrors how a skilled human knows a tool exists without holding its manual in working memory.

**3. Every turn pays the same pipeline cost regardless of content.**
Spec 27 identifies this as the "uniform turn cost" problem and proposes an embedding-space solution. A cheaper, earlier-landing approach: a lightweight turn classifier that runs before the expensive pipeline stages and routes short/trivial turns (confirmations, clarifications, single-word responses) past the recall query, working-state extraction, and Haiku fact extraction. These three stages account for ~600-900ms and 2-3 Haiku calls per turn on turns that don't need them.

---

## Design

### Principles

1. **Cache-awareness is a first-class concern in prompt construction.** The stable prefix must be assembled in a way that maximizes byte-identity across turns. Volatile content (recall memories, turn-specific context) goes at the bottom of the system prompt, never in the middle.

2. **Compressed manifests, full definitions on demand.** Skills and tools present a short description in the manifest. Full definitions are injected only when the tool is called or explicitly requested. The agent knows what exists; it doesn't need to hold all the details.

3. **Cost routing, not cost cutting.** The goal isn't to degrade capability — it's to not pay expensive pipeline costs for turns that don't benefit from them. Trivial turns should be identified early and routed to a lighter path.

4. **Agent control over pinned context.** Agents should be able to decide what stays resident in their context prefix vs. what's retrieved on demand. This is an extension of RSI: the agent can improve its own bootstrap efficiency over time.

5. **Borrowing patterns, not dependencies.** These ideas come from SWE-agent (ACI), Letta (context repositories, progressive disclosure), Mastra (working memory), and DSPy (prompt compilation). The implementation is native TypeScript in the aletheia runtime — no new SDKs.

---

### Architecture

#### A. Cache-Aware Bootstrap Assembly

Current assembly order in `nous/bootstrap.ts` (approximate):
```
[system preamble] [SOUL.md] [USER.md] [skill manifest] [recall memories] [working state]
```

The recall memories change every turn. If they're injected anywhere before the end of the system prompt, the cache prefix ends at that point and everything after is re-billed.

Target assembly order:
```
[cache block — stable]
  [system preamble]
  [SOUL.md]
  [USER.md]
  [compressed skill manifest]
[volatile block — never cached]
  [recall memories]
  [working state]
  [agent notes for this turn]
```

The Anthropic API caches prefixes using the `cache_control: { type: "ephemeral" }` marker on content blocks. The stable block gets this marker. The volatile block does not. Cache TTL is 5 minutes (ephemeral) or 1 hour (extended, `type: "persistent"`). For agents with stable SOUL/USER files, extended caching of the stable block would be appropriate.

This requires changing bootstrap assembly from string concatenation to structured content blocks (which the API already supports).

#### B. Compressed Skill Manifests (ACI Pattern)

Current: each skill entry in the manifest includes its full description, parameters, examples.

Target: two-tier skill representation.

**Manifest entry** (always loaded, ~15-25 tokens):
```
- memory_search: Search agent memories by query
- tool_create: Author and deploy a new skill
- workspace_read: Read a workspace file
```

**Full definition** (loaded on invocation or via `skill_help <name>`):
```typescript
// Injected as a user message tool_result or appended to system prompt
// only when the skill is being called this turn
```

Implementation: `organon/registry.ts` maintains both a `compressedManifest()` and `fullDefinition(skillId)` method. Bootstrap calls `compressedManifest()`. The turn handler calls `fullDefinition()` for skills used in that turn and appends them to the context before passing to the model.

This connects directly to spec 26's dynamic tool loading (the `enable_tool` pattern) — extending it to cover the description granularity, not just visibility.

#### C. Turn Cost Classifier

A lightweight pre-pipeline gate that runs before recall, working-state extraction, and Haiku operations. Classifies each incoming turn as:

| Class | Criteria | Pipeline stages skipped |
|-------|----------|------------------------|
| `trivial` | Length < 20 chars, no tool calls in context, no question marks, matches ack patterns ("ok", "thanks", "got it", "yes", "no") | recall, working-state, fact extraction |
| `simple` | Length 20-150 chars, single clear intent, no active tool chain | working-state, fact extraction |
| `complex` | Everything else | none — full pipeline |

Implementation: pure heuristic in `nous/turn-classifier.ts`, ~50 lines. Runs synchronously before any async operations. Spec 27's embedding-based approach is complementary — that's a v2 once the heuristic is validated.

#### D. Agent-Controlled Pinned Context (Letta Context Repository Pattern)

Letta's key insight: agents shouldn't just write to long-term memory — they should control what's always visible in their context prefix. The current workspace model is fixed: SOUL.md, USER.md, GOALS.md are always loaded. Agents can't decide "this document should be in my context every turn" vs. "this should be retrieved on demand."

Proposed: a `pinned/` subdirectory in each agent workspace. Any `.md` file in `pinned/` is loaded into the stable cache block on every turn. Files in the main workspace are available via `workspace_read` but not auto-injected.

This lets agents do things like:
- Pin a frequently-referenced reference document during a project, then unpin it when done
- Move USER.md facts to pinned after they're confirmed, promoting from retrieved to always-present
- Create a task-specific `pinned/current-project.md` that lives in context until the project is complete

The stable cache block grows and shrinks as the agent manages its pinned directory. Combined with cache-aware assembly, pinned documents are only billed once per cache window.

#### E. Skill Crystallization from Observation (Passive Pattern)

Spec 26 has `organon/skill-learner.ts` which extracts skills from successful tool call trajectories. This is system-triggered. The complementary pattern: agent-initiated skill proposals.

After any turn where the agent performed 4+ sequential tool calls to accomplish a task, inject a brief post-turn prompt: *"You just completed a multi-step task. If this is something you'll likely do again, consider using `tool_create` to define it as a reusable skill."* This is a lightweight nudge rather than an automatic extraction — the agent decides whether to crystallize.

The nudge is only shown when:
- 4+ tool calls in a single turn
- No skill was already invoked for this pattern
- Agent hasn't been nudged in the last 10 turns

Implementation: `nous/skill-nudge.ts`, ~30 lines, fires as a post-turn hook.

---

## Phases

### Phase 1: Cache-Aware Bootstrap

**Goal:** Restructure system prompt assembly to maximize Anthropic prompt cache hits.

**Changes:**
- `nous/bootstrap.ts` — split assembly into `stableBlock()` and `volatileBlock()`. Stable block: preamble + workspace files + compressed skill manifest. Volatile block: recall memories + working state + agent notes. Add `cache_control` markers to stable block content items.
- `nous/context.ts` — pass structured content blocks to the API rather than a single concatenated string.
- `hermeneus/anthropic.ts` — verify cache write/read token tracking is captured in metrics (Langfuse traces).

**Acceptance Criteria:**
- [ ] `cache_read_input_tokens > 0` on turns 2+ of a session with the same agent
- [ ] Cache hit rate ≥ 85% across a 10-turn session with no workspace file changes
- [ ] Input token cost per turn drops ≥ 60% on cache-hitting turns vs. baseline
- [ ] Existing tests pass unchanged (assembly contract preserved)

### Phase 2: Compressed Skill Manifest

**Goal:** Skills present short descriptions in the manifest; full definitions injected only on use.

**Changes:**
- `organon/registry.ts` — add `compressedEntry(): string` to `SkillEntry` type. Format: `- {name}: {one-line description}`. Add `fullDefinition(id): string` method.
- `nous/bootstrap.ts` — call `compressedManifest()` in stable block instead of full definitions.
- `nous/turn-handler.ts` (or equivalent) — after model response, if tool calls reference a skill, append full definitions to next-turn context.
- Update skill authoring format to require a `summary` field (one line, ≤ 80 chars) separate from the full `description`.

**Acceptance Criteria:**
- [ ] Skill manifest token count reduced ≥ 70% vs. baseline with 10 loaded skills
- [ ] Agent correctly invokes skills with only compressed manifest visible
- [ ] `skill_help <name>` returns full definition
- [ ] Skill authoring (`tool_create`) generates both `summary` and `description` fields

### Phase 3: Turn Cost Classifier

**Goal:** Trivial and simple turns skip expensive pipeline stages.

**Changes:**
- `nous/turn-classifier.ts` — new file. `classifyTurn(message: string, sessionContext: TurnContext): TurnClass`. Returns `"trivial" | "simple" | "complex"`.
- `nous/pipeline.ts` (or equivalent turn orchestrator) — check classifier before spawning recall query and Haiku operations. Pass `TurnClass` through to logging.
- Langfuse traces — add `turn_class` attribute to each turn span for cost attribution analysis.

**Acceptance Criteria:**
- [ ] "ok", "yes", "thanks", "got it" classified as `trivial`
- [ ] Recall and Haiku calls not fired on `trivial` turns
- [ ] `complex` turns unchanged — full pipeline runs
- [ ] Unit tests cover classifier boundary cases
- [ ] No regression in response quality on `complex` turns (verified by existing session tests)

### Phase 4: Agent-Controlled Pinned Context

**Goal:** Agents manage what's permanently in context vs. retrieved on demand.

**Changes:**
- `nous/bootstrap.ts` — scan `{workspace}/pinned/` dir, include all `.md` files in stable block (after SOUL/USER, before recall).
- `organon/built-in/` — add `pin_document` and `unpin_document` tools. `pin_document(filename)` moves/copies a workspace file to `pinned/`. `unpin_document(filename)` moves it back.
- Bootstrap doc in `nous/_example/` — document the `pinned/` pattern for new agents.
- Token budget — pinned files count against the stable block budget (tracked separately from recall).

**Acceptance Criteria:**
- [ ] File added to `pinned/` appears in system prompt on next turn without recall
- [ ] File removed from `pinned/` no longer auto-loaded
- [ ] `pin_document` and `unpin_document` visible in skill manifest
- [ ] Token budget enforced (pinned block capped at configurable limit, default 8K)

### Phase 5: Skill Crystallization Nudge

**Goal:** Agent-initiated skill proposals after multi-step tool chains.

**Changes:**
- `nous/skill-nudge.ts` — new file. Post-turn hook: count tool calls in completed turn, check nudge cooldown, inject suggestion into next system prompt addition.
- `nous/pipeline.ts` — register post-turn hook.
- Cooldown state in working state or session context to prevent nudge spam.

**Acceptance Criteria:**
- [ ] Nudge appears after 4+ tool calls in a single turn
- [ ] Nudge does NOT appear on turns with < 4 tool calls
- [ ] Nudge does NOT appear if agent was nudged in last 10 turns
- [ ] Nudge text is concise (≤ 30 tokens) and doesn't inflate the stable cache block

---

## Open Questions

- **Cache TTL strategy:** 5-minute ephemeral cache works for active conversations. For agents with long gaps between turns (overnight, weekends), extended 1-hour cache (`type: "persistent"`) may be more appropriate. Should cache type be per-agent config or automatic based on session activity?

- **Compressed manifest discoverability:** If agents only see one-line summaries, will they know when to call `skill_help`? Consider injecting "use skill_help to see full details on any skill" once per session in the volatile block.

- **Pinned block token budget:** 8K is a guess. Should this be a hard limit (drop oldest if exceeded) or a soft warning? Relates to the existing 40K total bootstrap budget in `bootstrap.ts`.

- **Skill nudge text placement:** Post-turn suggestion should go in the volatile block, not the stable block (it's per-turn). Needs careful placement so it doesn't accidentally get cached.

- **Turn classifier false positives:** A short message like "why?" is not trivial — it requires full recall to answer well. The 20-char threshold and ack-pattern matching may need tuning. Logging `turn_class` to Langfuse and reviewing after 1 week would ground the thresholds in real data.

---

## References

- **SWE-agent ACI paper** (Princeton NLP, NeurIPS 2024) — compressed tool descriptions reduce context use while preserving agent capability. Key finding: simple/efficient tools outperform complex/powerful tools because agents use them more reliably. [arxiv.org/abs/2405.15793](https://arxiv.org/abs/2405.15793)

- **Letta Context Repositories** (2026) — git-backed agent memory with programmatic progressive disclosure. Agents manage their own pinned/unpinned context hierarchy. [letta.com/blog/context-repositories](https://www.letta.com/blog/context-repositories)

- **Mastra Working Memory** — structured agent scratchpad with resource-scoped vs. thread-scoped persistence. Two implementation modes: free-form Markdown (replace semantics) or Zod schema (merge semantics). [mastra.ai/docs/memory/working-memory](https://mastra.ai/docs/memory/working-memory)

- **Anthropic Prompt Caching docs** — cache_control markers, ephemeral vs. persistent TTL, billing model (cache writes at 1.25x, cache reads at 0.1x input token price). [docs.anthropic.com/prompt-caching](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching)

- **Spec 26: Recursive Self-Improvement** — existing skill authoring and skill-learner infrastructure this spec extends. Phase 5 (skill crystallization nudge) is a lightweight complement to spec 26's trajectory-based extraction.

- **Spec 27: Embedding-Space Intelligence** — proposes embedding-based turn classification as v2 of the heuristic turn classifier proposed here. Phases are complementary, not competing.
