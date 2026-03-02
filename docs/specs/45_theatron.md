# Spec 45: Theatron -- Composable Operations System for Human + Agents

**Status:** Draft
**Author:** Syn
**Date:** 2026-03-01
**Revised:** 2026-03-01
**Spec:** 45

---

## Name

**Theatron** (θέατρον) -- the seeing-place. From θεάομαι (theaomai): to gaze upon, to behold, to contemplate. In antiquity, the theatron was not the stage -- it was the structure that made seeing possible. The seats, the acoustics, the sightlines. The Greeks did not build a fixed performance; they built a place where any performance could be seen.

| Layer | Reading |
|-------|---------|
| L1 | The operations interface -- dashboards, controls, live state, agent views |
| L2 | The connective surface between human and agents: shared visibility, mutual awareness |
| L3 | A seeing-place -- not a thing seen but the structure that makes seeing possible. As the ancient theatron shaped what could be witnessed, this system shapes what can be known about the agents and their work |
| L4 | The system itself is a theatron: it doesn't contain truth, it arranges the conditions for unconcealment. Every widget, every view, every data binding is a sightline cast into the system's inner life |

The theatron composes naturally with the existing topology. Pylon (the gate) serves it. Prosoche (attention) feeds signals into it. Dianoia (reasoning) renders its phase graphs within it. Nous (mind) streams its state through it. The theatron doesn't think, plan, or act -- it *makes thinking, planning, and acting visible*.

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

A web-based composable operations system served by Pylon. Not a dashboard with fixed views -- a **view composition engine** with good defaults. The operator and agents compose, customize, and share views built from a registry of typed widgets bound to live data sources.

Signal remains the mobile/quick channel. Theatron is the deep-work channel.

The system is **bidirectional**. The human sees agent state. Agents see human decisions. Toggles take effect immediately. Tasks sync in both directions. Health is always visible. Cost is always visible. The room is always lit.

The system is **nous-aware**. Views can be typed to a specific nous, a group of nous, or shared across all. An agent's dashboard reflects its strengths, domain, and personality. Syn's view emphasizes orchestration and PR status. Demi's view emphasizes test coverage and code health. The operator composes views per-agent, per-team, or system-wide. Agents themselves can author views as work products.

This is one human's personal operations center and coworking space -- not multi-tenant, not multi-user. One deployment, one operator.

### What This Is Not

