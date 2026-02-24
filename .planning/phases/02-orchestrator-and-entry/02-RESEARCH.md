# Phase 2: Orchestrator & Entry - Research

**Researched:** 2026-02-23
**Domain:** TypeScript runtime wiring — command registry, event bus hooks, CLI subcommands, working-state injection
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**First-contact behavior (/plan command)**
- When `/plan` fires and a project already exists for this nous: ask "Still working on [project goal]?" before resuming
- User confirms → resume from current state machine position
- User declines → soft-archive old project (state → `abandoned`), start fresh project
- If abandoned project contains context potentially relevant to a new project, note it in the new project's context at creation (researcher-role judgment call on what's worth noting)
- New project with no existing plan → immediately transition to `questioning`, brief framing message, first question

**Orchestrator first message**
- Minimal preamble: "Starting a Dianoia planning project. First: what are you building?"
- Do NOT explain the full flow (research → requirements → roadmap etc.) upfront — start questioning immediately
- Brief framing only (1 sentence max) before the first question

**Intent detection (turn:before hook)**
- When `turn:before` detects planning intent: inject one-line offer into the turn context
- Example: "This sounds like a planning task — want me to open a Dianoia project?"
- Non-blocking: user can decline and the turn proceeds normally
- Do NOT silently create a project without user confirmation
- Sensitivity: detect unambiguous planning intent (new project, complex multi-phase work), NOT simple tasks

**Session continuity and planning state presence**
- Planning context injected into agent working-state silently (no explicit session-open greeting)
- Aletheia's continuity model: the nous has persistent identity, the "session boundary" is invisible to the user
- "Soft presence" pattern: agent can naturally reference active planning context when contextually relevant (e.g., "by the way, you have a Dianoia project at Phase 3 — want to continue?")
- Not on every turn — only when the message or task is meaningfully related to the active project
- Working-state injection is the mechanism; the nous decides when to surface it

**Project archiving**
- Abandoned projects are soft-archived: state → `abandoned` (not deleted from SQLite)
- State machine already has `abandoned` as a terminal state (Phase 1 FSM)
- Archived projects remain queryable but do not surface in active context

### Claude's Discretion
- Exact intent detection prompt engineering (what constitutes "unambiguous planning intent")
- How "critical context from old project" is transferred to new project notes
- CLI subcommand flags beyond the basic invocation (Phase 2 scopes the subcommand itself)
- Exact working-state key name for planning context injection

### Deferred Ideas (OUT OF SCOPE)
- Multi-project support (user has multiple simultaneous Dianoia projects) — Phase 2 assumes one active project per nous; multi-project is a future concern
- "Archive all old projects" bulk command — out of scope for Phase 2
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ENTRY-01 | User can initiate planning via `/plan` slash command in any session | CommandRegistry.register() pattern verified; `/plan` matches same `!`/`/` prefix as all commands |
| ENTRY-02 | Agent detects planning intent in turn pipeline via `turn:before` hook and offers to engage planning mode | `turn:before` emitted in context.ts before API call; eventBus.on() is the registration mechanism |
| ENTRY-03 | Both entry paths (command and agent-detected) route to the same DianoiaOrchestrator state machine | Single orchestrator instance wired in aletheia.ts createRuntime() is the pattern |
| ENTRY-04 | Planning session is associated with the initiating nous and session, resumable from any later session with the same nous | nousId available at context stage; PlanningStore.listProjects(nousId) already exists |
| ENTRY-05 | `aletheia plan` CLI subcommand starts planning mode directly | commander.js pattern verified in entry.ts; new subcommand added same as `gateway`, `agent`, `cron` groups |
| TEST-03 | Unit tests for intent detection hook (true positive and false positive scenarios) | vitest run; pattern matches machine.test.ts (pure function, no mocks) |
</phase_requirements>

---

## Summary

Phase 2 wires the DianoiaOrchestrator into Aletheia's existing infrastructure: the command registry, event bus, CLI, and working-state injection. All four integration surfaces are well-understood from codebase inspection. No new libraries are needed.

The key architectural insight: `turn:before` is emitted in `context.ts` BEFORE the API call but AFTER nousId/sessionId are resolved. This makes intent detection possible — the hook has full routing context. However, shell-based YAML hooks (koina/hooks.ts) are fire-and-forget and cannot inject content into the turn. Intent detection must register directly on `eventBus` as a TypeScript handler, not via YAML.

