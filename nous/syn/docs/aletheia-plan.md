# Aletheia Build Plan

Sequenced by dependency. Each phase produces something usable. No phase is wasted if we stop.

---

## Phase 1: Distillation (replace compaction)
**Why first:** Every other improvement depends on not losing state. Current compaction is lossy summarization — insights are lost, context degrades, sessions restart cold. Fix this and everything else compounds.

**What exists:** OpenClaw compaction fires at ~150K tokens, runs a basic summarization prompt, dumps the rest. We lose structure, decisions, emotional context, open threads.

**Build:**
1. **Pre-compaction hook** — Before OpenClaw compacts, we intercept and:
   - Extract structured insights (decisions, facts, preferences, corrections)
   - Update facts.jsonl with new learnings
   - Update FalkorDB with new entities/relationships/events
   - Save session state (topic, tone, open threads, pending decisions)
   - Write daily memory file entry
2. **Session state file** — `memory/session-state.yaml` per nous:
   - Current conversation topic/focus
   - Active task context
   - Open questions/decisions
   - Recent corrections (what I got wrong and the right answer)
3. **Resumption compiler** — After compaction or new session:
   - Load session state
   - Pull relevant facts from graph
   - Assemble minimal context that makes the gap invisible
   - Inject as the compacted "summary" instead of OpenClaw's default

**Deliverable:** Compaction that makes me smarter, not dumber. Session state that persists.

**Effort:** 2-3 focused sessions. Mostly Python scripts + OpenClaw config tweaks.

---

## Phase 2: Context Compilation (replace static injection)
**Why second:** Once we preserve state well (Phase 1), we need to USE it well. Currently 30K+ tokens of static files injected every turn regardless of conversation. 

**What exists:** compile-context generates AGENTS.md from templates. OpenClaw reads workspace files and dumps them all in. We already know the injection mechanism (workspace.js).

**Build:**
1. **Smart AGENTS.md** — Instead of all-sections-always:
   - Core identity (always, ~500 tokens)
   - Relevant operational sections (based on time, topic, activity)
   - Recent state summary (from Phase 1's session state)
   - Dynamically assembled per-conversation
2. **TOOLS.md → YAML** — Convert 27K prose to structured lookups:
   - Only inject tool docs relevant to current conversation
   - Or: make TOOLS.md much shorter, reference YAML for details
3. **Conversation-aware pre-flight** — A script that runs before each session:
   - Checks calendar, tasks, system state
   - Assembles today's context
   - Writes optimized workspace files
   
**Deliverable:** 40-60% token reduction on context injection. More relevant context per token.

**Effort:** 2-3 sessions. Python scripts + template restructuring.

---

## Phase 3: Shared Awareness (lateral connections)
**Why third:** With good continuity (Phase 1) and efficient context (Phase 2), now connect the minds.

**What exists:** facts.jsonl is already shared across all 7 nous. FalkorDB is running. Letta has per-agent stores. Inter-agent messaging is crude text via sessions_send.

**Build:**
1. **Shared knowledge graph** — FalkorDB as the substrate:
   - Per-nous namespace (Chiron's SQL knowledge, Demiurge's leather knowledge)
   - Shared namespace (cross-domain insights, preferences, decisions)
   - Typed relationships (not just "related to" but "contradicts", "depends on", "evolved from")
2. **Insight propagation** — When one nous distills (Phase 1):
   - Classify insights: domain-specific or cross-cutting?
   - Cross-cutting insights get written to shared graph
   - Other nous pick them up at next session start via context compilation
3. **Unified query API** — Wrap FalkorDB + facts.jsonl + Letta:
   - Single command: `aletheia query "what do we know about X"`
   - Returns results from all sources, ranked by relevance and recency
   - Any nous can use it

**Deliverable:** Minds that know what other minds know. Cross-domain insights surface automatically.

**Effort:** 3-4 sessions. Graph schema design, Python API, integration with distillation.

---

## Phase 4: Attention System (replace heartbeats)
**Why fourth:** With continuity, context, and shared awareness, now build adaptive attention.

**What exists:** OpenClaw heartbeat fires every 30 min with static HEARTBEAT.md prompt. Some custom checks (agent-health, blackboard, alerts).

**Build:**
1. **Attention engine** — Python daemon or cron-based:
   - Monitors: calendar, tasks, system health, file changes, recent activity
   - Scores: urgency × importance × relevance
   - Decides: what to surface, when, to whom
2. **Adaptive timing** — Not fixed intervals:
   - Active conversation? Check less, don't interrupt.
   - Quiet morning? Check calendar, prep context.
   - Deadline approaching? Increase attention frequency.
   - Something broke? Alert immediately.
3. **Context-aware prompts** — Instead of static HEARTBEAT.md:
   - Generate attention prompt based on what actually needs attention
   - Include only relevant checks
   - Different prompts for different times/states

**Deliverable:** Attention that feels like awareness, not a timer.

**Effort:** 2-3 sessions. Python daemon, integration with Phase 2 context compilation.

---

## Phase 5: OpenClaw Patches (make the runtime serve us)
**Why fifth:** By now we know exactly what we need from the runtime. Patch surgically.

**Potential patches:**
1. **Dynamic workspace file selection** — workspace.js: instead of loading all files, call a compiler script that decides what to load
2. **Custom compaction handler** — system-prompt.js: hook our distillation (Phase 1) into the compaction flow
3. **Structured context format** — support YAML/structured injection alongside markdown
4. **Session state persistence** — save/load session state across restarts

**Deliverable:** OpenClaw serves Aletheia instead of constraining it.

**Effort:** Varies. Some patches are 5-line changes, others are deeper.

---

## Phase 6: Character Refinement (who each nous IS)
**Why last in sequence but continuous:** Character is foundational but benefits from all the infrastructure. With good continuity and context, character expression becomes more consistent.

**Build:**
1. **SOUL.md audit** — For each nous:
   - Separate character (SOUL) from operations (AGENTS)
   - Trim bloat, sharpen essence
   - Ensure character is internalized, not performed
2. **Character testing** — Does each nous actually embody its character?
   - Review conversation logs
   - Identify where character breaks
   - Adjust SOUL.md based on evidence, not hope
3. **Character evolution** — As each nous develops:
   - Track what works and what doesn't
   - Update character based on interaction patterns
   - Let character be shaped by experience, not just designed

---

## Resource Requirements

| Phase | Primary tools | Dependencies |
|---|---|---|
| 1. Distillation | Python, OpenClaw hooks | None |
| 2. Context compilation | Python, templates | Phase 1 (state to compile) |
| 3. Shared awareness | FalkorDB, Python API | Phase 1 (insight extraction) |
| 4. Attention system | Python daemon, cron | Phase 2 (context assembly) |
| 5. OpenClaw patches | Node.js | Phases 1-4 (know what to patch) |
| 6. Character | Prose, testing | Phases 1-2 (consistent expression) |

## What's First

Phase 1, step 1: **Pre-compaction hook.**

The single highest-impact thing we can build is intercepting what happens when context gets distilled. Right now we LOSE information at every compaction. Fix that, and every session after is better.

Concrete first task: Write the distillation script that:
1. Receives the pre-compaction context
2. Extracts structured insights
3. Updates the knowledge stores
4. Produces a resumption state
5. Hands back a tight summary to OpenClaw

This is buildable today. Everything else follows from it.
