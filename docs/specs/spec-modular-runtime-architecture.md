# Spec: Modular Runtime Architecture

**Status:** Approved  
**Author:** Syn  
**Reviewer:** Cody  
**Date:** 2026-02-18  
**Updated:** 2026-02-19 — Decisions locked on buildMessages, config layout, hot-reload scope  

---

## Problem

`manager.ts` is 1,446 lines and growing. It contains two near-identical turn execution paths (streaming and non-streaming), hardwired feature composition (recall, loop detection, circuit breakers, tracing, skill learning, competence tracking, distillation), and a constructor that takes four dependencies with six more injected via setters. Every new feature requires touching the same file. The streaming and non-streaming paths drift apart with each change.

More broadly: the runtime is a monolith that happens to have good internal module boundaries. A user who wants to change behavior — disable recall, swap the loop detector strategy, add a pre-turn hook, change how distillation triggers — has to either edit TypeScript or hope the existing config surface covers their need. It usually doesn't.

The config schema (`taxis/schema.ts`) covers deployment topology well (agents, bindings, channels, gateway) but barely touches turn execution behavior. The turn pipeline is implicit in code, not declarative in config.

## Goals

1. **One config change propagates everywhere.** A user edits `config.yaml` to disable memory recall → the turn pipeline skips it. No code changes, no restart (ideally).
2. **Manager decomposition.** Extract the turn pipeline into composable stages. Kill the streaming/non-streaming duplication.
3. **Agent-level overrides.** Different agents should be able to run different pipeline configurations. Eiron might skip recall. A utility spawned agent might skip tracing entirely.
4. **Hot reload where safe.** Config changes that don't affect wiring (thresholds, feature flags) apply without restart. Structural changes (new agents, new bindings) require restart.

## Non-Goals

- Plugin API redesign (the existing `prostheke` system handles extension points)
- UI changes (this is runtime-only)
- Changing the external API surface (`/api/sessions/stream`, Signal listener)

---

## Decisions

These were discussed and agreed on 2026-02-19:

### D1: `buildMessages` is a utility, not a pipeline stage

`buildMessages` (~150 lines) does format conversion: translating stored `Message[]` into API-shaped `MessageParam[]`, handling media blocks, repairing orphaned tool_use, merging consecutive user messages, and injecting ephemeral timestamps. None of this is pipeline *behavior* — it's structural plumbing. It doesn't need configuration, toggling, or per-agent variation.

**Implementation:** Lives at `src/nous/pipeline/utils/build-messages.ts`. The `history` stage imports and calls it. If ephemeral timestamps ever need a toggle, the utility takes an options bag — not a pipeline config key.

### D2: Single `config.yaml` with per-agent sparse overrides

All human-authored configuration lives in one file:

```yaml
agents:
  defaults:
    pipeline:           # ← shared baseline for all agents
      recall:
        enabled: true
        # ...
  list:
    - id: eiron
      pipeline:         # ← sparse override, only what differs
        recall:
          enabled: false
```

**Resolution order (4 layers):**

```
hardcoded fallback
  → agents.defaults.pipeline.*
    → agents.list[id].pipeline.*
      → nous/<id>/config-overrides.yaml  (machine-written, from config_write tool)
```

Layer 4 (overlay files) exists only for agent self-configuration via the `config_write` tool. Humans edit `config.yaml`. Agents write overlay files. Clean separation. `aletheia config show <agent>` resolves the full chain and shows provenance per field.

**No per-agent config files for human use.** One file to look at, one file to validate, one file to version control.

### D3: Conservative hot-reload — feature flags and thresholds only (Phase 3.0)

**Safe to hot-reload (takes effect next turn):**
- Feature flags: enable/disable recall, tracing, skill learning, signal classification, competence tracking
- Thresholds: loop detection params, recall timeout/minScore, circuit breaker sensitivity
- Tool timeouts (defaultMs + per-tool overrides)

**Safe with side effects — Phase 3.1 (model changes):**
- Model swap: takes effect next turn, history format is model-agnostic, no issue. **But** if the new model has a smaller context window, the history stage truncates more aggressively — the agent may lose context. Log a warning: `"Model changed for {nousId}: effective context reduced from {old} to {new} tokens."`
- `contextTokens` change: same truncation behavior. Additionally, if the new value puts the current history above the distillation threshold, distillation triggers on the next turn. This is correct behavior but surprising — log it.