The second key insight: `SessionStore.db` is private with no public accessor. Phase 2 must add a `getDb()` method to SessionStore before DianoiaOrchestrator can instantiate PlanningStore. This is the critical wiring step deferred from Phase 1.

**Primary recommendation:** DianoiaOrchestrator is a class wired in `createRuntime()` in `aletheia.ts` — it receives `store` and instantiates `PlanningStore` from `store.getDb()`. The `/plan` command, `turn:before` handler, and CLI subcommand all call `orchestrator.handle(nousId, sessionId)`.

---

## Standard Stack

### Core (no new dependencies)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `commander` | ^14.0.3 | CLI subcommand registration | Already used in entry.ts |
| `better-sqlite3` | ^12.6.2 | PlanningStore via SessionStore.getDb() | Phase 1 established this |
| `vitest` | ^4.0.18 | Unit tests for intent detection | Project standard |
| `eventBus` (internal) | existing | turn:before registration | Typed event bus, already in koina/ |
| `CommandRegistry` (internal) | existing | /plan slash command | semeion/commands.ts |

**Installation:** No new packages. Zero new dependencies.

### Registration Surfaces
| Surface | File | How to Register |
|---------|------|----------------|
| Slash command `/plan` | `semeion/commands.ts` | `registry.register({ name: "plan", ... })` in `createDefaultRegistry()` |
| turn:before hook | `koina/event-bus.ts` | `eventBus.on("turn:before", handler)` in `aletheia.ts` |
| CLI subcommand | `entry.ts` | `program.command("plan").description(...).action(...)` |
| Working-state inject | `nous/pipeline/stages/context.ts` | `systemPrompt.push(...)` — already the pattern |

---

## Architecture Patterns

### Recommended File Structure for Phase 2

```
infrastructure/runtime/src/dianoia/
├── index.ts           # (existing) — add exports for orchestrator
├── machine.ts         # (existing, unchanged)
├── store.ts           # (existing, unchanged)
├── schema.ts          # (existing, unchanged)
├── types.ts           # (existing, unchanged)
├── orchestrator.ts    # NEW — DianoiaOrchestrator class
└── intent.ts          # NEW — detectPlanningIntent() pure function + eventBus wiring
```

Wire in aletheia.ts `createRuntime()` after store creation, before gateway start.

### Pattern 1: DianoiaOrchestrator class

**What:** Single class that holds PlanningStore and drives all state transitions. All entry points call `orchestrator.handle(nousId, sessionId)`.

**When to use:** Every entry point (command, hook, CLI) calls this one method.

```typescript
// Source: Codebase — modeled on NousManager pattern
import { createLogger } from "../koina/logger.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import { eventBus } from "../koina/event-bus.js";
import type Database from "better-sqlite3";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const log = createLogger("dianoia:orchestrator");

export class DianoiaOrchestrator {
  private store: PlanningStore;

  constructor(db: Database.Database, private defaultConfig: PlanningConfigSchema) {
    this.store = new PlanningStore(db);
  }

  async handle(nousId: string, sessionId: string): Promise<string> {
    // 1. Check for existing active project for this nous
    const projects = this.store.listProjects(nousId);
    const active = projects.find((p) => p.state !== "abandoned" && p.state !== "complete");

    if (active) {
      // Return confirmation question — orchestrator does NOT resume automatically
      return `Still working on "${active.goal}"? (yes to resume, no to start fresh)`;
    }

    // 2. No active project — transition idle → questioning
    const project = this.store.createProject({
      nousId,
      sessionId,
      goal: "",  // goal filled during questioning phase
      config: this.defaultConfig,
    });

    this.store.updateProjectState(project.id, transition(project.state, "START_QUESTIONING"));
    eventBus.emit("planning:project-created", { projectId: project.id, nousId, sessionId });

    log.info(`Created planning project ${project.id} for ${nousId}`);
    return "Starting a Dianoia planning project. First: what are you building?";
  }

  async abandon(projectId: string): Promise<void> {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(project.id, transition(project.state, "ABANDON"));
    log.info(`Abandoned planning project ${projectId}`);
  }

  getActiveProject(nousId: string) {
    const projects = this.store.listProjects(nousId);
    return projects.find((p) => p.state !== "abandoned" && p.state !== "complete");
  }
}
```

