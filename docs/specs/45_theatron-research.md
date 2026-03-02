# Theatron Research Notes -- Implementation Readiness Audit

**Date:** 2026-03-01
**Purpose:** Identify what needs to be decided or built before Spec 45 can be turned into dispatch-ready prompts at M6.

---

## 1. Existing UI Inventory

The current `ui/` directory is a Svelte 5 app with substantial infrastructure already in place. This is NOT a greenfield build.

### What Already Exists

| Component | Files | Status | Theatron Relevance |
|-----------|-------|--------|-------------------|
| SSE event system | `lib/events.svelte.ts` | Production | **Core reuse.** Already handles reconnection, heartbeat, typed events (turn, tool, status, planning, task). Theatron's "SSE as the spine" is already built. |
| Agent store | `stores/agents.svelte.ts` | Production | Direct reuse. Identity cache, emoji, active agent tracking. |
| Task store | `stores/tasks.svelte.ts` | Production | Direct reuse. Full CRUD, optimistic updates, SSE subscription. |
| Chat view | `components/chat/*` (18 files) | Production | Becomes Chat view widget. Already has streaming, tool calls, thinking panels, planning cards, markdown. |
| Planning view | `components/planning/*` (15 files) | Production | Becomes Projects view widget. Has execution status, roadmap, retrospective, checkpoints, discussion, annotations. |
| Metrics view | `components/metrics/*` | Production | Becomes Health/Cost widget. Has uptime, tokens, cache rates, cron status, service health. |
| Cost dashboard | `CostDashboard.svelte` | Production | Becomes Cost widget. Has per-agent cost, token breakdown. |
| Graph view | `components/graph/*` (7 files) | Production | Becomes Memory Graph Explorer widget. Has 2D/3D force graphs, node cards, timeline slider, drift panel, health bar. |
| File editor | `components/files/*` (4 files) | Production | Becomes code/file widget. Has tree explorer, CodeMirror editor tabs. |
| Settings view | `components/settings/*` | Production | Partial reuse for agent controls. |
| Auth/onboarding | `components/auth/*`, `components/onboarding/*` | Production | Reuse as-is. Setup wizard, login flow. |
| Layout | `components/layout/*` | Production | **Replace.** Current layout is chat-centric with tab switching. Theatron needs grid-based view composition. |
| Shared components | `components/shared/*` | Production | Reuse: Badge, ErrorBanner, Spinner, Toast. |
| Type system | `lib/types.ts` | Production | Extensive types for agents, sessions, messages, tools, plans, metrics, graphs, costs, threads. Foundation for widget data contracts. |
| Utility libs | `lib/api.ts`, `lib/format.ts`, `lib/auth.ts`, `lib/stream.ts` | Production | Direct reuse. |

### Key Patterns Established

- **Svelte 5 runes** (`$state`, `$effect`) for all reactive state -- NOT legacy stores.
- **SvelteMap/SvelteSet** from `svelte/reactivity` for collections.
- **SSE dispatch model:** `onGlobalEvent(cb)` subscribes, components filter by event type.
- **Optimistic mutations** with rollback on error (see task store).
- **Auth:** Token-based with httpOnly cookie session refresh.
- **No component library.** All custom CSS, CSS variables for theming (`--text-muted`, `--border`, `--accent`, etc.).
- **No chart library.** `UsageChart.svelte` likely uses SVG or Canvas directly. `force-graph` and `3d-force-graph` for graph viz.
- **CodeMirror 6** for code editing.
- **Vite 7** + **vitest 4** for build/test.
- **Svelte 5.53.5** (latest rune-based reactive system).

---

## 2. Clean-Room Assessment

**Question:** Do we rebuild from scratch, or restructure what exists?

**Answer: Neither extreme. Peel and restructure.**

The existing UI has two layers:
1. **Shell** (Layout.svelte, TopBar.svelte, App.svelte) -- chat-centric, tab-switching navigation. This is the part that needs to be replaced with Theatron's view composition engine.
2. **Content components** -- ChatView, PlanningView, MetricsView, GraphView, CostDashboard, FileEditor. These are the actual useful UI. They're already self-contained with their own data fetching and state management.