**Not hot-reloadable (requires restart):**
- New agents / removed agents
- Binding changes (channel → agent mapping)
- Gateway config (port, auth, CORS)
- Tool registry changes
- Store / database config
- Channel config (new Signal numbers, webhook URLs)

**Mechanism:** Extend existing SIGUSR1 handler. No API endpoint in Phase 3.0 — an HTTP config reload endpoint is effectively `POST /reconfigure-the-runtime`, which is security-critical surface that needs auth and rate-limiting. SIGUSR1 requires local access. Start there, widen later if needed.

---

## Design

### 1. Turn Pipeline as Composable Stages

The current turn execution is a monolithic async generator. Decompose it into a pipeline of discrete stages, each with a clear interface:

```
InboundMessage
  → [resolve]      Route to nous, resolve session, select model
  → [guard]        Circuit breakers, depth limits, drain check
  → [context]      Bootstrap assembly, recall, broadcasts, working state
  → [history]      Budget calculation, history retrieval, message building
  → [execute]      LLM call + tool loop (the inner loop)
  → [finalize]     Trace persistence, signal classification, skill extraction, distillation check
  → TurnOutcome
```

**Stage ordering is static.** Users cannot reorder or inject custom stages. The existing `prostheke` plugin system handles custom hooks (pre-turn, post-turn, pre-tool, etc.). Stages are structural; plugins are behavioral extensions.

Each stage is a function with a typed input/output contract. The pipeline runner composes them. Streaming and non-streaming share the same stages — the only difference is whether `execute` yields events or buffers them.

```typescript
// pipeline/types.ts

export interface TurnState {
  // Accumulated through stages
  msg: InboundMessage;
  nousId: string;
  sessionId: string;
  sessionKey: string;
  model: string;
  nous: NousConfig;
  workspace: string;
  temperature?: number;

  // Built by context/history stages
  systemPrompt: SystemBlock[];
  messages: MessageParam[];
  toolDefs: ToolDefinition[];
  trace: TraceBuilder;

  // Set by execute stage
  outcome?: TurnOutcome;
}

export interface PipelineStage {
  name: string;
  /** Return false to short-circuit (e.g., circuit breaker refusal) */
  execute(state: TurnState, ctx: PipelineContext): Promise<TurnState | false>;
}

export interface StreamingStage {
  name: string;
  execute(state: TurnState, ctx: PipelineContext): AsyncGenerator<TurnStreamEvent, TurnState>;
}
```

The `execute` stage (the LLM call + tool loop) is the only `StreamingStage`. All other stages are synchronous transforms on `TurnState`. This eliminates the duplication: the pre- and post-execute stages are identical regardless of streaming mode.

**The tool loop is not decomposed.** The inner tool execution loop (call tool → check loop detector → accumulate results → next LLM call) stays as a single unit inside `execute.ts`. It's the most complex piece (~300 lines) but decomposing it into a sub-pipeline would be over-engineering. The loop detector and tool timeout configs are read from pipeline config; that's sufficient control.

### 2. Feature Flags in Config

Add a `pipeline` section to agent defaults and allow per-agent sparse overrides:

```yaml
agents:
  defaults:
    pipeline:
      recall:
        enabled: true
        timeoutMs: 3000
        minScore: 0.75
        maxTokens: 1500
      loopDetection:
        enabled: true
        windowSize: 12
        warnThreshold: 3
        haltThreshold: 5
        consecutiveErrorThreshold: 4
      circuitBreakers:
        enabled: true
        inputChecks: true
        responseQuality: true
      tracing:
        enabled: true
        persistToWorkspace: true
      skillLearning:
        enabled: true
        minToolCalls: 3
      competenceTracking:
        enabled: true
      signalClassification:
        enabled: true
      workingStateInjection:
        enabled: true
        intervalTurns: 8
      broadcastInjection:
        enabled: true
        maxBroadcasts: 5
      distillation:
        auto: true               # trigger automatically when threshold hit
        thresholdShare: 0.7       # fraction of context window
        minMessages: 10
        workspaceFlush: true      # write to memory/YYYY-MM-DD.md
      toolTimeouts:
        defaultMs: 120000
        overrides:
          exec: 0                 # no timeout (long-running commands)
          sessions_ask: 0         # no timeout (waiting for agent response)
          browser: 180000         # 3 minutes

  list:
    - id: syn
      # inherits all defaults
    - id: eiron
      pipeline:
        recall:
          enabled: false
        skillLearning:
          enabled: false
    - id: utility-spawn
      pipeline:
        recall:
          enabled: false
        tracing:
          enabled: false
        competenceTracking:
          enabled: false
        signalClassification:
          enabled: false
        skillLearning:
          enabled: false
        distillation:
          auto: false
```

Resolution order: `agent.pipeline.X` → `defaults.pipeline.X` → hardcoded fallback → `nous/<id>/config-overrides.yaml` (machine overlay, highest priority).

Existing configs with no `pipeline` key behave identically — all hardcoded defaults match current behavior. Zero breaking changes.

### 3. Pipeline Assembly

The manager no longer contains turn logic. It becomes a thin orchestrator:

```typescript
// manager.ts (after refactor — ~200 lines)

export class NousManager {
  private pipelineCache = new Map<string, Pipeline>();

  constructor(
    private config: AletheiaConfig,
    private store: SessionStore,
    private router: ProviderRouter,
    private tools: ToolRegistry,
    private services: RuntimeServices,
  ) {}

  async *handleMessageStreaming(msg: InboundMessage): AsyncGenerator<TurnStreamEvent> {
    const pipeline = this.resolvePipeline(msg);
    yield* pipeline.executeStreaming(msg);
  }

  async handleMessage(msg: InboundMessage): Promise<TurnOutcome> {
    const pipeline = this.resolvePipeline(msg);
    return pipeline.execute(msg);
  }

  /** Called by hot-reload to force pipeline re-assembly with new config */
  clearPipelineCache(): void {
    this.pipelineCache.clear();
  }

  private resolvePipeline(msg: InboundMessage): Pipeline {
    const nousId = this.resolveNousId(msg);
    if (!this.pipelineCache.has(nousId)) {
      const pipelineConfig = this.resolvePipelineConfig(nousId);
      this.pipelineCache.set(nousId, buildPipeline(pipelineConfig, this.services));
    }
    return this.pipelineCache.get(nousId)!;
  }

  private resolvePipelineConfig(nousId: string): ResolvedPipelineConfig {
    // Layer 1: hardcoded defaults (in Zod schema .default() values)
    // Layer 2: agents.defaults.pipeline
    // Layer 3: agents.list[nousId].pipeline
    // Layer 4: nous/<nousId>/config-overrides.yaml (if exists)
    // Deep merge, later layers win
  }
}
```

### 4. RuntimeServices Bundle

Replace the six `setX()` methods with a single services object:

```typescript
export interface RuntimeServices {
  config: AletheiaConfig;
  store: SessionStore;
  router: ProviderRouter;
  tools: ToolRegistry;
  plugins?: PluginRegistry;
  watchdog?: Watchdog;
  competence?: CompetenceModel;
  uncertainty?: UncertaintyTracker;
  skillsSection?: string;
}
```

This is passed to pipeline stages via `PipelineContext`. Stages pull what they need; they don't import the world.

### 5. File Structure

```
src/nous/pipeline/
  types.ts              # TurnState, PipelineStage, PipelineContext, StreamingStage
  runner.ts             # Pipeline composition and execution (streaming + non-streaming)
  build.ts              # Pipeline factory: config → stage list
  stages/
    resolve.ts          # Nous routing, session creation, model selection
    guard.ts            # Circuit breakers, depth limits, drain check
    context.ts          # Bootstrap assembly, recall, broadcasts, working state
    history.ts          # Budget calc, history retrieval → calls buildMessages utility
    execute.ts          # LLM streaming + tool loop (the core, ~300 lines)
    finalize.ts         # Trace, signals, skills, distillation trigger
  utils/
    build-messages.ts   # Message[] → MessageParam[] (format conversion, orphan repair,
                        #   media handling, ephemeral timestamps, consecutive-user merge)
```

