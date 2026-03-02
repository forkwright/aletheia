# Spec 45: Coworking Workspace -- Shared Operations Surface for Human + Agents

**Status:** Draft
**Author:** Syn
**Date:** 2026-03-01
**Spec:** 45

---

## Problem

The human-agent interaction model is serial messaging through a slot in a wall. Cody sends a message, waits, reads a response. No shared visible state. No mutual awareness. No ability to see what agents are thinking about, what's healthy, what's burning money, or what needs attention -- without explicitly asking.

This creates three concrete failures:

1. **Context reconstruction tax.** Every session starts with "what's the current state of X?" because neither party can see the other's workspace. The human reconstructs agent state from memory. The agent reconstructs human intent from messages.

2. **Invisible operations.** Prosoche is scoring signals. Daemon crons are running. Phases are executing. Cost is accumulating. Sessions are active. None of this is visible unless you ask or grep logs. Failures surface as symptoms ("why did that take so long?"), not as live indicators.

3. **No shared work surface.** When Cody adds a task, he types it in Signal. When an agent finishes work, it reports in chat. There is no single surface where both parties see pending work, check items off, and share awareness of what is done and what remains.

Signal is a communication *method* -- a pipe. What is missing is a communication *manner* -- a shared room where human and agents cowork with mutual visibility.

---

## Vision

A web-based operations workspace served by Pylon that replaces the current chat-only WebUI as the primary coworking surface. Signal remains the mobile/quick channel. The workspace is the deep-work channel.

The workspace is **bidirectional**. The human sees agent state. Agents see human decisions. Toggles take effect immediately. Tasks sync in both directions. Health is always visible. Cost is always visible. The room is always lit.

This is one human's personal operations center and coworking space -- not multi-tenant, not multi-user. One deployment, one operator.

### What This Is Not