### Pattern 2: `/plan` Slash Command Registration

**What:** Register in `createDefaultRegistry()` in `semeion/commands.ts`. The command handler calls `orchestrator.handle()`.

**Critical discovery:** `CommandRegistry.match()` handles BOTH `!` and `/` prefixes. Registering `name: "plan"` makes both `!plan` and `/plan` work automatically.

```typescript
// Source: Verified from semeion/commands.ts match() implementation
// Register in createDefaultRegistry() — receives store and later orchestrator
registry.register({
  name: "plan",
  description: "Start or resume a Dianoia planning project",
  async execute(_args, ctx) {
    // ctx.sessionId is set for WebUI turns; derive sessionKey from Signal sender otherwise
    const nousId = ctx.store.findSessionById(ctx.sessionId ?? "")?.nousId
      ?? ctx.config.agents.list[0]?.id ?? "syn";
    const sessionId = ctx.sessionId ?? ""; // orchestrator uses this for project association
    return orchestrator.handle(nousId, sessionId);
  },
});
```

**Problem:** `createDefaultRegistry()` currently doesn't receive `orchestrator`. Two options:
1. Pass orchestrator as a parameter to `createDefaultRegistry()` (preferred — matches existing pattern for `store`, `config`)
2. Register the plan command separately after orchestrator is created in `aletheia.ts`

Option 2 is simpler and avoids changing `createDefaultRegistry()` signature used in tests. Register via `commandRegistry.register(...)` in `aletheia.ts` after orchestrator is wired.

### Pattern 3: turn:before Intent Detection Hook

**What:** TypeScript handler registered on eventBus. Detects planning intent in message text and injects a one-line offer into the system prompt via a separate mechanism.

**Critical finding:** The `turn:before` event fires in `context.ts` at line 23:
```typescript
eventBus.emit("turn:before", { nousId, sessionId, sessionKey, channel: msg.channel });
```

This is AFTER nousId/sessionId resolve but BEFORE the system prompt is built. The event handler cannot modify the system prompt — it doesn't have access to it.

**The correct mechanism for working-state injection:** The handler must write to the session's `working_state` in SQLite via `store.updateWorkingState()`. The context stage then reads this in the NEXT turn (not the current turn). For immediate injection on the current turn, a different approach is needed.

**Two viable approaches for intent detection injection:**

**Option A — Next-turn injection (simple):** eventBus handler writes a flag to working_state. The NEXT turn's context stage reads it and the nous sees the offer. Slight delay but clean.

**Option B — Pre-turn system block (better UX):** Move intent detection logic into the context stage itself (context.ts) rather than as an eventBus handler. Context stage can push directly to `systemPrompt`. This requires modifying context.ts to query PlanningStore.

Recommendation: **Option B** — inject directly in context.ts after working state injection (line ~148). This ensures the offer appears in the SAME turn that triggered the intent.

```typescript
// In context.ts, after working state injection — Source: direct codebase inspection
// Requires orchestrator passed into RuntimeServices or resolved via module singleton
const activeProject = planningOrchestrator?.getActiveProject(nousId);
if (!activeProject && detectsPlanningIntent(msg.text)) {
  systemPrompt.push({
    type: "text",
    text: "## Planning Context\n\nThis sounds like a planning task — want me to open a Dianoia project? Reply 'yes' to start structured planning.",
  });
}
```

**Intent detection function (pure — easily testable):**

```typescript
// Source: design — pure function with no side effects
export function detectsPlanningIntent(text: string): boolean {
  // Detect unambiguous planning signals:
  // - "I want to build X" / "I'm building X"
  // - "help me plan" / "create a plan" / "planning project"
  // - "new project" / "start a project" / "design a system"
  // - complex multi-phase signals: "phases", "roadmap", "architecture", "requirements"
  // NOT: simple tasks, questions, debugging, file operations
  const planningPatterns = [
    /\b(plan|planning|roadmap|architecture|design)\b.*\b(project|system|app|service|feature)\b/i,
    /\b(build|create|develop|implement)\b.*\b(app|system|service|platform|tool)\b/i,
    /\bhelp\s+me\s+(plan|design|architect|build)\b/i,
    /\bnew\s+project\b/i,
    /\b(requirements|phases|milestones)\b/i,
    /\/plan\b/i,  // explicit command even in natural text
  ];
  return planningPatterns.some((p) => p.test(text));
}
```

