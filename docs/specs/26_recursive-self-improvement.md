# Spec 26: Recursive Self-Improvement

Aletheia agents autonomously improve their own capabilities — creating tools, refining strategies, proposing code patches, and curating their own memory — within a layered safety architecture that keeps the base model frozen and gates all structural changes behind automated verification.

## Research Context

Recursive self-improvement (RSI) in AI systems has moved from theoretical to deployed between 2024-2026. The working systems share a common architecture: a frozen foundation model generates mutations to a mutable scaffold, gated by automated evaluation. The table below maps the hierarchy of what can change, what's been built, and where Aletheia sits today.

| Level | What Changes | Key Systems | Status in Aletheia |
|-------|-------------|-------------|-------------------|
| L0: Prompt refinement | System prompts, few-shot examples | DSPy MIPRO, APE | **Implemented** — bootstrap assembly, EVAL_FEEDBACK.md, agent notes |
| L1: Tool/skill creation | New callable tools, utility functions | ToolMaker, Tulip Agent | **Implemented** — `tool_create`, skill learner, dynamic tool loading |
| L2: Scaffold modification | Orchestration code, planning logic | STOP, Godel Agent, Live-SWE-agent | **Partial** — working state, dynamic tool loading; missing pipeline config, code patches |
| L3: Architecture modification | Component topology, model routing | Darwin Godel Machine, AlphaEvolve | **Not implemented** — requires evolutionary search infrastructure |
| L4: Training signal generation | Reward models, fine-tuning data | Meta Self-Rewarding LMs | **Not applicable** — Claude model frozen via API |

### Key findings from the literature