The content components are already widget-shaped. PlanningView doesn't know it's in a tab -- it just renders planning state. CostDashboard doesn't care about navigation -- it fetches costs and shows cards. The refactor is:

- **Replace the shell** with Theatron's grid layout + widget registry + view store.
- **Wrap existing content components as widgets** by adding widget metadata (data bindings, config schema, size constraints) and registering them.
- **Incrementally build new widgets** (prosoche signals, session replay, agent controls) that don't exist yet.

This means the existing stores (agents, tasks, chat, planning, etc.) and utility libs (api, auth, events, format) survive untouched. The SSE event system survives untouched. The content components get thin widget wrappers. The only thing that dies is Layout.svelte's tab-switching approach.

**What gets clean-roomed:**
- Layout engine (new: grid-based composition)
- Widget registry (new: component type → lazy import map)
- View store (new: CRUD for view definitions)
- Navigation (new: scope-aware sidebar with groups)
- The shell CSS (new: grid system, widget chrome)

**What gets wrapped/adapted:**
- All existing content components become widgets
- Existing stores gain scope-filtering (pass nousId down)
- Existing CSS variables and theme system carry forward

**What survives as-is:**
- SSE event system (`events.svelte.ts`)
- All stores (`stores/*.svelte.ts`)
- All lib utilities (`lib/*.ts`)
- Auth flow
- Shared components (Badge, Spinner, Toast, ErrorBanner)

---

## 3. Technology Stack Decisions

### 3a. Component Library: shadcn-svelte

**Decision: Use shadcn-svelte.**