### Pattern 4: `aletheia plan` CLI Subcommand

**What:** A new top-level command in `entry.ts` following the exact pattern of existing commands (`gateway`, `agent`, `cron`, `doctor`).

```typescript
// Source: Verified from entry.ts pattern for existing commands
program
  .command("plan")
  .description("Start or resume a Dianoia planning project")
  .option("-a, --agent <id>", "Agent ID to plan for")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .option("-t, --token <token>", "Auth token")
  .action(async (opts: { agent?: string; url: string; token?: string }) => {
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

      // CLI sends the /plan command via the gateway API
      const res = await fetch(`${opts.url}/api/sessions/send`, {
        method: "POST",
        headers,
        body: JSON.stringify({
          agentId: opts.agent ?? "syn",
          message: "/plan",
          sessionKey: "cli:plan",
        }),
        signal: AbortSignal.timeout(120000),
      });

      const data = await res.json() as Record<string, unknown>;
      if (!res.ok) {
        console.error(`Error: ${data["error"] ?? res.statusText}`);
        process.exit(1);
      }
      console.log(data["response"]);
    } catch (err) {
      console.error(`Failed: ${err instanceof Error ? err.message : err}`);
      process.exit(1);
    }
  });
```

### Pattern 5: Working-State Injection for Planning Context

**What:** Write active planning context to session's `working_state` so it persists across turns and survives distillation.

**How working-state works:** After each turn, `extractWorkingState()` in `nous/working-state.ts` uses a cheap model to extract structured context from the assistant response. This is stored in `sessions.working_state` JSON column. In `context.ts`, this is read and injected into the system prompt via `formatWorkingState()`.

**Planning context injection:** Write to working_state via `store.updateWorkingState()`. The format must match the `WorkingState` interface:

```typescript
// WorkingState interface from mneme/store.ts
interface WorkingState {
  currentTask: string;
  completedSteps: string[];
  nextSteps: string[];
  recentDecisions: string[];
  openFiles: string[];
  updatedAt: string;
}

// Planning context as working state — injected after project creation/resume
store.updateWorkingState(sessionId, {
  currentTask: `Dianoia planning: ${project.goal || "new project"} (state: ${project.state})`,
  completedSteps: [],
  nextSteps: ["Answer project questions to advance planning"],
  recentDecisions: [`Planning project ${project.id} active`],
  openFiles: [],
  updatedAt: new Date().toISOString(),
});
```

**Important caveat:** `extractWorkingState()` runs AFTER each turn and overwrites `working_state`. Planning context added this way will be overwritten unless the model's response references planning. The more durable mechanism is a dedicated `planning_context` system block injected in context.ts that reads directly from PlanningStore — this is the "soft presence" pattern. Working-state is the short-term mechanism; PlanningStore is the durable ground truth.

### Pattern 6: SessionStore.getDb() Accessor (Critical Gap)

**What:** `PlanningStore` needs a `Database.Database` instance. `SessionStore.db` is private with no accessor. Phase 2 must add `getDb()`.

```typescript
// Add to SessionStore in mneme/store.ts
getDb(): Database.Database {
  return this.db;
}
```

This is the wiring mechanism deferred from Phase 1. One line change, but it must be in Plan 02-01 before orchestrator can be instantiated.

### Anti-Patterns to Avoid

- **Using YAML shell hooks for intent detection:** Shell hooks are fire-and-forget, can't modify turn context, and spawn a process for each turn. Use TypeScript eventBus handlers instead.
- **Directly modifying context.ts's TurnState from eventBus handler:** The `turn:before` event payload is `{ nousId, sessionId, sessionKey, channel }` — it does NOT pass TurnState. Handlers cannot reach systemPrompt this way.
- **Registering intent detection as a VALID_EVENTS shell hook:** `koina/hooks.ts` validates against VALID_EVENTS set — `turn:before` IS in the set, but the shell hook mechanism is wrong for this use case.
- **Adding new EventName for planning:** The EventBus EventName union is in `koina/event-bus.ts`. Planning events (`planning:project-created` etc.) need to be added here AND to `VALID_EVENTS` in `koina/hooks.ts`. Both files must be updated.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Command prefix matching | Custom parser | `CommandRegistry.match()` | Already handles both `!` and `/`, lowercases, splits args |
| CLI argument parsing | Manual argv | `commander` (already wired) | Already used in entry.ts, handles errors and help text |
| Session/nousId resolution | Manual lookup | `resolveNousId()` from pipeline/stages/resolve.ts | Already handles channel routing, fallbacks |
| Working state persistence | Direct SQL | `store.updateWorkingState(sessionId, state)` | Already in SessionStore, handles JSON serialization |
| Event bus wiring | Manual pub/sub | `eventBus.on("turn:before", handler)` | Typed, teardown-safe, already exists |