The `execute.ts` stage contains the tool loop — still the most complex piece, but now isolated. It receives fully-prepared `systemPrompt`, `messages`, and `toolDefs` and just runs the LLM interaction.

### 6. Hot Reload

#### Phase 3.0: Feature flags and thresholds

Extend the existing SIGUSR1 handler:

```typescript
// taxis/reload.ts

export interface ReloadResult {
  applied: boolean;
  requiresRestart: boolean;
  reason?: string;
  changes: ConfigChange[];
  warnings: string[];
}

export function reloadConfig(
  runtime: AletheiaRuntime,
  newConfig: AletheiaConfig,
): ReloadResult {
  const diff = diffConfig(runtime.config, newConfig);
  const warnings: string[] = [];

  // Structural changes → reject, require restart
  if (diff.structural) {
    return {
      applied: false,
      requiresRestart: true,
      reason: diff.structuralReason,
      changes: diff.changes,
      warnings,
    };
  }

  // Apply safe changes
  runtime.config = newConfig;
  runtime.manager.clearPipelineCache();

  return { applied: true, requiresRestart: false, changes: diff.changes, warnings };
}

/**
 * Categorize config paths as structural (restart required) or safe (hot-reloadable).
 *
 * Structural paths:
 *   - agents.list (add/remove agents)
 *   - agents.list[].bindings
 *   - channels.*
 *   - gateway.*
 *   - store.*
 *   - tools.registry (adding/removing tools)
 *
 * Safe paths (Phase 3.0):
 *   - agents.defaults.pipeline.*
 *   - agents.list[].pipeline.*
 *   - agents.defaults.branding.*
 *   - agents.list[].branding.*
 *   - logging.*
 *
 * Safe with warnings (Phase 3.1):
 *   - agents.defaults.model
 *   - agents.list[].model
 *   - agents.defaults.contextTokens
 *   - agents.list[].contextTokens
 */
```

#### Phase 3.1: Model changes (additive)

Same mechanism, but model/contextTokens changes emit warnings:

```typescript
if (diff.modelChanges.length > 0) {
  for (const change of diff.modelChanges) {
    const oldCtx = resolveContextTokens(runtime.config, change.nousId);
    const newCtx = resolveContextTokens(newConfig, change.nousId);
    if (newCtx < oldCtx) {
      warnings.push(
        `Model changed for ${change.nousId}: effective context reduced from ${oldCtx} to ${newCtx} tokens. ` +
        `History will be truncated on next turn.`
      );
    }
  }
}

if (diff.contextTokensChanges.length > 0) {
  for (const change of diff.contextTokensChanges) {
    // Check if new threshold would trigger immediate distillation
    const session = runtime.store.getActiveSession(change.nousId);
    if (session) {
      const currentTokens = estimateSessionTokens(session);
      const newThreshold = change.newValue * runtime.config.agents.defaults.pipeline.distillation.thresholdShare;
      if (currentTokens > newThreshold) {
        warnings.push(
          `contextTokens reduced for ${change.nousId}: current session (${currentTokens} tokens) ` +
          `exceeds new distillation threshold (${newThreshold}). Distillation will trigger on next turn.`
        );
      }
    }
  }
}
```

**No API endpoint in Phase 3.** SIGUSR1 only. HTTP endpoint deferred until auth/rate-limiting infrastructure exists.

### 7. Config Validation & CLI

The Zod schema already validates structure. Additions:

**Schema documentation:** Add `.describe()` to every pipeline field in the Zod schema:

```typescript
recall: z.object({
  enabled: z.boolean().default(true).describe("Enable pre-turn memory recall from Mem0 sidecar"),
  timeoutMs: z.number().default(3000).describe("Max milliseconds to wait for memory recall response"),
  minScore: z.number().default(0.75).describe("Minimum similarity score for recall results (0-1)"),
  maxTokens: z.number().default(1500).describe("Maximum tokens of recall context to inject"),
}),
```

**CLI commands:**