- Not a kanban board (Spec 30's task board is a component, not the product)
- Not a monitoring-only dashboard (read-write, not read-only)
- Not a replacement for Signal (complement -- different interaction mode)
- Not a replacement for A2UI canvas (canvas is agent-writable surfaces within the workspace)

### Relationship to Existing Specs

| Spec | Relationship |
|------|-------------|
| 29 (UI Layout) | Theatron supersedes current layout. Agent bar, theme, responsive design carry forward. |
| 30 (Homepage Dashboard) | Absorbed. Task board and activity feed become default widgets. |
| 43b (A2UI Canvas) | Integrated. Canvas surfaces render within theatron as agent-writable panels. |
| 41 (Observability) | Feeds theatron. Tracing spans, structured logs, and metrics are data sources for widgets. |

---

## Design

### Principles

1. **Unconcealment over reporting.** State is always visible, not fetched on demand. If prosoche is scoring, you see it. If a phase is blocked, you see it. The theatron practices aletheia.

2. **Bidirectional by default.** Every display is also a control. Agent status is visible AND the model powering it is switchable. Tasks are visible AND checkable by either party. Health is visible AND services are restartable.

3. **Immediacy.** Toggles take effect now. No confirmation dialogs, no "are you sure?" The human is the operator. The theatron is a cockpit, not a wizard.

4. **Complementary to Signal.** Signal is the phone in your pocket. The theatron is the desk you sit at. Different postures, same system. A message sent via Signal appears in the theatron. A task created in the theatron is visible to agents in their next turn.

5. **Composition over construction.** The theatron is not a monolith of designed views. It is a system of typed widgets, data sources, layout grids, and view definitions. Default views ship as starter templates. Every view is forkable, rearrangeable, and customizable. Agents and humans alike can compose views.

6. **Nous-aware by default.** Every view has a scope: global (all agents), group (a named set of agents), or individual (one nous). Widgets inherit their view's scope unless overridden. This means the same widget type -- say, "recent sessions" -- automatically shows the right data depending on whether you're in a shared dashboard or Demi's personal view.

### Core Architecture

```
Browser                          Pylon (Axum)
┌─────────────────┐              ┌─────────────────────────┐
│                  │   SSE        │                         │
│  Theatron UI     │◄────────────│  /ws/events (SSE)       │
│  (Svelte 5)      │              │                         │
│                  │   REST       │  /api/theatron/*        │
│  Widget Engine   │────────────►│    views, widgets,      │
│  Layout Renderer │              │    tasks, toggles,      │
│  View Store      │              │    health, config       │
│                  │              │                         │
│  Canvas panels   │◄────────────│  /api/canvas/* (43b)    │
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

**SSE as the spine.** A single SSE connection carries all live state: agent status changes, task updates, health heartbeats, cost ticks, phase transitions, canvas surface updates. The theatron subscribes once and routes events to the appropriate widgets.

**REST for mutations.** Toggle a model, create a task, restart a service, change a config value, save a view definition. POST/PATCH endpoints that take effect immediately and emit SSE events confirming the change.

**Pylon serves everything.** Static assets (Svelte build) + API routes + SSE stream. No separate dashboard server. The theatron IS the WebUI -- same origin, same auth (symbolon), same port.

### View Composition Engine

The beating heart of Theatron. Instead of hardcoded views, the system composes views from four primitives:

#### 1. Widget Registry

Every UI element is a self-contained, typed widget. Each widget declares:

- **Type identifier.** `health-tile`, `cost-sparkline`, `task-list`, `agent-status`, `session-timeline`, `context-window-bar`, `phase-graph`, `prosoche-signals`, etc.
- **Data source binding.** What feeds the widget. Can be one or more of:
  - `sse:<event-type>` -- live from the SSE stream (e.g., `sse:agent.status`)
  - `rest:<endpoint>` -- polled REST endpoint (e.g., `rest:/api/health`)
  - `query:cozo:<relation>` -- direct CozoDB query with parameters
  - `file:<glob>` -- watched file content (e.g., `file:memory/*.md`)
  - `derived:<widget-id>` -- computed from another widget's output
- **Config schema.** What the widget accepts: time range, agent filter, refresh interval, display mode, color theme.
- **Size constraints.** Min/max grid cells (width x height). Whether the widget can be collapsed.
- **Scope compatibility.** Which view scopes the widget supports: `global`, `group`, `nous`, or `any`.

The registry mirrors the organon pattern (tool registry): same philosophy -- register typed capabilities, compose dynamically -- different domain.

#### 2. View Definitions

A view is a declarative document (YAML or JSON) describing:

```yaml
# Example: Syn's personal dashboard
name: "Syn Overview"
scope:
  type: nous
  nous_id: syn
layout:
  columns: 12
  rows: auto
widgets:
  - type: agent-status
    position: { col: 1, row: 1, width: 12, height: 1 }
    config:
      show_controls: true

  - type: task-list
    position: { col: 1, row: 2, width: 6, height: 4 }
    source: { rest: "/api/tasks?assignee=syn" }
    config:
      show_completed: false

  - type: recent-sessions
    position: { col: 7, row: 2, width: 6, height: 4 }
    config:
      limit: 5

  - type: prosoche-signals
    position: { col: 1, row: 6, width: 6, height: 3 }
    config:
      min_score: 0.3

  - type: cost-sparkline
    position: { col: 7, row: 6, width: 6, height: 3 }
    config:
      window: 7d
      group_by: model
```

View definitions are stored in taxis config, hot-reloadable via arc-swap. The 7 default views (Home, Agent Detail, Projects, Health, Cost, Replay, Chat) ship as built-in configs. The operator customizes by forking them or building from scratch.

#### 3. Nous-Typed Views

Views have a scope that determines whose data they show and who they're relevant to:

| Scope | Sees | Example |
|-------|------|---------|
| `global` | All agents, system-wide data | Home dashboard, Health board, Cost cockpit |
| `group:<name>` | Named agent group | "Infrastructure team" (Demi + Akron), "Orchestration" (Syn + Arbor) |
| `nous:<id>` | Single agent | Syn's personal dashboard, Demi's test coverage view |

**Inheritance rules:**
- Widgets within a `nous:syn` view automatically filter data to Syn unless overridden.
- A `group:infra` view shows aggregated data for all group members.
- `global` views show everything, with optional per-agent drill-down.

**Agent groups** are named sets defined in taxis config:

```yaml
theatron:
  groups:
    infrastructure:
      name: "Infrastructure"
      members: [demi, akron]
      icon: "wrench"
    orchestration:
      name: "Orchestration"
      members: [syn, arbor]
      icon: "network"
```

Groups reflect operational reality: which agents collaborate on what. The theatron navigation shows groups as collapsible sections. Click "Infrastructure" to see the group dashboard. Click "Demi" within it to see Demi's personal view.

**Agent personality in views.** A nous-scoped view can reflect the agent's character. Syn's dashboard might emphasize PR status, merge queues, and cross-agent coordination. Demi's might emphasize test coverage, lint warnings, and code health metrics. The operator configures this; the agent can suggest or author view definitions as work products.

#### 4. Agent-Authored Views

Agents don't just appear in the theatron -- they build parts of it. An agent can compose a view definition as a work output:

- Demi finishes a test coverage expansion. It authors a "Coverage Report" view: test count over time chart, coverage by crate table, failing test list, and a phase progress widget. Saves it via REST API.
- Syn completes a PR review sprint. It authors a "Review Summary" view: merged PRs table, remaining open PRs, code churn sparkline. Posts it as a session artifact.

Agent-authored views are tagged with their creator and appear in a "Views by Agent" section of the navigation. The operator can adopt, fork, or discard them. This makes the theatron a collaborative artifact -- not just a tool the operator uses to watch agents, but a surface that agents actively contribute to.

**View authoring API:**

```
POST /api/theatron/views
{
  "name": "Coverage Report",
  "scope": { "type": "nous", "nous_id": "demi" },
  "author": "demi",
  "layout": { ... },
  "widgets": [ ... ]
}
```

#### 5. Data Source Abstraction

Widgets don't know where data comes from. They bind to typed data sources:

| Source Type | Protocol | Use Case |
|-------------|----------|----------|
| `sse:<event>` | Server-Sent Events | Live agent status, task changes, health heartbeats, phase transitions |
| `rest:<path>` | HTTP GET (polled) | Task lists, session history, cost summaries, config values |
| `query:cozo` | CozoDB query | Knowledge graph exploration, memory facts, relation statistics |
| `file:<glob>` | Filesystem watch | Memory files, session notes, prosoche state |
| `derived:<id>` | Computed | Aggregations, filters, and transforms of other sources |

This abstraction means custom widgets can surface *anything* the system knows. Want a view showing all CozoDB relations with >1000 rows? Bind a table widget to a Cozo query. Want prosoche scores over time? Bind a chart to the scoring history. Want a live grep of agent session notes? Bind a text widget to a file glob.

### Layout Engine

Views render on a 12-column responsive grid (matching CSS grid conventions). Widgets are placed by column/row position and span. The layout engine handles:

- **Drag-and-drop rearrangement.** Grab a widget, drop it elsewhere. Grid snaps.
- **Resize handles.** Drag edges to resize within the widget's declared min/max constraints.
- **Responsive breakpoints.** On narrow screens, columns collapse. Widgets reflow based on priority.
- **Collapse/expand.** Any widget can be collapsed to a title bar. Remembers state.
- **Save layout.** Changes persist to the view definition in taxis. Hot-reloaded.

Inspiration: Homarr's tile placement, but with typed data bindings and nous-scoping.

---

## Default Views

The theatron ships with 7 default view templates. These are the "starter kit" -- opinionated compositions of built-in widgets that cover the most common needs. Every one of them is forkable and customizable.

### 1. Home -- The Room

The default view. What you see when you sit down at the desk. Scope: `global`.

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

**Widgets:** agent-bar, greeting, task-list (global), active-now, health-summary, cost-sparkline (7d, by model).

### 2. Agent Detail

Nous-scoped view. See and control a specific agent. Scope: `nous:<id>`.

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
│  │  Signal: "3 overdue work tasks" -- score: 0.72      │     │
│  │  Signal: "PR #385 open 2d" -- score: 0.45           │     │
│  │  Signal: "Memory approaching 80%" -- score: 0.31    │     │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Widgets:** agent-status-header, agent-controls, recent-sessions, prosoche-signals, task-list (nous-scoped), cost-sparkline (nous-scoped).

**Controls write to taxis config (hot-reloaded via arc-swap):**
- **Model selector.** Dropdown: Sonnet / Opus / Haiku. Writes to agent's model config. Takes effect on next turn.
- **Autonomous mode toggle.** When ON, agent picks up open tasks and prosoche signals without being prompted. Writes to autonomy gradient config (Spec 39).
- **Wake on signal.** Whether prosoche wakes this agent. Toggle writes to daemon config.
- **Prosoche cycle.** Interval slider or preset. Writes to daemon cron config.

### 3. Projects + Phases

Dianoia execution state visualized. Scope: `global`.

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

**Widgets:** phase-graph (SVG/Canvas dependency renderer), plan-list (per-phase), plan-detail (agent assignment, status). Checkpoint approvals can be given directly from this view.

### 4. Health + Services

Full system health board. Scope: `global`.

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

**Widgets:** health-tile (per subsystem), daemon-cron-status, log-stream (filtered). Every tile has action buttons that hit pylon REST endpoints.

### 5. Cost Cockpit

Dedicated view for token economics. Scope: `global` with per-agent drill-down.

**Widgets:**
- **Burn rate.** Per agent, per model, per phase, per day. Line charts.
- **Model comparison.** Same task class, Sonnet vs Opus cost and quality. Data for informing model toggle decisions.
- **Projections.** "At current rate, this month will cost $X."
- **Budget alerts.** Optional ceiling per day/week. Visual warning when approaching.
- **Per-session cost.** Click any session to see its cost breakdown by turn.

### 6. Session Replay

AgentOps-inspired timeline view. Scope: `nous:<id>` (one session at a time).

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

**Widgets:** session-timeline (scrubbing), turn-detail (expand for full request/response), context-window-bar (token budget per turn), tool-call-detail. Active sessions support live-follow mode.

### 7. Chat

The existing pylon chat interface, now one view among many rather than the only interface. Scope: `nous:<id>`.

**Widgets:** chat-history, message-input, session-selector, model-indicator.

---

## Wishlist -- Things We Want Even If Hard

These are ideas that may not make v1 but belong in the design space. They represent the ceiling of what this system could be. Organized roughly by how deeply they change the interaction model.

### Interaction Model

#### Shared Cursor / Presence

When Cody is looking at the Projects view, agents can see "operator is viewing Phase 2." When an agent is actively executing, the theatron shows which file or tool the agent is touching. Mutual awareness of attention. Inspiration: multiplayer cursors in Figma, but asymmetric -- one human, many agents.

#### Voice Notes

Press-and-hold to record a voice note from the theatron. Transcribed and delivered to the agent as a message. The agent can respond with synthesized voice. Low-friction input when typing is too slow. We already have voice_reply in the agent toolkit -- this is the UI counterpart.

#### Command Palette

Cmd+K / Ctrl+K universal command palette. "Switch Syn to Opus." "Show health." "Create task: review PR #390." "Pause Demi's phase." Type a natural language command, the theatron interprets and executes. Blurs the line between UI navigation and agent instruction.

#### Quick Capture

Floating button or keyboard shortcut to capture a thought without context-switching views. "Note: check if CozoDB compaction is scheduled" -- drops into an inbox that prosoche can score and route. Inspired by Things/Todoist quick-add.

### Observability

#### Context Window Visualizer

Show the token budget breakdown for any agent's current or recent turn. System prompt: 12K tokens. History: 8K. Tools: 2K. Available for response: 78K. Stacked bar chart per turn. Helps diagnose "why did the agent forget X?" -- because it was pushed out of the window. Inspired by Langfuse's trace detail view.

#### Memory Graph Explorer

Interactive visualization of an agent's CozoDB knowledge graph. Nodes = entities, edges = relationships. Click a node to see its facts, provenance, and temporal history. Filter by entity type, time range, or confidence. Uses the graph surface type from Spec 43b canvas.

#### Prompt Playground

Select an agent, see its current system prompt fully assembled (SOUL + TELOS + MNEME + tools + prosoche + tasks). Edit it live. Run a test turn against it. See the response. Iterate. Based on Langfuse's playground concept but tightly integrated with our assembly pipeline.

#### Log Stream

Tail logs from any subsystem in real-time within the theatron. Filter by crate, log level, agent. Inspired by Clawd Control's activity feed, but with structured tracing spans (from Spec 41 observability). Click a span to expand its children.

#### Infinite Loop Detection

Visual indicator when an agent is stuck in a loop -- editing the same file repeatedly, retrying the same tool call, or generating similar outputs across turns. Ralph TUI's core value proposition. The theatron should surface this pattern automatically and offer a "break loop" button. We already track tool calls per session.

### Control

#### Tiered Model Strategy

Not just per-agent model selection, but per-task-type model routing. "Use Haiku for lint fixes, Sonnet for feature work, Opus for architecture decisions." Configure tiers in the theatron, enforced by hermeneus model routing. Inspired by Ralph TUI's tiered cost strategy.

#### Autonomous Run Limits

When autonomous mode is ON, set iteration limits: "Work on up to 5 tasks, then stop and report." Prevents runaway autonomous sessions. Visible in agent detail as a progress bar (3/5 tasks completed this cycle).

#### Scheduled Actions

"At 6am tomorrow, have Demi start the test coverage expansion." "Every Monday, run a full health check and report." Cron-like scheduling from the UI that writes to daemon config. Visual calendar or timeline of upcoming scheduled actions.

#### Bulk Operations

Select multiple agents, apply the same model change. Select multiple tasks, reassign to a different agent. Multi-select with shift-click, batch actions toolbar. Important as the team grows beyond 4 agents.

### Visualization

#### Ambient Mode

A view designed to stay open on a secondary monitor. Minimal chrome, large type, auto-rotating between: active agent status, task board, cost ticker, phase progress. The theatron as a passive awareness surface -- glanceable, not interactive. Think airport departure board aesthetic.

#### Activity Heatmap

GitHub-contribution-style heatmap showing agent activity over time. Color intensity = number of turns/tasks completed. Hover for details. Spans days/weeks/months. Shows patterns: "Demi is most active on Mondays," "Syn had a burst Wednesday evening."

#### Dependency Graph View

Not just Dianoia phases, but a broader view of how everything connects: specs reference issues, issues reference code, code references tests, agents own domains. A navigable graph of the entire project topology. Click any node to drill into it.

### Data

#### Replay Diffing

Compare two session replays side by side. Same task, different models. Same agent, before and after a prompt change. See where behavior diverged. Useful for evaluating model switches and prompt engineering.

#### Agent Journaling

A read-only view of an agent's memory files, session notes, and MNEME entries over time. See how the agent's understanding of the world has evolved. Timeline scrubbing. The unconcealment of the agent's inner life.

#### Evaluation Snapshots

Periodically snapshot agent performance on standard tasks. Track quality over time as prompts, models, and memory evolve. Inspired by Langfuse's evaluation datasets. "Is Syn getting better at PR reviews?"

#### Export / Reporting

Generate a PDF or markdown summary of work completed this week/month. Tasks done, cost incurred, phases advanced, sessions run. Useful for Cody's own accountability and for sharing with collaborators who don't have workspace access.

### Platform

#### Mobile Companion

Not a full mobile app -- a responsive web view optimized for phone. Check tasks, see health, toggle a model. Quick actions from the couch. The full theatron stays on the desktop.

#### Notification Bridge

Theatron events (task completed, phase failed, health degraded) can optionally push to Signal or system notifications. The theatron doesn't require you to be watching it -- it reaches out when something needs attention.

#### Plugin Marketplace View

If prostheke (WASM plugins) matures, a view for browsing, installing, and configuring plugins. Each plugin shows its granted capabilities, resource usage, and health.

### Search + Navigation

#### Global Search

Cmd+/ or a persistent search bar. Search across everything simultaneously: sessions, tasks, memory facts, file contents, view names, widget types, config keys, agent notes. Results grouped by domain. Click any result to navigate directly. The theatron's nervous system -- find anything in the system without knowing where it lives.

#### Breadcrumb Trail

Show navigation history as a breadcrumb path. "Home → Syn → Session #s_01J8K → Turn 14." Click any segment to jump back. Supports deep drill-down without losing context. Useful when exploring session replays or chasing a thread through agent → session → tool call → file.

#### Pinned Views

Pin any view to a favorites bar for one-click access. "Syn's detail," "Current sprint," "Cost this week." Personal shortcuts that survive sessions. The operator's bookmarks.

### Governance + Audit

#### Decision Log

Every mutation the operator makes through the theatron -- model toggle, task creation, config change, phase approval, loop break -- logged with timestamp, old value, new value, and context. Not for compliance. For "what did I change yesterday that broke Demi's session?" Browsable, searchable, filterable by agent/action type.

#### Annotation Layer

Attach sticky notes to anything: a session, a turn, a widget, an agent, a phase. "This session was great -- Demi nailed the test strategy." "This phase took too long because of the CozoDB compaction issue." Annotations persist and surface in relevant views. They become training data for evaluation snapshots and the raw material for retrospectives.

#### Permission Boundaries

As agent capabilities grow, define what each agent can do through the theatron. Can Demi create views scoped to other agents? Can Syn modify taxis config directly? Not ACLs -- lightweight guardrails. Probably just a taxis config section with sensible defaults. Mostly relevant once agents start authoring views and interacting with the system autonomously.

### Temporal

#### Time Travel

"Show me the home dashboard as it looked at 3pm yesterday." The theatron snapshots system state periodically (agent statuses, task lists, health, cost). Scrub a timeline to see historical state. Not replay (that's session-scoped) -- this is system-wide state at a point in time. Answers "when did things go sideways?" when you come back to the desk after being away.

#### Drift Detection

Has an agent's behavior changed over time? Compare tool usage patterns, error rates, response lengths, cost per turn, and task completion rates across weeks and months. Surface subtle degradation: "Syn's average PR review quality dropped 15% after the model switch on March 3." Feeds into evaluation snapshots but distinct -- drift detection is passive and continuous, evaluations are active and periodic.

#### Config Diffing

What changed in taxis since yesterday? Since last week? Diff any two config snapshots. Shows which agents had model changes, which thresholds moved, which groups were modified. Paired with the decision log, this reconstructs the full "what happened and why" story for any time period.

### Intelligence

#### What-If Projections

"If I switch Syn to Haiku for all lint tasks, what would last week have cost?" Retroactive cost modeling based on historical token usage and alternative model pricing. Doesn't require re-running anything -- just reprices the token counts. Helps make informed model routing decisions before committing.

#### Correlation Surfacing

The theatron knows cost, quality (via annotations/evals), speed (session duration), and model choice. Surface correlations automatically: "Sessions using Opus for architecture tasks have 40% fewer correction notes." "Haiku sessions are 3x cheaper but take 2x more turns on average." The data exists -- the theatron just needs to connect dots.

#### Suggested Actions

Based on patterns, prosoche signals, and system state, the theatron suggests actions: "Demi has been idle for 6 hours and there are 3 open test tasks -- wake Demi?" "Cost is trending 20% over last week -- consider switching Akron to Haiku for routine work." Not autonomous execution -- just nudges in a persistent suggestions panel. The operator decides.

### Ceremony

#### Standup View

A purpose-built view for daily check-ins. For each agent: what it did since last standup, what's planned next, what's blocked. Auto-populated from session history, task completions, and prosoche signals. The operator reviews it with coffee. Replaces "hey Syn, what's the status?" with a surface that's always ready.

#### Retrospective View

End-of-week or end-of-sprint view. Aggregates: tasks completed, cost incurred, phases advanced, sessions run, errors encountered, annotations left. Side-by-side comparison with previous period. "This week cost $45 (down from $62) and completed 14 tasks (up from 9)." The material for honest self-assessment of the human-agent team.

#### New Agent Onboarding

When a new nous joins the team, a guided setup flow: choose a name (with gnomon suggestions), configure personality/domain, select model, seed initial memory, assign to groups, create a starter dashboard. Not a wizard -- a purpose-built view that walks through the bootstrapping steps and creates the initial taxis config. Makes adding the 7th, 8th, 9th agent feel intentional rather than ad hoc.

### Infrastructure

#### Resource Monitor

Not just token cost but compute cost. CPU, memory, and disk usage per agent session. CozoDB size trends over time. Embedding index growth. Session store size. The theatron as a window into the machine, not just the agents. Useful for capacity planning: "At this growth rate, CozoDB will hit 1GB in 4 months."

#### Communication Graph

Visualize inter-agent communication. Which agents ask each other questions (sessions_ask)? Which send fire-and-forget messages (sessions_send)? How often? A force-directed graph where node size = activity, edge thickness = communication frequency. Shows team dynamics: is Syn a bottleneck? Are there isolated agents that never collaborate?

#### External Integration Tiles

GitHub PR status, CI pipeline health, NAS availability, network status. Small status tiles that surface information from outside Aletheia. Bind to REST endpoints or webhook receivers. The theatron as the single pane of glass for the operator's entire infrastructure, not just the agent system.

---

## Implementation Strategy

### Phase 0: Frame + Widget Engine

The skeleton that everything hangs on, plus the composition engine that makes everything else possible.

- Pylon serves static Svelte build at `/` (already does this)
- Single SSE endpoint `/ws/events` with typed event discriminator
- Symbolon auth for all theatron routes
- **Widget registry** -- typed widget components with declared data bindings, config schemas, and size constraints
- **Layout renderer** -- 12-column grid engine with placement, resize, collapse
- **View store** -- CRUD API for view definitions (`/api/theatron/views`), persisted in taxis config
- **Nous-scope resolution** -- views know their scope, widgets inherit scope for data filtering
- Navigation shell: Home, Agents (grouped), Projects, Health, Cost, Chat
- Agent bar (from Spec 29) as persistent top element

### Phase 1: Home + Tasks + Default Widgets

The "sit down at the desk" experience, composed from the Phase 0 engine.

- Task CRUD API (`/api/tasks`)
- Task store (SQLite, system-level -- not per-session)
- Default Home view definition: greeting, task-list, active-now, health-summary, cost-sparkline widgets
- Agent tools: `task_list`, `task_create`, `task_complete`
- SSE events: `task:created`, `task:updated`, `task:completed`
- First batch of widget types: task-list, agent-bar, health-summary, cost-sparkline, active-now, greeting

### Phase 2: Agent Detail + Controls + Groups

See and control each agent. Nous-scoped views.

- Agent detail view definition with status, metrics, recent sessions widgets
- Config toggle widgets that write to taxis (model, autonomy, wake, prosoche)
- Hot-reload via arc-swap so toggles take effect without restart
- Prosoche attention widget (read-only view of scored signals)
- **Agent groups** -- taxis config for named groups, group navigation, group-scoped views
- Agent-authored view API (POST `/api/theatron/views` with author field)

### Phase 3: Health Board + Log Stream

Full system visibility.

- Health check endpoints for each subsystem
- Heartbeat protocol (each actor reports periodically)
- Health view with tiles, status indicators, action buttons
- Service actions: restart, force-cycle, compact, clear cache
- Log stream widget with crate/level/agent filtering

### Phase 4: Projects + Phases

Dianoia visualization.

- Phase dependency graph renderer widget (SVG or Canvas)
- Plan-level detail widget with agent assignment and status
- Checkpoint approval widget
- Verification gap display

### Phase 5: Cost Cockpit

Token economics.

- Per-turn cost recording (already in session store)
- Aggregation queries by agent, model, time window
- Cost view with chart widgets, projections, model comparison
- Budget alert configuration

### Phase 6: Session Replay

The crown jewel.

- Session timeline data structure (turns, tools, thinking, tokens)
- Timeline renderer widget with scrubbing
- Turn detail expansion (full request/response)
- Context window visualizer widget (token budget per turn)
- Active session live-follow mode

### Phase 7: Canvas Integration + Drag-and-Drop

Spec 43b brought into the theatron, plus full layout customization.

- Canvas panel widget (collapsible sidebar or inline)
- Surface type renderers (progress, table, metrics, markdown, graph)
- Agent tool: `canvas_update`
- Drag-and-drop widget rearrangement
- Resize handles
- Save/load custom layouts

### Phase 8: Command Palette + Quick Capture

Power-user interaction model.

- Cmd+K palette with natural language parsing
- Quick capture inbox with prosoche routing
- Keyboard shortcuts for common actions

---

## Open Questions

1. **Task backend: SQLite or CozoDB?** Tasks are simple CRUD. SQLite is proven (sessions already use it). CozoDB could unify storage but adds complexity for a flat table. Leaning SQLite.

2. **SSE event schema.** One mega-stream with typed discriminators, or multiple SSE endpoints per view? Single stream is simpler for the client. Multiple streams allow partial subscription. Need to decide.

3. **Config hot-reload granularity.** Can we reload a single agent's model without touching other config? arc-swap replaces the whole config atomically. May need per-agent config segments.

4. **Session replay storage.** Full turn data (including thinking blocks and tool I/O) is large. Store everything? Trim after N days? Separate replay store from session store?

5. **How does autonomous mode work?** When toggled ON, does the agent immediately scan for work? Or wait for next prosoche cycle? What triggers the first autonomous turn?

6. **Widget registry: static or dynamic?** Are widget types compiled into the Svelte build (static set, fast), or loaded dynamically at runtime (extensible, slower)? Static for v1, dynamic for prostheke plugin widgets later?

7. **View definition format.** YAML in taxis config? JSON in SQLite? Both? The view store needs to support both built-in defaults (shipped with the binary) and user-created views (persisted at runtime).

8. **Agent group membership: static or dynamic?** Config-defined groups are simple but rigid. Alternatively, groups could be derived from task assignments or domain ownership. Start static, evolve later.

9. **View authoring permissions.** Can any agent create any view? Or only views scoped to themselves? Should agent-authored views require operator approval before appearing in navigation?

---

## Non-Goals

- Multi-user / multi-tenant (this is personal infrastructure)
- Mobile app (web-responsive is sufficient for v1)
- Replacing Signal (complementary, not competitive)
- Real-time collaborative editing (not Google Docs for agents)

---

## References

### Internal

- [Spec 29 -- UI Layout & Theming](29_ui-layout-and-theming.md)
- [Spec 30 -- Homepage Dashboard](30_homepage-dashboard.md)
- [Spec 41 -- Observability](41_observability.md)
- [Spec 43b -- A2UI Live Canvas](43_a2ui-canvas.md)
- [Spec 39 -- Autonomy Gradient](archive/DECISIONS.md) (absorbed)

### External -- Agent Dashboards + Ops

- [AgentOps](https://github.com/AgentOps-AI/agentops) (5.3K stars, MIT) -- Session replay timelines, cost tracking, agent execution graphs. Best reference for session replay UI.
- [Clawd Control](https://github.com/Temaki-AI/clawd-control) (111 stars, MIT) -- SSE live updates, fleet health tiles, agent detail drilldowns. Vanilla HTML/JS + Node. Best reference for simple health monitoring.
- [mudrii/openclaw-dashboard](https://github.com/mudrii/openclaw-dashboard) (136 stars, MIT) -- Python + pure HTML/SVG. 11 panels: cost trends, session tracking, sub-agent hierarchy. Zero external dependencies.
- [Lattice Workbench](https://latticeruntime.com/) (Apache 2.0) -- Agent IDE + operations console. Enforcement primitives (identity, auth, audit, constraints).
- [Ralph TUI](https://www.verdent.ai/guides/ralph-tui-ai-agent-dashboard) -- Terminal agent mission control. Iteration limits, infinite loop detection, tiered model strategy.

### External -- LLM Engineering + Composition

- [Langfuse](https://github.com/langfuse/langfuse) (22.5K stars, open source) -- LLM observability, trace visualization, prompt playground, evaluation datasets. Best reference for trace detail views.
- [Homarr](https://homarr.dev/) -- Self-hosted server dashboard with drag-and-drop widget placement. Reference for composable layout patterns.