**Key insight:** Every integration surface already exists. Phase 2 is wiring, not infrastructure. The patterns to follow are already in the codebase; copy them exactly.

---

## Common Pitfalls

### Pitfall 1: Intent Detection Can't Inject Into Current Turn via eventBus

**What goes wrong:** Handler registered with `eventBus.on("turn:before", ...)` fires too early — the system prompt hasn't been assembled yet. The handler can't push to `systemPrompt` because it doesn't have access to `TurnState`.

**Why it happens:** `turn:before` fires at line 23 of context.ts. `systemPrompt` is built starting at line 92. The event payload is `{ nousId, sessionId, sessionKey, channel }` — no systemPrompt reference.

**How to avoid:** Place intent detection logic directly inside `context.ts` (or pass orchestrator via `RuntimeServices`). Query the orchestrator after working-state injection (around line 150 in context.ts), push to `state.systemPrompt` directly.

**Warning signs:** Intent detection works but users don't see the offer until the next turn.

### Pitfall 2: planning: events not in EventName type

**What goes wrong:** `eventBus.emit("planning:project-created", ...)` fails TypeScript compilation because `planning:project-created` is not in the `EventName` union in `koina/event-bus.ts`.

**Why it happens:** EventName is a closed union type. `VALID_EVENTS` in `koina/hooks.ts` is a separate set (for YAML hook validation) that also needs updating.

**How to avoid:** Add planning events to BOTH `EventName` union in `event-bus.ts` AND `VALID_EVENTS` Set in `hooks.ts` in the same commit as any code that emits them.

**Warning signs:** `tsc --noEmit` fails with "Argument of type 'planning:...' is not assignable to parameter of type EventName".

### Pitfall 3: SessionStore.db is private — PlanningStore can't be instantiated

**What goes wrong:** `new PlanningStore(store.db)` fails because `db` is private on SessionStore.

**Why it happens:** Phase 1 deferred wiring explicitly: "PlanningStore receives pre-initialized db instance; wiring to SessionStore db deferred to Phase 2."

**How to avoid:** Add `getDb(): Database.Database { return this.db; }` to SessionStore as the FIRST task in Plan 02-01. All subsequent orchestrator code depends on this.

**Warning signs:** TypeScript error "Property 'db' is private".

### Pitfall 4: Command registration after commandRegistry is passed to setCommandsRef

**What goes wrong:** `/plan` command registered after `setCommandsRef(commandRegistry)` fires in `aletheia.ts` — the command shows in `commandRegistry` but Signal listener was set up before.

**Why it happens:** Signal listener receives `commands: commandRegistry` at startup. But `CommandRegistry` uses the same Map reference, so late registrations DO work (the Map is shared). This is actually fine — just be aware the signal listener gets a reference to the registry object, not a snapshot.

**How to avoid:** No special action needed. Register the plan command before `commandRegistry.register(...)` calls in `aletheia.ts` (or add it to `createDefaultRegistry()` with an `orchestrator` parameter). Either works.

### Pitfall 5: listProjects(nousId) returns ALL projects including completed/abandoned

**What goes wrong:** `orchestrator.handle()` calls `listProjects(nousId)` and finds a completed project from months ago, prompts "Still working on X?".

**Why it happens:** `listProjects()` returns all projects for a nous — no state filter.

**How to avoid:** Filter in orchestrator: `projects.find((p) => p.state !== "abandoned" && p.state !== "complete")`. Only states `idle`, `questioning`, `researching`, `requirements`, `roadmap`, `phase-planning`, `executing`, `verifying`, `blocked` count as "active".

---

## Code Examples