- **`aletheia config show <agent>`** — Dumps fully-resolved effective config for an agent, showing all 4 layers merged. Annotates each field with its source (`default`, `agent`, `overlay`, `hardcoded`).
- **`aletheia doctor --verbose`** — Existing doctor check, extended to show per-agent pipeline config and flag any anomalies (e.g., agent with recall enabled but no Mem0 sidecar configured).

### 8. Agent Self-Configuration

Agents can modify their own pipeline config within bounds:

```typescript
// New tool: config_write (scoped to own agent)
{
  name: "config_write",
  description: "Update your own pipeline configuration. Changes persist across sessions.",
  parameters: {
    path: "pipeline.recall.enabled",  // dot-notation, scoped to pipeline.*
    value: false,
  }
}
```

**Guardrails:**
- Agents can only modify their own `pipeline.*` — no cross-agent changes
- Cannot modify bindings, channels, gateway, other agents, or any structural config
- Changes written to `nous/<id>/config-overrides.yaml` (machine-managed, not human-edited)
- `config_write` triggers hot reload for the calling agent's pipeline cache entry only
- Overlay file is a simple flat YAML merged at the highest priority

**Example overlay file (`nous/eiron/config-overrides.yaml`):**

```yaml
# Machine-generated by config_write tool. Do not edit manually.
# To reset: delete this file and send SIGUSR1 to the runtime.
pipeline:
  recall:
    enabled: false
    minScore: 0.85
```

---

## Migration Path

This is a refactor, not a rewrite. The external API doesn't change. Migration is internal:

### Phase 1: Extract Pipeline Stages (no config changes)
1. Create `src/nous/pipeline/` directory with `types.ts`, `runner.ts`, `build.ts`
2. Create `src/nous/pipeline/stages/` with `resolve.ts`, `guard.ts`, `context.ts`, `history.ts`, `execute.ts`, `finalize.ts`
3. Create `src/nous/pipeline/utils/build-messages.ts` — extract from manager
4. Extract each stage from `manager.ts` into its corresponding file
5. Build `runner.ts` that composes stages for both streaming and non-streaming
6. Manager delegates to runner — becomes ~200 lines
7. Delete the non-streaming `executeTurn` entirely — the runner handles both modes
8. **Tests:** Existing manager tests still pass (black-box). No behavior change.

### Phase 2: Pipeline Config
1. Add `pipeline` section to Zod schema (`taxis/schema.ts`) with all defaults
2. `buildPipeline()` reads config to decide which stages run and with what params
3. Per-agent overrides work via deep merge
4. Add `resolvePipelineConfig()` with 4-layer resolution
5. **Backward compatible:** Existing configs with no `pipeline` key get all defaults = current behavior

### Phase 3.0: Hot Reload — Feature Flags & Thresholds
1. Implement `diffConfig()` with structural vs safe categorization
2. Extend SIGUSR1 handler to call `reloadConfig()`
3. Pipeline cache invalidation on reload
4. Logging: what changed, what was applied, what requires restart

### Phase 3.1: Hot Reload — Model Changes
1. Add model/contextTokens to safe-with-warnings category
2. Implement context budget warnings
3. Implement distillation threshold warnings

### Phase 3.2: Agent Self-Configuration
1. Implement `config_write` tool with path validation
2. Overlay file read/write (`nous/<id>/config-overrides.yaml`)
3. Per-agent pipeline cache invalidation (not full cache clear)

### Phase 4: CLI & Documentation
1. `aletheia config show <agent>` with provenance annotations
2. `aletheia doctor --verbose` with per-agent pipeline display
3. Schema-driven config reference generation from `.describe()` annotations

---

## Metrics

Success looks like:

- `manager.ts` drops from 1,446 lines to <250 (orchestration only)
- No stage file exceeds 300 lines
- `build-messages.ts` is a pure utility with no pipeline config dependencies
- Adding a new pre-turn or post-turn feature requires creating one file and adding one config key — zero changes to existing stage files
- Existing test suite passes without modification after Phase 1
- A user can disable memory recall for one agent by adding 3 lines to `config.yaml`
- Hot reload applies feature flag changes without restart or session disruption
- `aletheia config show <agent>` shows exactly where every setting comes from