- Not a kanban board (Spec 30's task board is a component, not the product)
- Not a monitoring-only dashboard (read-write, not read-only)
- Not a replacement for Signal (complement -- different interaction mode)
- Not a replacement for A2UI canvas (canvas is agent-writable surfaces within the workspace)

### Relationship to Existing Specs

| Spec | Relationship |
|------|-------------|
| 29 (UI Layout) | Workspace supersedes current layout. Agent bar, theme, responsive design carry forward. |
| 30 (Homepage Dashboard) | Absorbed. Task board and activity feed become workspace views. |
| 43b (A2UI Canvas) | Integrated. Canvas surfaces render within the workspace as agent-writable panels. |

---

## Design

### Principles

1. **Unconcealment over reporting.** State is always visible, not fetched on demand. If prosoche is scoring, you see it. If a phase is blocked, you see it. The workspace practices aletheia.

2. **Bidirectional by default.** Every display is also a control. Agent status is visible AND the model powering it is switchable. Tasks are visible AND checkable by either party. Health is visible AND services are restartable.

3. **Immediacy.** Toggles take effect now. No confirmation dialogs, no "are you sure?" The human is the operator. The workspace is a cockpit, not a wizard.

4. **Complementary to Signal.** Signal is the phone in your pocket. The workspace is the desk you sit at. Different postures, same system. A message sent via Signal appears in the workspace. A task created in the workspace is visible to agents in their next turn.

5. **Incremental assembly.** The workspace is a collection of views and panels, not a monolith. Each view can ship independently. The frame (navigation, SSE connection, auth) ships first, views fill in.

### Architecture

```
Browser                          Pylon (Axum)
┌─────────────────┐              ┌─────────────────────────┐
│                  │   SSE        │                         │
│  Workspace UI    │◄────────────│  /ws/events (SSE)       │
│  (Svelte 5)      │              │                         │
│                  │   REST       │  /api/workspace/*       │
│                  │────────────►│    tasks, toggles,      │
│                  │              │    health, config       │
│                  │              │                         │
│  Canvas panels   │◄────────────│  /api/canvas/* (Spec 43b)│
│                  │              │                         │
└─────────────────┘              └──────────┬──────────────┘
                                            │
                              ┌─────────────┼──────────────┐
                              │             │              │
                         NousActor     Prosoche       Daemon
                         sessions      signals        crons
                         turns         scores         evolution
                         tools         attention      distillation
```

**SSE as the spine.** A single SSE connection carries all live state: agent status changes, task updates, health heartbeats, cost ticks, phase transitions, canvas surface updates. The workspace subscribes once and routes events to the appropriate view.

**REST for mutations.** Toggle a model, create a task, restart a service, change a config value. POST/PATCH endpoints that take effect immediately and emit SSE events confirming the change.

**Pylon serves everything.** Static assets (Svelte build) + API routes + SSE stream. No separate dashboard server. The workspace IS the WebUI -- same origin, same auth (symbolon), same port.

---

## Views

### 1. Home -- The Room

The default view. What you see when you sit down at the desk.

```
┌──────────────────────────────────────────────────────────────┐
│  Aletheia  ●Syn Idle  ◉Demi Working  ●Syl Idle  ●Akron Idle │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Evening, Cody.                            Mar 1, 8:30 PM   │
│                                                              │
│  ┌─ Tasks ──────────────────────────────────────────────┐   │
│  │  ☐ Review pylon auth tests              → Syn        │   │
│  │  ☐ Respond to James re Q3               → Cody       │   │
│  │  ☐ Research dashboard projects           → Syn    ✓   │   │
│  │  ☑ Merge PRs #387-389                   → Syn        │   │
│  │  [+ Add task]                                        │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌─ Active Now ────────────────────────────────────────┐    │
│  │  Demi: Executing Phase 2 "Infrastructure" (3/7)     │    │
│  │  Prosoche: 3 signals scored, next cycle in 4m       │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌─ System ─────────────┬─ Cost (7d) ─────────────────┐    │
│  │  Gateway: ● healthy  │  Sonnet: $8.42    ▁▃▂▅▃▁▂   │    │
│  │  Signal:  ● healthy  │  Opus:   $4.10    ▁▁▃▁▁▁▅   │    │
│  │  Prosoche: ● active  │  Embed:  $0.00               │    │
│  │  CozoDB:  ● healthy  │  Total:  $12.52              │    │
│  └──────────────────────┴──────────────────────────────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Components:**
- **Task board** (from Spec 30, refined). Shared between human and agents. Either party adds, either checks off. Backed by SQLite via pylon API. Agents see their tasks injected into context.
- **Active now**. What is happening right this moment. Drawn from SSE events: active turns, running phases, prosoche cycles, daemon crons.
- **System health**. Heartbeat tiles for every subsystem. Green/yellow/red. Click to drill.
- **Cost summary**. 7-day rolling cost by model. Sparkline trends. Updated per-turn.

### 2. Agent Detail

Click an agent pill to see their world.

```
┌──────────────────────────────────────────────────────────────┐
│  ← Home    Syn                                     [Chat ↗]  │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Status: Idle          Model: [Sonnet 4 ▾]                   │
│  Last active: 2m ago   Sessions today: 7                     │
│  Tokens (24h): 48.2K   Cost (24h): $1.84                    │
│                                                              │
│  ┌─ Controls ──────────────────────────────────────────┐    │
│  │  Model:       [Sonnet 4 ▾]  (dropdown, immediate)  │    │
│  │  Autonomous:  [●━━━━○]  OFF                         │    │
│  │  Wake on signal: [━━━━●○]  ON                       │    │
│  │  Prosoche:    [━━━━●○]  ON   (cycle: 5m)            │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌─ Recent Sessions ──────────────────────────────────┐     │
│  │  #s_01J8... "PR merge and docs update"   42 turns   │     │
│  │  #s_01J7... "Prosoche dedup fix"         18 turns   │     │
│  │  #s_01J6... "Profile README"              8 turns   │     │
│  │  [View session replay →]                            │     │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌─ Attention (Prosoche) ─────────────────────────────┐     │
│  │  Signal: "3 overdue work tasks" — score: 0.72       │     │
│  │  Signal: "PR #385 open 2d" — score: 0.45            │     │
│  │  Signal: "Memory approaching 80%" — score: 0.31     │     │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Controls that write to taxis config (hot-reloaded via arc-swap):**
- **Model selector.** Dropdown: Sonnet / Opus / Haiku. Writes to agent's model config. Takes effect on next turn.
- **Autonomous mode toggle.** When ON, agent picks up open tasks and prosoche signals without being prompted. Writes to autonomy gradient config (Spec 39).
- **Wake on signal.** Whether prosoche wakes this agent. Toggle writes to daemon config.
- **Prosoche cycle.** Interval slider or preset. Writes to daemon cron config.

**Session replay** (inspired by AgentOps): Click a session to see a timeline of every turn, tool call, and decision. Scrub through it like a video. Data already exists in session store -- this is a read-only visualization.

**Prosoche attention panel.** Live view of what this agent's prosoche is scoring. Directly surfacing the signal pipeline that is currently invisible.

### 3. Projects + Phases

Dianoia execution state visualized.

```
┌──────────────────────────────────────────────────────────────┐
│  ← Home    Projects                                          │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Aletheia Issue Hackathon                                    │
│  State: executing                                            │
│                                                              │
│  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐    │
│  │Quick Win│──►│Infra &  │──►│CLI      │──►│A2UI     │    │
│  │  ✅     │   │Reliab.  │   │Found.   │   │Canvas   │    │
│  │ done    │   │ ◉ exec  │   │ ○ queue │   │ ○ queue │    │
│  └─────────┘   └────┬────┘   └─────────┘   └─────────┘    │
│                      │                                       │
│               ┌──────▼──────┐                                │
│               │ 3/7 plans   │                                │
│               │ ██████░░░░  │                                │
│               │             │                                │
│               │ ✅ lint fix │                                │
│               │ ✅ CI yaml  │                                │
│               │ ◉ test cov  │ ← Demi working                │
│               │ ○ error fmt │                                │
│               │ ○ dead code │                                │
│               │ ○ dep audit │                                │
│               │ ○ docs sync │                                │
│               └─────────────┘                                │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

Phase dependency graph. Click a phase to expand its plans. Click a plan to see the agent executing it. Verification status shown per phase. Checkpoint approvals can be given directly from this view.

### 4. Health + Services

Full system health board. Expansion of the home view's summary tiles.

```
┌──────────────────────────────────────────────────────────────┐
│  ← Home    System Health                                     │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─ Pylon Gateway ───────────────────────── ● Healthy ──┐   │
│  │  Port: 18789  │  Uptime: 14d 3h  │  Req/min: 12     │   │
│  │  Active SSE connections: 2                            │   │
│  │  [View logs]                                          │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌─ Signal Channel ──────────────────────── ● Healthy ──┐   │
│  │  signal-cli PID: 48291  │  Last msg: 3m ago          │   │
│  │  Pending sends: 0  │  Failed (24h): 0                │   │
│  │  [View logs]  [Restart]                               │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌─ Prosoche ────────────────────────────── ● Active ───┐   │
│  │  Cycle: 5m  │  Last run: 2m ago  │  Signals: 7       │   │
│  │  Wake budget: 1/2 used (1h window)                    │   │
│  │  Dedup window: 8h  │  Fingerprints: 3                 │   │
│  │  [View signals]  [Force cycle]  [Clear fingerprints]  │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌─ CozoDB ─────────────────────────────── ● Healthy ───┐   │
│  │  Relations: 42  │  Rows: 18.4K  │  Size: 28MB        │   │
│  │  Last compact: 6h ago  │  HNSW indices: 3             │   │
│  │  [Compact now]  [View stats]                          │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌─ Daemon Crons ────────────────────────── ● Active ───┐   │
│  │  Syn:  distill 6h ago ✓  │  evolve 23h ago ✓         │   │
│  │  Demi: distill 2h ago ✓  │  evolve 12h ago ✓         │   │
│  │  Syl:  distill 1d ago ✓  │  evolve 3d ago ⚠          │   │
│  │  [Run now ▾]                                          │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

Every tile is a subsystem. Each tile shows key metrics, last-known state, and action buttons. Actions hit pylon REST endpoints that talk to the relevant actor.

### 5. Cost Cockpit

Dedicated view for token economics.

- **Burn rate** per agent, per model, per phase, per day. Line charts.
- **Model comparison.** Same task class, Sonnet vs Opus cost and quality. Helps inform the model toggle decision with data.
- **Projections.** "At current rate, this month will cost $X."
- **Budget alerts.** Optional. Set a daily/weekly ceiling. Visual warning when approaching.
- **Per-session cost.** How much did that conversation cost? Drill into any session.

### 6. Session Replay

The AgentOps-inspired feature. A timeline view of a completed (or active) session.

```
Session #s_01J8K... "PR merge and docs update"
Agent: Syn  │  42 turns  │  Duration: 1h 14m  │  Cost: $3.22

Timeline:
├─ 0:00  System prompt assembled (12.4K tokens)
├─ 0:02  User: "Let's merge those PRs"
├─ 0:04  Tool: exec("gh pr list") → 3 open PRs
├─ 0:06  Tool: exec("gh pr view 387") → reviews docs
├─ 0:08  Thinking: "Need to check merge conflicts..."
├─ 0:12  Tool: exec("gh api .../merge") → merged #387
├─ ...
├─ 1:12  Response: "All three merged, branches cleaned up"
└─ 1:14  Session end (stop_reason: end_turn)
```

Scrub the timeline. Expand any turn to see full request/response. See thinking blocks. See tool inputs and outputs. See token counts per turn. This is the "video replay" of agent work.

### 7. Chat

The existing chat view, preserved. Click "Chat" from an agent detail page or from the agent bar. Full conversation interface. This is what Signal does in web form -- direct messaging with an agent.

### 8. Canvas (Spec 43b)

Agent-writable dynamic surfaces that appear as panels within any view. A Dianoia phase can push a progress surface. An agent can push a metrics surface. Canvas surfaces render inline or in a collapsible side panel.

---

## Wishlist -- Things We Want Even If Hard

These are ideas that may not make v1 but belong in the design space. They represent the ceiling of what this system could be.

### Shared Cursor / Presence

When Cody is looking at the Projects view, agents can see "operator is viewing Phase 2." When an agent is actively executing, the workspace shows which file or tool the agent is touching. Mutual awareness of attention.

### Voice Notes

Press-and-hold to record a voice note from the workspace. Transcribed and delivered to the agent as a message. The agent can respond with synthesized voice. Low-friction input when typing is too slow.

### Ambient Mode

A view designed to stay open on a secondary monitor. Minimal chrome, large type, auto-rotating between: active agent status, task board, cost ticker, phase progress. The workspace as a passive awareness surface -- glanceable, not interactive.

### Mobile Companion

Not a full mobile app -- a responsive web view optimized for phone. Check tasks, see health, toggle a model. Quick actions from the couch. The full workspace stays on the desktop.

### Notification Bridge

Workspace events (task completed, phase failed, health degraded) can optionally push to Signal or system notifications. The workspace doesn't require you to be watching it -- it reaches out when something needs attention.

### Replay Diffing

Compare two session replays side by side. Same task, different models. Same agent, before and after a prompt change. See where behavior diverged. Useful for evaluating model switches and prompt engineering.

### Agent Journaling

A read-only view of an agent's memory files, session notes, and MNEME entries. See how the agent's understanding of the world has evolved over time. The unconcealment of the agent's inner life.

### Plugin Marketplace View

If prostheke (WASM plugins) matures, a view for browsing, installing, and configuring plugins. Each plugin shows its granted capabilities, resource usage, and health.

---

## Implementation Strategy

### Phase 0: Frame

The skeleton that everything hangs on.

- Pylon serves static Svelte build at `/` (already does this)
- Single SSE endpoint `/ws/events` with typed event discriminator
- Symbolon auth for all workspace routes
- Navigation shell: Home, Agents, Projects, Health, Cost, Chat
- Agent bar (from Spec 29) as persistent top element

### Phase 1: Home + Tasks

The "sit down at the desk" experience.

- Task CRUD API (`/api/tasks`)
- Task store (SQLite, system-level -- not per-session)
- Home view: greeting, task board, active-now panel, system summary, cost summary
- Agent tools: `task_list`, `task_create`, `task_complete`
- SSE events: `task:created`, `task:updated`, `task:completed`

### Phase 2: Agent Detail + Controls

See and control each agent.

- Agent detail view with status, metrics, recent sessions
- Config toggles that write to taxis (model, autonomy, wake, prosoche)
- Hot-reload via arc-swap so toggles take effect without restart
- Prosoche attention panel (read-only view of scored signals)

### Phase 3: Health Board

Full system visibility.

- Health check endpoints for each subsystem
- Heartbeat protocol (each actor reports periodically)
- Health view with tiles, status indicators, action buttons
- Service actions: restart, force-cycle, compact, clear cache

### Phase 4: Projects + Phases

Dianoia visualization.

- Phase dependency graph renderer (SVG or Canvas)
- Plan-level detail with agent assignment and status
- Checkpoint approval from the UI
- Verification gap display

### Phase 5: Cost Cockpit

Token economics.

- Per-turn cost recording (already in session store)
- Aggregation queries by agent, model, time window
- Cost view with charts, projections, model comparison
- Budget alert configuration

### Phase 6: Session Replay

The crown jewel.

- Session timeline data structure (turns, tools, thinking, tokens)
- Timeline renderer with scrubbing
- Turn detail expansion (full request/response)
- Active session live-follow mode

### Phase 7: Canvas Integration

Spec 43b brought into the workspace.

- Canvas panel (collapsible sidebar or inline)
- Surface type renderers (progress, table, metrics, markdown, graph)
- Agent tool: `canvas_update`

---

## Open Questions

1. **Task backend: SQLite or CozoDB?** Tasks are simple CRUD. SQLite is proven (sessions already use it). CozoDB could unify storage but adds complexity for a flat table. Leaning SQLite.

2. **SSE event schema.** One mega-stream with typed discriminators, or multiple SSE endpoints per view? Single stream is simpler for the client. Multiple streams allow partial subscription. Need to decide.

3. **Config hot-reload granularity.** Can we reload a single agent's model without touching other config? arc-swap replaces the whole config atomically. May need per-agent config segments.

4. **Session replay storage.** Full turn data (including thinking blocks and tool I/O) is large. Store everything? Trim after N days? Separate replay store from session store?

5. **How does autonomous mode work?** When toggled ON, does the agent immediately scan for work? Or wait for next prosoche cycle? What triggers the first autonomous turn?

6. **Canvas panel placement.** Right sidebar (like VS Code panels), bottom panel (like a terminal), or floating/dockable? Probably right sidebar with collapse toggle.

7. **What is the gnomon name for this?** The workspace where truth is unconcealed. Where human and agents share a room. The seeing-place. Contenders: theatron (seeing-place), synoptikon (seeing-together), synergeion (working-together-place). Or something we have not yet discovered.

---

## Non-Goals

- Multi-user / multi-tenant (this is personal infrastructure)
- Mobile app (web-responsive is sufficient for v1)
- Replacing Signal (complementary, not competitive)
- Real-time collaborative editing (not Google Docs for agents)
- Custom dashboard builder / widget framework (views are designed, not assembled)

---

## References

- [Spec 29 -- UI Layout & Theming](29_ui-layout-and-theming.md)
- [Spec 30 -- Homepage Dashboard](30_homepage-dashboard.md)
- [Spec 43b -- A2UI Live Canvas](43_a2ui-canvas.md)
- [Spec 39 -- Autonomy Gradient](archive/DECISIONS.md) (absorbed)
- [AgentOps](https://github.com/AgentOps-AI/agentops) -- Session replay, cost tracking, agent graphs
- [Clawd Control](https://github.com/Temaki-AI/clawd-control) -- SSE live updates, fleet health, zero-dependency UI
- [mudrii/openclaw-dashboard](https://github.com/mudrii/openclaw-dashboard) -- SVG charts, cost trends, sub-agent trees
- [Lattice Workbench](https://latticeruntime.com/) -- Agent IDE + monitoring, enforcement primitives