Verified patterns from actual codebase:

### Adding a new command to the default registry
```typescript
// Source: semeion/commands.ts createDefaultRegistry() — verified
registry.register({
  name: "plan",
  description: "Start or resume a Dianoia planning project",
  async execute(_args, ctx) {
    // ctx has: sender, sessionId, store, config, manager, client, target, watchdog, skills
    return "Planning logic here";
  },
});
```

### Registering on eventBus (TypeScript, not YAML)
```typescript
// Source: aletheia.ts — eventBus.on() pattern for lifecycle events
eventBus.on("turn:before", (payload) => {
  const nousId = payload["nousId"] as string;
  const sessionId = payload["sessionId"] as string;
  // fire-and-forget or sync — async errors are caught by EventBus
});
```

### Adding a CLI subcommand in entry.ts
```typescript
// Source: entry.ts — pattern for all existing subcommands (doctor, fork, status, send)
program
  .command("plan")
  .description("Start or resume a Dianoia planning project")
  .option("-a, --agent <id>", "Agent ID")
  .option("-u, --url <url>", "Gateway URL", "http://localhost:18789")
  .action(async (opts: { agent?: string; url: string }) => {
    // implementation
  });
```

### Injecting a system block from context stage
```typescript
// Source: context.ts — multiple examples of systemPrompt.push(...)
systemPrompt.push({
  type: "text",
  text: "## Planning Context\n\n[planning state here]",
  // no cache_control — this is dynamic content
});
```

### updateWorkingState pattern
```typescript
// Source: mneme/store.ts updateWorkingState() — verified signature
services.store.updateWorkingState(sessionId, {
  currentTask: "Dianoia planning active",
  completedSteps: [],
  nextSteps: ["Answer project questions"],
  recentDecisions: [],
  openFiles: [],
  updatedAt: new Date().toISOString(),
});
```