[shadcn-svelte](https://www.shadcn-svelte.com/) (6.9K stars) is the Svelte port of shadcn/ui. It is NOT a dependency you install -- it's a collection of components you copy into your project and own. This is the key distinction from traditional component libraries.

**Why it's the right choice:**

1. **You own the code.** Components live in your repo at `ui/src/lib/components/ui/`. No node_modules dependency to fight. Full customization control. Matches the philosophy of understanding what you use.

2. **Built on Bits UI.** The accessibility primitives (dialogs, dropdowns, tooltips, popovers, command palettes) are battle-tested. We don't reinvent focus management and ARIA patterns.

3. **Theming via CSS variables.** shadcn-svelte uses CSS custom properties for all colors, radii, spacing. We already use CSS variables. Migration is remapping variable names, not a rewrite.

4. **Dashboard examples.** shadcn-svelte ships with dashboard, task manager, and playground examples that are structurally close to what Theatron needs.

5. **Chart system built in.** shadcn-svelte's chart components use LayerChart (see below). Copy the chart components, customize, done.

6. **Command palette.** The `Command` component is exactly Cmd+K. Copy it, wire it to Theatron actions. Wishlist item delivered as a component, not a feature build.

7. **Svelte 5 native.** Uses runes, snippets, and the latest Svelte patterns. No legacy compatibility overhead.

**What we adopt from shadcn-svelte:**
- Card, Button, Badge, Dialog, Dropdown, Tooltip, Command (palette), Tabs, Toggle, Slider, Select
- Chart wrappers (Area, Bar, Line, Pie, Radar, Radial)
- Theme system (CSS variables, dark mode)
- Form primitives (Input, Label, Field)

**What we don't adopt:**
- Table (we have custom task/session lists)
- Full form validation (we have simpler needs)
- Anything that doesn't serve Theatron's dashboard use case

**Migration path:**
1. Install Bits UI as a dependency (the one actual dep -- it's the accessibility primitive layer)
2. Copy shadcn-svelte components into `ui/src/lib/components/ui/`
3. Remap existing CSS variables to shadcn-svelte's naming convention (or vice versa)
4. Gradually replace custom buttons, badges, dropdowns, dialogs with shadcn components
5. Adopt chart components for Cost Cockpit and activity visualization

### 3b. Charts: LayerChart + LayerCake

**Decision: LayerChart for rich charts, custom SVG for sparklines.**

[LayerCake](https://layercake.graphics/) is a headless Svelte-native graphics framework. It creates scales from your data and target div dimensions, then lets you layer SVG, HTML, Canvas, or WebGL components that share the same coordinate space.

[LayerChart](https://github.com/techniq/layerchart) builds on LayerCake to provide ready-made chart components: area, bar, line, scatter, pie, sankey, treemap, and more. It's what shadcn-svelte uses for its chart system.

**Why this stack:**

1. **Svelte-native.** Components are Svelte files, not a JS library with a Svelte wrapper. Reactive, SSR-capable, tree-shakeable.

2. **Headless composability.** LayerCake provides the scales and layout. You build the visual layers. This means we can create custom chart types (context window stacked bars, activity heatmaps, dependency graphs) without fighting a library's opinion about what charts should look like.

3. **SVG output.** CSS-stylable, theme-able with our CSS variables. No canvas-only lock-in.

4. **SSR support.** Charts can render server-side with percentage-based scales. Useful for export/reporting wishlist item.

5. **shadcn integration.** The chart components from shadcn-svelte are LayerChart wrappers with consistent theming. Copy them, customize, compose.

**For simple sparklines** (cost trend in a card, token count mini-chart), we'll use custom SVG. A sparkline is 10 lines of SVG -- a chart library adds nothing. Keep these lightweight.

**For complex visualizations** (session timeline, activity heatmap, cost cockpit, dependency graph), use LayerChart/LayerCake for the heavy lifting.

### 3c. Grid Layout: gridstack.js for drag-and-drop

**Decision: CSS Grid for static rendering, gridstack.js for interactive editing.**

[gridstack.js](https://github.com/gridstack/gridstack.js) (8.7K stars, MIT, v12) is a mature, zero-dependency TypeScript library for dashboard layouts with drag-and-drop.

**Key capabilities:**
- Pure HTML5 drag-and-drop (no jQuery since v6)
- CSS variable-based column system (no extra CSS needed since v12)
- Serialize/load from JSON arrays: `grid.load([{x: 0, y: 0, w: 2, h: 2}, ...])`
- Responsive breakpoints via `columnOpts`
- Touch device support built in
- Nested grids (sub-grids within widgets)
- Custom engine extensibility

**Why two layers:**

The theatron needs two modes:
1. **View mode** (default): Render widgets in a grid from a view definition. No drag handles, no resize borders. Clean, focused. CSS Grid handles this perfectly -- it's just `grid-template-columns: repeat(12, 1fr)` with widgets placed via `grid-column` and `grid-row`.

2. **Edit mode** (toggled): Rearrange widgets, resize them, add/remove. gridstack.js handles this -- it's purpose-built for exactly this interaction pattern.

**Integration approach:**
- Render all views using CSS Grid in view mode (fast, simple, no library)
- When "Edit layout" is toggled, mount gridstack.js over the same grid
- gridstack.js reads widget positions, enables drag/resize
- On save, serialize positions back to the view definition
- Unmount gridstack.js, return to CSS Grid view mode

This avoids gridstack.js as a runtime dependency for normal viewing. It's only loaded when editing, via dynamic import. The view definition format works for both modes because gridstack.js uses the same `{x, y, w, h}` model as CSS Grid placement.

**Svelte integration:** No official wrapper exists. We'll write a thin one:
- Svelte action (`use:gridstack`) that initializes gridstack on a container
- Reactive updates when view definition changes
- Event forwarding (change → update view definition store)
- ~100 lines of wrapper code, not a framework battle

### 3d. Icons: Lucide

**Decision: Lucide icons.**

shadcn-svelte uses [Lucide](https://lucide.dev/) icons. 1000+ icons, tree-shakeable (only import what you use), Svelte components, consistent 24x24 grid. Already the standard in the shadcn ecosystem. No decision needed -- it comes with the component library choice.

---

## 4. View Definition Schema

This is the contract that everything builds on. Written as TypeScript types with YAML examples.

### TypeScript Types

```typescript
// ─── Core Types ──────────────────────────────────────────────

/** Scope determines whose data a view shows */
type ViewScope =
  | { type: 'global' }
  | { type: 'group'; groupId: string }
  | { type: 'nous'; nousId: string };

/** A complete view definition */
interface ViewDefinition {
  /** Unique identifier (UUID for runtime views, slug for built-ins) */
  id: string;

  /** Human-readable name */
  name: string;

  /** Whose data this view shows */
  scope: ViewScope;

  /** Who created this view */
  author: 'system' | 'operator' | string; // string = nous ID

  /** When this was created */
  createdAt: string; // ISO 8601

  /** When last modified */
  updatedAt: string; // ISO 8601

  /** Grid layout configuration */
  layout: GridLayout;

  /** Widgets placed in the grid */
  widgets: WidgetPlacement[];

  /** Optional: view-level data filters applied to all widgets */
  filters?: Record<string, unknown>;

  /** Whether this is a built-in default (not deletable, but forkable) */
  builtIn?: boolean;

  /** Optional: icon identifier for navigation */
  icon?: string;
}

/** Grid layout dimensions */
interface GridLayout {
  /** Number of columns (default: 12) */
  columns: number;

  /** Row behavior: 'auto' grows with content, number is fixed row count */
  rows: 'auto' | number;

  /** Gap between widgets in pixels */
  gap?: number;

  /** Responsive breakpoints: at width <= breakpoint, use specified columns */
  breakpoints?: Array<{
    maxWidth: number;
    columns: number;
  }>;
}

// ─── Widget Placement ────────────────────────────────────────

/** A widget placed in the grid */
interface WidgetPlacement {
  /** Instance ID (unique within this view) */
  id: string;

  /** Widget type from the registry */
  type: string;

  /** Grid position */
  position: GridPosition;

  /** Override the view's scope for this widget */
  scope?: ViewScope;

  /** Data source override (otherwise widget uses its default) */
  source?: DataSource;

  /** Widget-specific configuration */
  config?: Record<string, unknown>;

  /** Whether this widget starts collapsed */
  collapsed?: boolean;

  /** Display priority for responsive collapse (lower = keep visible) */
  priority?: number;
}

/** Position within the 12-column grid */
interface GridPosition {
  /** Starting column (1-based) */
  col: number;

  /** Starting row (1-based) */
  row: number;

  /** Column span */
  width: number;

  /** Row span */
  height: number;
}

// ─── Data Sources ────────────────────────────────────────────

/** How a widget gets its data */
type DataSource =
  | { type: 'sse'; event: string }
  | { type: 'rest'; path: string; poll?: number }
  | { type: 'query'; engine: 'cozo'; query: string; params?: Record<string, unknown> }
  | { type: 'file'; glob: string }
  | { type: 'derived'; sourceWidgetId: string; transform: string };

// ─── Widget Registry ─────────────────────────────────────────

/** Metadata for a registered widget type */
interface WidgetTypeDefinition {
  /** Unique type identifier */
  type: string;

  /** Human-readable name */
  name: string;

  /** Description for widget picker */
  description: string;

  /** Category for grouping in widget picker */
  category: 'status' | 'tasks' | 'metrics' | 'charts' | 'data' | 'control' | 'content';

  /** Default data source if not overridden by placement */
  defaultSource?: DataSource;

  /** JSON schema for the config object */
  configSchema?: Record<string, unknown>;

  /** Size constraints in grid cells */
  minWidth: number;
  minHeight: number;
  maxWidth?: number;
  maxHeight?: number;

  /** Which scopes this widget supports */
  scopes: Array<'global' | 'group' | 'nous'>;

  /** Lazy import function for the Svelte component */
  component: () => Promise<{ default: typeof import('*.svelte').default }>;
}

// ─── Agent Groups ────────────────────────────────────────────

/** Named group of agents */
interface AgentGroup {
  /** Group identifier */
  id: string;

  /** Human-readable name */
  name: string;

  /** Agent IDs in this group */
  members: string[];

  /** Icon identifier */
  icon?: string;

  /** Optional description */
  description?: string;
}
```

### YAML Example: Default Home View

```yaml
id: home
name: "Home"
scope:
  type: global
author: system
builtIn: true
icon: home
layout:
  columns: 12
  rows: auto
  gap: 16
  breakpoints:
    - maxWidth: 768
      columns: 1
    - maxWidth: 1024
      columns: 6
widgets:
  - id: agent-bar
    type: agent-status-bar
    position: { col: 1, row: 1, width: 12, height: 1 }
    priority: 1

  - id: greeting
    type: greeting
    position: { col: 1, row: 2, width: 12, height: 1 }
    priority: 2

  - id: tasks
    type: task-list
    position: { col: 1, row: 3, width: 6, height: 4 }
    source: { type: rest, path: "/api/tasks/daily" }
    config:
      showCompleted: false
      showAssignee: true
    priority: 3

  - id: active-now
    type: active-sessions
    position: { col: 7, row: 3, width: 6, height: 2 }
    priority: 4

  - id: health
    type: health-summary
    position: { col: 7, row: 5, width: 3, height: 2 }
    source: { type: rest, path: "/api/health", poll: 30 }
    priority: 5

  - id: cost-7d
    type: cost-sparkline
    position: { col: 10, row: 5, width: 3, height: 2 }
    config:
      window: 7d
      groupBy: model
    priority: 6
```

### YAML Example: Syn's Personal Dashboard

```yaml
id: syn-overview
name: "Syn Overview"
scope:
  type: nous
  nousId: syn
author: operator
icon: brain
layout:
  columns: 12
  rows: auto
  gap: 16
widgets:
  - id: status
    type: agent-status-header
    position: { col: 1, row: 1, width: 12, height: 1 }
    config:
      showControls: true

  - id: pr-status
    type: github-prs
    position: { col: 1, row: 2, width: 6, height: 3 }
    config:
      repo: forkwright/aletheia
      filter: author

  - id: recent-sessions
    type: session-list
    position: { col: 7, row: 2, width: 6, height: 3 }
    config:
      limit: 5

  - id: prosoche
    type: prosoche-signals
    position: { col: 1, row: 5, width: 6, height: 3 }
    config:
      minScore: 0.3

  - id: cost
    type: cost-sparkline
    position: { col: 7, row: 5, width: 6, height: 3 }
    config:
      window: 7d
      groupBy: model

  - id: tasks
    type: task-list
    position: { col: 1, row: 8, width: 12, height: 4 }
    config:
      showCompleted: true
```

### YAML Example: Agent-Authored View (Demi's Coverage Report)

```yaml
id: demi-coverage-2026-03
name: "Coverage Report - March 2026"
scope:
  type: nous
  nousId: demi
author: demi
icon: shield-check
layout:
  columns: 12
  rows: auto
  gap: 16
widgets:
  - id: summary
    type: stat-cards
    position: { col: 1, row: 1, width: 12, height: 1 }
    config:
      stats:
        - label: "Total Tests"
          value: 718
          trend: "+42 this week"
        - label: "Coverage"
          value: "84%"
          trend: "+6% from last month"
        - label: "Failing"
          value: 0
          variant: success

  - id: coverage-by-crate
    type: bar-chart
    position: { col: 1, row: 2, width: 8, height: 4 }
    source:
      type: rest
      path: "/api/metrics/coverage-by-crate"
    config:
      xField: crate
      yField: coverage
      target: 80
      colorScale: threshold

  - id: test-trend
    type: area-chart
    position: { col: 9, row: 2, width: 4, height: 4 }
    source:
      type: rest
      path: "/api/metrics/test-count-history"
    config:
      xField: date
      yField: count
      window: 30d
```

---

## 5. Widget Type Catalog (Initial Set)

Organized by category, showing which existing component maps to each widget.

### Status Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `agent-status-bar` | `AgentPill.svelte` | Adapt | Horizontal bar of all agents with status indicators |
| `agent-status-header` | `AgentCard.svelte` | Adapt | Full agent header with name, model, status, metrics |
| `greeting` | None | New | Time-aware greeting with date |
| `active-sessions` | None (partial in chat) | New | Currently running agent sessions with progress |

### Task Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `task-list` | `planning/TaskList.svelte` | Adapt | Filterable task list with CRUD |
| `task-board` | None | New | Kanban-style columns (pending/active/done) |

### Metrics Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `health-summary` | `MetricsView.svelte` (partial) | Extract | Compact health tiles for all subsystems |
| `health-tile` | None | New | Single subsystem health with actions |
| `cost-sparkline` | `CostDashboard.svelte` (partial) | Extract | Mini cost chart for embedding |
| `cost-breakdown` | `CostDashboard.svelte` | Adapt | Full cost view with per-agent breakdown |
| `stat-cards` | None | New | Row of configurable stat cards (KPI tiles) |

### Chart Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `area-chart` | None | New | Time-series area chart (LayerChart) |
| `bar-chart` | None | New | Categorical bar chart (LayerChart) |
| `line-chart` | None | New | Multi-series line chart (LayerChart) |
| `sparkline` | None | New | Tiny inline chart (custom SVG) |

### Data Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `session-list` | `SessionItem.svelte` | Adapt | Recent sessions with drilldown |
| `session-timeline` | None | New | Session replay timeline (Phase 6) |
| `prosoche-signals` | None | New | Scored signals with priority |
| `memory-graph` | `graph/GraphView.svelte` | Adapt | 2D/3D knowledge graph |
| `log-stream` | None | New | Real-time filtered log tail |

### Control Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `agent-controls` | `SettingsView.svelte` (partial) | Extract | Model toggle, autonomy, wake config |
| `cron-status` | `MetricsView.svelte` (partial) | Extract | Daemon cron list with run/skip actions |

### Content Widgets

| Widget Type | Existing Component | New? | Description |
|-------------|-------------------|------|-------------|
| `chat` | `chat/ChatView.svelte` | Wrap | Full chat interface |
| `planning` | `planning/PlanningView.svelte` | Wrap | Full planning dashboard |
| `file-editor` | `files/FileEditor.svelte` | Wrap | CodeMirror file editor |
| `markdown` | None | New | Render markdown content (for notes, docs) |

---

## 6. Pylon Route Gaps

Current Pylon routes (Rust):

```
GET  /api/health
POST /api/sessions
GET  /api/sessions/{id}
DEL  /api/sessions/{id}
POST /api/sessions/{id}/messages
GET  /api/sessions/{id}/history
GET  /api/nous
GET  /api/nous/{id}
GET  /api/nous/{id}/tools
```

Current TypeScript runtime also serves (not yet ported to Rust):

```
GET  /api/events               (SSE stream -- global event bus)
GET  /api/metrics              (system metrics)
GET  /api/credentials          (credential info)
GET  /api/agents/{id}/identity (agent name, emoji)
GET  /api/graph/*              (knowledge graph queries)
GET  /api/tasks*               (task CRUD)
POST /api/tasks                (create)
PATCH /api/tasks/{id}          (update)
DEL  /api/tasks/{id}           (delete)
POST /api/tasks/{id}/complete  (mark done)
GET  /api/setup/status         (onboarding)
```

**Routes Theatron needs that don't exist anywhere:**

```
# View management
GET    /api/theatron/views                  (list all views for scope)
GET    /api/theatron/views/{id}             (get view definition)
POST   /api/theatron/views                  (create view)
PATCH  /api/theatron/views/{id}             (update layout/widgets)
DELETE /api/theatron/views/{id}             (delete view)

# Agent groups
GET    /api/theatron/groups                 (list groups)

# Agent controls (write)
PATCH  /api/nous/{id}/config               (toggle model, autonomy, wake)

# Prosoche
GET    /api/prosoche/signals                (current scored signals)
GET    /api/prosoche/status                 (cycle info, dedup state)
POST   /api/prosoche/cycle                  (force cycle)

# Daemon crons
GET    /api/daemon/crons                    (list crons + status)
POST   /api/daemon/crons/{id}/run           (force run)

# Cost time series
GET    /api/costs/daily?window=7d           (daily cost aggregates)
GET    /api/costs/by-agent?window=7d        (per-agent cost over time)

# Session replay
GET    /api/sessions/{id}/turns             (turn-by-turn data with token counts)

# Decision audit log
GET    /api/theatron/audit                  (operator mutation log)
```

---

## 7. SSE Event Gaps

Current SSE events (from `events.svelte.ts`):

```
init                          (bootstrap: agent list, active turns)
turn:before / turn:after      (agent turn lifecycle)
tool:called / tool:failed     (tool execution)
status:update                 (agent status text)
session:created / archived    (session lifecycle)
distill:before/stage/after    (distillation)
planning:*                    (7 planning events)
task:*                        (5 task events)
ping                          (heartbeat)
```

**Events Theatron needs that don't exist:**

```
health:update          (subsystem health change)
cost:tick              (periodic cost summary push)
prosoche:cycle         (prosoche scored signals)
prosoche:wake          (wake attempt)
config:changed         (taxis config hot-reload notification)
view:updated           (view definition changed by another client/agent)
agent:model-changed    (model toggle took effect)
cron:executed          (daemon cron completed)
```

---

## 8. Implementation Readiness Summary

| Phase | Backend Ready? | Frontend Ready? | Blocking Decisions |
|-------|---------------|----------------|-------------------|
| Phase 0: Frame + Widget Engine | Partial (pylon serves static, has SSE) | Need grid engine, widget registry, view store | None -- decisions made |
| Phase 1: Home + Tasks | Tasks API exists in TS, not Rust | Task store exists, need widget wrappers | None -- can proceed |
| Phase 2: Agent Detail + Controls + Groups | Need `/nous/{id}/config` write endpoint | Agent store exists, need control widgets | Config hot-reload granularity |
| Phase 3: Health Board + Log Stream | Partial (health check exists) | MetricsView exists, need decomposition into widgets | Prosoche/daemon API |
| Phase 4: Projects + Phases | Planning exists in TS runtime | PlanningView exists (15 components!) | None -- mostly widget wrapping |
| Phase 5: Cost Cockpit | Need time-series cost API | CostDashboard exists, need LayerChart integration | None -- decisions made |
| Phase 6: Session Replay | Need turns endpoint with full data | Nothing exists | Session replay data model |
| Phase 7: Canvas + Drag-and-Drop | Canvas API (Spec 43b) | gridstack.js integration | None -- decisions made |
| Phase 8: Command Palette | Need NL parsing or command router | shadcn Command component | NL parsing approach |

---

## 9. Dependency Summary

### New Dependencies

| Package | Purpose | Size | Justification |
|---------|---------|------|---------------|
| `bits-ui` | Accessible UI primitives (dialogs, dropdowns, tooltips, menus) | ~30KB gzip | Foundation for shadcn-svelte components. Replaces hand-rolled accessibility code. |
| `layercake` | Headless chart framework (scales, layout) | ~5KB gzip | Svelte-native, SSR-capable, composable. Foundation for all chart widgets. |
| `layerchart` | Ready-made chart components on LayerCake | ~15KB gzip | Area, bar, line, pie, radar charts. What shadcn-svelte uses. |
| `gridstack` | Drag-and-drop grid layout (edit mode only) | ~25KB gzip | Dynamic import, only loaded in edit mode. 8.7K stars, battle-tested. |
| `lucide-svelte` | Icon library | Tree-shaken | Only imported icons ship. shadcn default. |

### Existing Dependencies (kept)

| Package | Purpose |
|---------|---------|
| `codemirror` + langs | Code editing in file widget |
| `3d-force-graph` / `force-graph` | Knowledge graph visualization |
| `marked` + `dompurify` | Markdown rendering |
| `three` | 3D graph rendering |

### Removed Dependencies (candidates)

None immediately. The existing deps all serve specific widgets. They can be lazy-loaded to avoid impacting initial bundle.

---

## 10. Research Complete -- What's Left

This document answers the three research questions:

1. **Grid layout:** CSS Grid for view rendering + gridstack.js for edit mode. Two layers, one format.
2. **Widget registry:** Static import map with lazy loading. shadcn-svelte components as the design system.
3. **View definition schema:** TypeScript types written above. YAML examples for home, agent, and agent-authored views.

**What's left is execution, not research.** The spec + this research doc contain enough to write dispatch-ready prompts when M6 arrives. The prompts will reference:
- This research doc for tech stack decisions and existing component mapping
- The view definition schema as the contract
- The widget type catalog for what to build per phase
- The Pylon route and SSE event gaps for backend work

**Nothing else needs to happen until M6 except the normal crate work (M3-M5) that builds the backend Theatron will depend on.**