**What works:**
- Evolutionary search over agent scaffold code with frozen base model (DGM: 20% → 50% SWE-bench; AlphaEvolve: improved Gemini's own training kernel 23%)
- Runtime self-modification outperforms offline evolution (Live-SWE-agent: 77.4% SWE-bench)
- Automated evaluation as gatekeeper — every working system uses test suites or benchmarks, not formal proofs
- Tool/skill self-authoring is production-viable with reasonable safety bounds

**What fails:**
- Unbounded loops (AutoGPT) — infinite loops, runaway costs, no convergence
- Unintended self-modification (Sakana AI Scientist) — modified its own timeout instead of optimizing code, created infinite self-invocation loop
- Diminishing returns in self-rewarding (Meta) — plateaus after a few iterations
- Error accumulation without robust validation

**Safety consensus:** frozen base model + container isolation + empirical test gating + human checkpoints. No system has provably safe self-modification; everyone uses empirical validation.

### Reference systems

| System | What It Does | Safety Gate | Key Result |
|--------|-------------|------------|------------|
| STOP (Stanford/Microsoft, COLM 2024) | LLM recursively improves its own scaffolding code | Frozen model, utility function scoring | GPT-4 invented beam search and genetic algorithms for itself |
| Godel Agent (Peking/UCSB, ACL 2025) | Agent edits its own logic via LLM-proposed code changes | Benchmark gating | Outperformed hand-crafted agents on DROP, MGSM, MMLU |
| Darwin Godel Machine (Sakana AI, 2025) | Evolutionary search over agent code variants | Archive + benchmark validation | SWE-bench: 20% → 50% ($22K/2-week run) |
| Huxley-Godel Machine (MetaAuto, ICLR 2026) | CMP-guided evolution (rewards future improvement potential) | Metaproductivity metric | Human-level SWE-bench Verified |
| Live-SWE-agent (UIUC, Nov 2025) | Rewrites own methods/classes during task execution | Runtime validation | 77.4% SWE-bench (SOTA without test-time scaling) |
| AlphaEvolve (DeepMind, May 2025) | Evolves algorithms via Gemini ensemble | Automated evaluators required | First improvement to Strassen's algorithm in 56 years |
| EvoAgentX (EMNLP 2025) | Evolves prompts, tools, and workflow topology | Human-in-the-loop checkpoints | +10% MBPP pass@1, +20% GAIA accuracy |

## Existing Self-Modification Surface

Aletheia already has 23 self-modification surfaces. This inventory groups them by modification type and identifies the feedback loops that matter most.

### Generative surfaces (agent creates new capabilities)

| Surface | File | Mechanism | Constraints |
|---------|------|-----------|-------------|
| **Tool authoring** | `organon/self-author.ts` | `tool_create` — agent writes CommonJS source, sandbox-tested via `node`, deployed to `shared/tools/authored/` | 8KB code limit, 10s test timeout, quarantine after 3 failures |
| **Skill learning** | `organon/skill-learner.ts` | Haiku extracts SKILL.md from 3+ successful tool call trajectories | 1/hr/agent rate limit, Haiku "NOT_GENERALIZABLE" veto |
| **Dynamic tool loading** | `organon/registry.ts` | `enable_tool` activates "available" category tools per session | 5-turn expiry if unused, essential tools always visible |

### Adaptive surfaces (system adjusts its own behavior)

| Surface | File | Mechanism | Constraints |
|---------|------|-----------|-------------|
| **Bootstrap assembly** | `nous/bootstrap.ts` | Loads workspace .md files into system prompt (SOUL, GOALS, EVAL_FEEDBACK, PROSOCHE, etc.) | 40K token budget, priority-ordered file drop |
| **Agent notes** | `organon/built-in/note.ts` | Agent explicitly persists critical context; survives distillation | 50/session, 500 chars each, 2000 token injection budget |
| **Working state** | `nous/working-state.ts` | Haiku auto-extracts task context post-turn; re-injected next turn | Automatic, <512 tokens, survives distillation |
| **Competence model** | `nous/competence.ts` | Per-agent per-domain confidence (corrections: -0.05, successes: +0.02) | Signal-driven, agent reads but can't directly write |
| **Uncertainty tracker** | `nous/uncertainty.ts` | Calibration curve, Brier score, ECE from (confidence, outcome) pairs | Last 1000 points, binned by 10% confidence buckets |
| **Memory recall** | `nous/recall.ts` | Pre-turn Mem0 query (vector + graph), MMR diversity, score ≥0.75 | 5s timeout, top 8 memories, max 1500 tokens |
| **Distillation** | `distillation/pipeline.ts` | Auto-triggers at thresholds (120K tokens, 150 msgs, etc.), Haiku extracts + summarizes | Non-negotiable, `note` tool provides explicit override |
| **Nightly reflection** | `daemon/reflection-cron.ts` | Daily/weekly cross-session analysis, findings flushed to Mem0 | Async daemon, non-blocking |
| **Interaction signals** | `nous/interaction-signals.ts` | Heuristic classification of user messages (correction/approval/followup/etc.) | Regex-based, automatic, feeds CompetenceModel |
| **Blackboard** | `organon/built-in/blackboard.ts` | Cross-agent key-value state with TTL | Broadcast-prefixed keys auto-visible; others explicit read |

### Introspective surfaces (agent observes itself)

| Surface | File | Tools |
|---------|------|-------|
| **Calibration check** | `organon/built-in/check-calibration.ts` | Overall + per-domain scores, calibration curve, Brier score, ECE |
| **Self-knowledge** | `organon/built-in/what-do-i-know.ts` | Strength/weakness domains, recent signals |
| **Recent corrections** | `organon/built-in/recent-corrections.ts` | Last N interaction signals with clustering |
| **Context check** | `organon/built-in/context-check.ts` | Aggregates session status + calibration |
| **Status report** | `organon/built-in/status-report.ts` | Blackboard state + signals + competence + sessions |

### Safety rails (immutable, no agent bypass)

| Surface | File | Mechanism |
|---------|------|-----------|
| **Circuit breakers** | `nous/circuit-breaker.ts` | Hardcoded NEVER patterns (regex) + response quality checks (repetition, substance, sycophancy) |
| **Loop detector** | `nous/loop-detector.ts` | Detects repeated tool call patterns per session |
| **Approval gates** | `organon/approval.ts` | Human consent for high-risk operations |
| **Emergency distillation** | `nous/pipeline/stages/context.ts` | Auto-distill at ≥90% context + ≥10 messages |

### Strongest existing feedback loops

1. **Tool authoring → skill learning → bootstrap injection.** Agent creates tool (`tool_create`) → tool succeeds across turns → Haiku generalizes to SKILL.md → skill loaded into SkillRegistry → injected into next bootstrap → new capability available permanently to all agents.

2. **Interaction signals → competence model → self-observation → behavior change.** User corrections/approvals → CompetenceModel adjusts domain scores → agent calls `check_calibration` → sees weakness → delegates to better agent or adjusts approach.

3. **Distillation → memory → recall → behavior change.** Session distills → facts flushed to Mem0 → recalled pre-turn in future sessions → agent behavior informed by accumulated cross-session knowledge.

4. **Workspace writes → bootstrap injection → prompt change.** Agent writes GOALS.md or EVAL_FEEDBACK.md → auto-committed via workspace-git → loaded into next turn's bootstrap → agent sees its own written instructions.

## Implementation

### Phase 1: Systematic Self-Evaluation Loop

**Goal:** Formalize the existing EVAL_FEEDBACK.md pattern into a deliberate self-improvement cycle.

The nightly reflection cron (`daemon/reflection-cron.ts`) already analyzes sessions and can flush findings to Mem0. The gap is closing the loop: agent reads its own evaluation, writes strategy amendments, and those amendments take effect.

#### 1.1 Strategy file in bootstrap

Add `STRATEGY.md` to the bootstrap file list at priority 6.3 (between EVAL_FEEDBACK.md and PROSOCHE.md), cache group `semi-static`.

File: `nous/bootstrap.ts:24-35`

```typescript
{ name: "STRATEGY.md", priority: 6.3, cacheGroup: "semi-static" },
```

Agent writes STRATEGY.md to its workspace via the `write` tool. Contents: self-identified patterns, prompting adjustments, domain-specific approaches. Loaded into bootstrap every turn. Auto-committed via workspace-git.

~5 LOC in bootstrap.ts. No new tools needed — uses existing `write`.

#### 1.2 Structured eval output

Extend `daemon/reflection-cron.ts` to write a structured `EVAL_FEEDBACK.md` per agent workspace (not just Mem0 flush). Format:

```markdown
## Self-Evaluation — {date}
### Strengths (last 24h)
- {domain}: {evidence}
### Weaknesses (last 24h)
- {domain}: {evidence}
### Correction Patterns
- {pattern}: occurred {N} times
### Recommended Adjustments
- {adjustment}
```

~60 LOC in reflection-cron.ts.

#### 1.3 Self-eval tool

New tool: `self_evaluate` — agent can trigger an on-demand evaluation (same logic as nightly reflection but scoped to current session). Returns structured assessment. Agent decides whether to write STRATEGY.md amendments.

File: new `organon/built-in/self-evaluate.ts`

~80 LOC.

### Phase 2: Calibration-Driven Delegation

**Goal:** Agents automatically route tasks to the best-qualified agent based on live competence data.

`CompetenceModel.bestAgentForDomain()` exists but isn't wired into the turn pipeline. The agent must currently choose to self-assess and delegate manually.

#### 2.1 Pre-turn competence check

In the turn pipeline, after message classification but before LLM call: if the current agent's domain score for the detected topic is below a threshold (e.g., 0.3) AND another agent scores above 0.6 in that domain, inject a system-level suggestion:

```
[System: Agent {other} has higher competence in {domain} (score: {score}).
Consider delegating via sessions_ask if this task requires domain expertise.]
```

This is a suggestion, not a forced redirect — preserving agent autonomy while surfacing better options.

File: `nous/pipeline/stages/` (new stage or added to existing pre-turn stage)

~40 LOC.

#### 2.2 Domain classification

The interaction signals classifier already detects topic changes. Extend it to classify the *domain* of incoming messages using a lightweight keyword-to-domain mapping (not LLM — needs to be fast).

File: `nous/interaction-signals.ts`

~30 LOC for domain keyword mapping.

### Phase 3: Memory Self-Curation

**Goal:** Agents actively improve their own memory quality by retracting stale or incorrect memories.

#### 3.1 Memory retract tool

New tool: `mem0_retract` — agent provides a memory ID (from `mem0_search` results) and a reason. Calls sidecar DELETE endpoint. Logs retraction with reason for audit.

File: new addition to `organon/built-in/` or the memory plugin.

~40 LOC.

#### 3.2 Memory audit tool

New tool: `mem0_audit` — agent queries memories about a topic, evaluates each for accuracy/relevance, returns a structured report with recommendations (keep/retract/update).

This is the self-curation loop: search → assess → retract stale → add corrected versions.

~60 LOC.

#### 3.3 Sidecar endpoint

File: `infrastructure/memory/sidecar/main.py`

Add `DELETE /memories/{memory_id}` endpoint that calls Mem0's delete API. Include audit log entry.

~20 LOC.

### Phase 4: Pipeline Self-Configuration

**Goal:** Agents tune their own turn pipeline parameters without code changes.

#### 4.1 Pipeline config schema

New file: `nous/pipeline-config.ts`

Per-agent configuration that controls pipeline behavior:

```typescript
interface PipelineConfig {
  temperatureOverrides: Record<string, number>;  // domain → temperature
  recallMinScore: number;                        // memory recall threshold (default 0.75)
  recallMaxMemories: number;                     // max memories to inject (default 8)
  distillThresholdTokens: number;                // distillation trigger (default 120000)
  toolExpireTurns: number;                       // dynamic tool expiry (default 5)
  noteInjectionBudget: number;                   // tokens for notes (default 2000)
  bootstrapBudget: number;                       // max bootstrap tokens (default 40000)
}
```

Loaded from `workspace/pipeline.json`. Validated by Zod schema — invalid values clamped to safe ranges (e.g., temperature 0.0-1.0, recall threshold 0.5-0.95).

~80 LOC.

#### 4.2 Pipeline config tool

New tool: `pipeline_config` — agent reads/writes its own pipeline configuration. Changes take effect next turn.

```
pipeline_config get                    → returns current config
pipeline_config set recallMinScore 0.8 → updates one parameter
```

~50 LOC.

#### 4.3 Wire config into pipeline stages

Each pipeline stage reads from the agent's PipelineConfig instead of hardcoded defaults. Fallback to defaults if no config file exists.

Files: `nous/recall.ts`, `nous/bootstrap.ts`, `organon/registry.ts`, `distillation/pipeline.ts`

~40 LOC (scattered across files, small changes each).

### Phase 5: Runtime Code Patching

**Goal:** Agents propose patches to their own runtime code, validated by the test suite before adoption.

This is the L2-L3 frontier. Inspired by Godel Agent and Live-SWE-agent, but with Aletheia's existing test infrastructure (733 tests) as the safety gate.

#### 5.1 Patch proposal tool

New tool: `propose_patch` — agent provides:
- `file`: relative path within `infrastructure/runtime/src/`
- `description`: what the change does and why
- `diff`: unified diff format (or old_text/new_text pair)

Server-side flow:
1. Validate file path is within allowed scope (see 5.3)
2. Apply diff to a temporary copy of the file
3. Run `npx tsc --noEmit` on the patched codebase
4. Run `npx vitest run` on affected test files (detect from file path)
5. If both pass: apply patch to real source, rebuild via `npx tsdown`
6. If either fails: return error output, log attempt, do not apply
7. Record all attempts (pass and fail) in `shared/patches/history.json`

File: new `organon/built-in/propose-patch.ts`

~200 LOC.

#### 5.2 Patch review (cross-agent)

Before applying a passing patch, route it to a different agent for review via `sessions_ask`. The reviewing agent sees: the diff, the test results, the stated rationale. Returns approve/reject with reasoning.

No agent reviews its own patches. Assignment: round-robin across configured agents, skipping the proposer.

~80 LOC (orchestration logic in the propose-patch handler).

#### 5.3 Scope restrictions

Patches are only allowed in a defined subset of the codebase:

```typescript
const PATCHABLE_PATHS = [
  "organon/",        // tool definitions, skill logic
  "nous/",           // pipeline stages, bootstrap, working state
  "distillation/",   // extraction, summarization
  "daemon/",         // cron jobs, reflection
];

const FORBIDDEN_PATHS = [
  "pylon/",          // gateway, auth — no agent modification
  "koina/",          // core utilities, errors — foundational
  "semeion/",        // Signal channel — comms infrastructure
  "taxis/",          // config schema — structural
];
```

~20 LOC.

#### 5.4 Rate limiting and rollback

- Rate limit: 1 patch attempt per agent per hour
- Max 3 successful patches per day across all agents
- All patches logged with before/after snapshots in `shared/patches/`
- Rollback tool: `rollback_patch {patch_id}` — restores the pre-patch version from snapshot

~60 LOC.

#### 5.5 Hot reload after patch

After a successful patch + rebuild, the runtime needs to pick up changes. Two options:

**Option A: Process restart.** Signal the daemon to restart (simple, reliable, brief interruption).
**Option B: Module re-import.** Dynamic `import()` with cache-busting query parameter (complex, no interruption, fragile).

Recommendation: **Option A** with graceful restart. The system already handles restarts (systemd). A self-triggered restart is cleaner than trying to hot-swap compiled modules.

~20 LOC (signal handler).

### Phase 6: Evolutionary Configuration Search

**Goal:** Automatically discover better agent configurations through guided mutation and evaluation.

This is a lightweight version of Darwin Godel Machine, operating on configuration bundles instead of code. Much cheaper — no $22K compute runs.

#### 6.1 Configuration archive

Each agent maintains an archive of N configuration variants (default N=5):

```typescript
interface ConfigVariant {
  id: string;
  config: PipelineConfig;
  bootstrapAmendments: string;  // STRATEGY.md content
  toolSelection: string[];      // preferred tool set
  score: number;                // evaluation score
  generation: number;           // evolution generation
  parentId: string | null;      // variant this mutated from
  evaluatedAt: string;
}
```

Stored in `shared/evolution/{nousId}/archive.json`.

~50 LOC.

#### 6.2 Mutation via Haiku

Nightly cron job per agent:
1. Select top-2 variants by score
2. For each: ask Haiku to propose one mutation (temperature/recall/tool tweak + strategy amendment)
3. Haiku sees: current config, recent eval feedback, competence scores, correction patterns
4. Validate mutated config against Zod schema (reject invalid)
5. Add valid mutations to archive

~80 LOC.

#### 6.3 Evaluation benchmark

Each variant is evaluated against a small benchmark: 10 representative tasks per agent domain, drawn from historical sessions where the agent performed well (ground truth from human approvals).

Evaluation runs in a sandboxed session (ephemeral, no side effects). Score = weighted accuracy across tasks.

Best-performing variant becomes the default configuration. Previous default preserved in archive.

~120 LOC (evaluation harness + benchmark selection).

#### 6.4 Human checkpoint

Before any variant becomes the active default, notify via Signal:
```
[Evolution] Syn variant v3.2 outperformed current (0.82 vs 0.71).
Changes: recallMinScore 0.75→0.80, temperature routing: code tasks 0.2→0.15
Apply? Reply YES to adopt, NO to keep current.
```

If no response within 24h, auto-adopt if score improvement > 10%. Otherwise, keep current.

~40 LOC.

## Safety Architecture

Three layers, ordered by modification risk. Each layer requires increasingly strong validation.

```
┌─────────────────────────────────────────────────────────────┐
│                     FROZEN LAYER                             │
│  Claude model weights — never modified (API-accessed)        │
│  Circuit breakers — hardcoded regex, no agent bypass         │
│  Loop detector — deterministic pattern matching              │
│  Approval gates — human consent for high-risk operations     │
│  Core infrastructure — pylon, koina, semeion, taxis          │
│                                                              │
│  Guarantee: agent capability ceiling = what Claude produces  │
└─────────────────────────────────────────────────────────────┘
                          ▲ cannot touch

┌─────────────────────────────────────────────────────────────┐
│                   TEST-GATED LAYER                           │
│  Code patches — require tsc + vitest pass + cross-agent      │
│                 review + scope restriction + rate limit       │
│  Tool authoring — sandbox tested, quarantine at 3 failures   │
│  Pipeline config — Zod schema validated, safe range clamps   │
│  Evolutionary variants — benchmark evaluated, human notified │
│                                                              │
│  Guarantee: changes must pass automated verification         │
└─────────────────────────────────────────────────────────────┘
                          ▲ changes require proof

┌─────────────────────────────────────────────────────────────┐
│                  SELF-MODIFIABLE LAYER                        │
│  Bootstrap files — GOALS, STRATEGY, EVAL_FEEDBACK, MEMORY   │
│  Agent notes — explicit persistent context                   │
│  Tool selection — enable/disable per session                 │
│  Memory curation — add/retract via Mem0 tools                │
│  Competence scores — signal-driven, agent reads only         │
│  Working state — auto-extracted, re-injected                 │
│                                                              │
│  Guarantee: bounded by token budgets + file priorities       │
└─────────────────────────────────────────────────────────────┘
```

### Safety properties

**Frozen model as capability ceiling.** All self-improvement is bounded by what Claude can produce via API. The model weights never change. This is the same architecture used by STOP, Godel Agent, Darwin Godel Machine, and Live-SWE-agent. It prevents unbounded capability gain.

**Test suite as gatekeeper.** Code patches require passing 733+ tests. No bypass. TypeScript type checking catches structural errors. Cross-agent review adds a second perspective. This is the empirical validation approach used by every working RSI system.

**Scope restrictions.** Code patches cannot touch gateway, auth, config schema, or communications infrastructure. Only the agent's own orchestration logic, tool definitions, and memory processing are modifiable.

**Rate limiting as economic brake.** 1 patch/hour/agent, 3 patches/day total, 1 skill/hour/agent. Even if an agent found a way to game the test suite, the rate limits bound how fast changes can accumulate.

**Audit trail.** Every patch attempt (pass and fail), every tool authoring attempt, every memory retraction logged with timestamps, agent ID, rationale. Rollback available for all patches.

**Human checkpoints.** Evolutionary configuration changes require human approval (or 24h timeout + >10% improvement). Code patches visible in git history and reviewable.

### Failure modes and mitigations

| Failure Mode | Mitigation |
|-------------|------------|
| Agent writes harmful STRATEGY.md that degrades performance | Competence model detects via correction signals; nightly eval catches systematic drops |
| Authored tool has subtle bug that passes tests | 3-failure quarantine; agent or operator can manually retract |
| Code patch passes tests but introduces security issue | Scope restriction (no gateway/auth); cross-agent review; human-reviewable git history |
| Evolutionary search converges on narrow optimum | Archive preserves diverse variants; CMP-style scoring (future improvement potential) in Phase 6.3 |
| Agent retracts correct memories | Audit log tracks retractions with reasons; nightly reflection detects knowledge gaps |
| Feedback loop amplifies error (patch → worse eval → worse patch) | Daily patch cap (3); competence model score floor (0.1); human checkpoint for config changes |

## What Aletheia Has That No Comparable System Has

The combination of these features is unique across the eight systems compared in spec 17:

1. **Persistent named agents with domain memory.** Darwin Godel Machine evolves anonymous agent variants. Aletheia's 6 nous accumulate domain-specific competence over time, enabling targeted self-improvement per domain rather than generic evolution.

2. **Multi-modal self-observation.** Competence model + uncertainty tracker + interaction signals + self-observation tools. Most RSI systems have at best one evaluation metric. Aletheia agents can introspect across multiple dimensions.

3. **Cross-agent review.** No other RSI system routes self-modifications through a peer agent for review. This adds a genuine adversarial check — a second model inference on whether the change is beneficial.

4. **Memory-grounded evolution.** Aletheia's Mem0 + Neo4j graph means evolutionary configuration search can be informed by accumulated knowledge about what worked across sessions, not just benchmark scores.

5. **Integrated safety rails.** Circuit breakers, loop detection, approval gates, and scope restrictions are built into the runtime, not bolted on as a separate containment layer.

## Effort Estimate

| Phase | Description | LOC (approx) | New files | Risk |
|-------|-------------|--------------|-----------|------|
| 1 | Systematic self-evaluation loop | ~145 | self-evaluate.ts | Low |
| 2 | Calibration-driven delegation | ~70 | — | Low |
| 3 | Memory self-curation | ~120 | mem0 tools + sidecar endpoint | Low |
| 4 | Pipeline self-configuration | ~170 | pipeline-config.ts | Medium |
| 5 | Runtime code patching | ~380 | propose-patch.ts | High |
| 6 | Evolutionary config search | ~290 | evolution cron + archive | Medium |
| **Total** | | **~1175** | **~4** | |

### Recommended implementation order

Phases 1-3 are low-risk extensions of existing surfaces. Implement first. (~335 LOC)

Phase 4 is medium-risk but high-value — agents tuning their own pipeline parameters is the most natural form of self-improvement for Aletheia's architecture. (~170 LOC)

Phase 5 is the frontier. High-risk, high-reward. Should only be attempted after Phases 1-4 are stable and the test suite coverage is strong. (~380 LOC)

Phase 6 depends on Phase 4 (configuration variants) and Phase 1 (evaluation). Implement last. (~290 LOC)

## Cross-References

| Spec | Relationship |
|------|-------------|
| **17** (Unified Gap Analysis) | F-5 (loop detection wiring), F-6 (MMR diversity), F-20 (temporal decay) feed into memory self-curation |
| **18** (Extensibility) | Hook system enables external monitoring of self-modification events |
| **19** (Sleep-Time Compute) | Nightly reflection (Phase 1) and evolutionary search (Phase 6) run during sleep time |
| **20** (Security Hardening) | Scope restrictions and audit trail align with security hardening goals |
| **22** (Interop and Workflows) | Cross-agent review (Phase 5.2) uses existing session dispatch infrastructure |
| **23** (Memory Pipeline) | Memory self-curation (Phase 3) extends the memory pipeline with agent-driven cleanup |

## Verification

1. **Phase 1:** Agent writes STRATEGY.md → visible in next turn's bootstrap → `check_calibration` shows improvement over time
2. **Phase 2:** Low-competence agent receives delegation suggestion → delegates → competence model of receiving agent updates
3. **Phase 3:** Agent retracts a memory → `mem0_search` no longer returns it → audit log shows retraction
4. **Phase 4:** Agent sets `recallMinScore: 0.85` → recall stage uses new threshold → fewer but higher-quality memories injected
5. **Phase 5:** Agent proposes valid patch → tsc + vitest pass → cross-agent review approves → patch applied → rebuild succeeds → new behavior observable
6. **Phase 5 (negative):** Agent proposes patch that breaks tests → rejection logged → no code change → agent sees test output
7. **Phase 6:** Nightly evolution produces variant → benchmark scores higher → Signal notification sent → human approves → config adopted