### CommandRegistry.match() behavior (for test authors)
```typescript
// Source: semeion/commands.ts match() — verified
// Both "!plan" and "/plan" match name "plan"
// "!plan resume" → handler gets args = "resume"
// "/Plan" → matched case-insensitively via cmd.toLowerCase()
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| plan_create / plan_propose tools | DianoiaOrchestrator (Phase 2) | Phase 2 | Legacy tools stay (INTG-05: deprecated but not removed) |
| YAML shell hooks for behavior | TypeScript eventBus handlers | Already established | Intent detection must be TypeScript, not YAML |

**Deprecated/outdated:**
- `plan_create` and `plan_propose` tools: remain in codebase, should be marked deprecated in their descriptions. Not removed in Phase 2.

---

## Open Questions

1. **How does context.ts receive the orchestrator reference?**
   - What we know: `RuntimeServices` interface is the dependency injection container for pipeline stages. It currently has `config`, `store`, `router`, `tools`, `plugins`, `watchdog`, `competence`, `uncertainty`, `skillsSection`, `approvalGate`, `approvalMode`, `memoryTarget`.
   - What's unclear: Adding `planningOrchestrator?: DianoiaOrchestrator` to `RuntimeServices` requires modifying the type and wiring it through `NousManager` which assembles `RuntimeServices` internally.
   - Recommendation: Add `planningOrchestrator?: DianoiaOrchestrator` to `RuntimeServices` interface. Wire it in `aletheia.ts` via a new `setPlanningOrchestrator(orchestrator)` method on `NousManager` (same pattern as `setPlugins`, `setWatchdog`, etc.).

2. **Where does the resume-confirmation flow live?**
   - What we know: Orchestrator returns a string from `handle()`. The command handler returns this string as the command response.
   - What's unclear: The "yes/no" response from the user to the confirmation question is the NEXT user message. The orchestrator needs to detect this as a resume or abandon signal. This is stateful — the user's next message is a regular chat message, not a command.
   - Recommendation: Orchestrator stores a `pendingConfirmation` flag in the project's `config` JSON column. The `turn:before` handler checks for pending confirmation and interprets the next message as a yes/no answer. Phase 3 (questioning flow) will handle conversation state more fully; Phase 2 only needs to handle the single yes/no confirmation.

3. **Intent detection placement — eventBus vs context.ts**
   - What we know: eventBus handler can't inject into current-turn systemPrompt; only into next turn via working_state.
   - What's unclear: Whether adding orchestrator to RuntimeServices creates circular dependencies.
   - Recommendation: Proceed with RuntimeServices extension. The dependency graph is `aletheia.ts → DianoiaOrchestrator → PlanningStore` and `context.ts → RuntimeServices.planningOrchestrator`. No circular dependency.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | vitest 4.0.18 |
| Config file | `infrastructure/runtime/vitest.config.ts` (standard) |
| Quick run command | `cd /home/ckickertz/summus/aletheia/infrastructure/runtime && npx vitest run src/dianoia/` |
| Full suite command | `cd /home/ckickertz/summus/aletheia/infrastructure/runtime && npx vitest run` |
| Estimated runtime | ~10-15 seconds for dianoia suite |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ENTRY-01 | /plan command registered and returns orchestrator response | unit | `npx vitest run src/dianoia/orchestrator.test.ts -x` | ❌ Wave 0 gap |
| ENTRY-02 | Intent detection fires on turn:before | unit | `npx vitest run src/dianoia/intent.test.ts -x` | ❌ Wave 0 gap |
| ENTRY-03 | Both command and intent detection call same orchestrator | unit | `npx vitest run src/dianoia/orchestrator.test.ts -x` | ❌ Wave 0 gap |
| ENTRY-04 | Project persisted with nousId; resume finds it | unit | `npx vitest run src/dianoia/orchestrator.test.ts -x` | ❌ Wave 0 gap |
| ENTRY-05 | CLI subcommand exists in entry.ts program | manual/integration | manual: `aletheia plan --help` | ❌ manual only |
| TEST-03 | Intent detection true positives and false positives | unit | `npx vitest run src/dianoia/intent.test.ts -x` | ❌ Wave 0 gap |

### Nyquist Sampling Rate
- **Minimum sample interval:** After every committed task → run: `cd /home/ckickertz/summus/aletheia/infrastructure/runtime && npx vitest run src/dianoia/`
- **Full suite trigger:** Before merging final task of any plan wave
- **Phase-complete gate:** Full suite green before `/gsd:verify-work` runs
- **Estimated feedback latency per task:** ~10 seconds

### Wave 0 Gaps (must be created before implementation)
- [ ] `infrastructure/runtime/src/dianoia/orchestrator.test.ts` — covers ENTRY-01, ENTRY-03, ENTRY-04
- [ ] `infrastructure/runtime/src/dianoia/intent.test.ts` — covers ENTRY-02, TEST-03 (pure function tests, no mocks needed)

*(ENTRY-05 CLI subcommand is manual-only verification — no automated test needed in Wave 0)*

---

## Sources

### Primary (HIGH confidence)
- Direct codebase inspection — `semeion/commands.ts` CommandRegistry, match(), register()
- Direct codebase inspection — `koina/event-bus.ts` EventBus, EventName, emit()
- Direct codebase inspection — `koina/hooks.ts` YAML hooks — confirmed NOT usable for intent injection
- Direct codebase inspection — `nous/pipeline/stages/context.ts` — turn:before emission point, systemPrompt assembly
- Direct codebase inspection — `nous/pipeline/stages/resolve.ts` — nousId/sessionId resolution order
- Direct codebase inspection — `mneme/store.ts` — SessionStore.db private field, WorkingState interface, updateWorkingState()
- Direct codebase inspection — `entry.ts` — commander.js CLI subcommand pattern
- Direct codebase inspection — `aletheia.ts` — createRuntime() wiring pattern, RuntimeServices assembly
- Direct codebase inspection — `dianoia/store.ts` — listProjects(nousId), PlanningStore API
- Direct codebase inspection — `dianoia/machine.ts` — FSM events, VALID_TRANSITIONS

### Secondary (MEDIUM confidence)
- Phase 1 summaries (01-01, 01-02, 01-03) — confirmed what was built
- CONTEXT.md Phase 2 decisions — locked user requirements

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries verified in actual codebase files
- Architecture patterns: HIGH — all patterns verified against actual source code with line references
- Pitfalls: HIGH — based on direct code inspection (private db field, EventName type, turn:before timing)
- Intent detection: MEDIUM — prompt engineering strategy is discretionary, actual patterns need iteration

**Research date:** 2026-02-23
**Valid until:** 2026-03-23 (stable runtime codebase; changes would come from active development)
