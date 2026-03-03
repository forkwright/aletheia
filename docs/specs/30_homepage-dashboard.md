# Spec 30 — Homepage Dashboard and Shared Task Board

| Field       | Value                          |
|-------------|--------------------------------|
| Status      | Skeleton                       |
| Author      | Demiurge                       |
| Created     | 2026-02-22                     |
| Scope       | `ui/`, `infrastructure/runtime/` |
| Priority    | High                           |
| Depends On  | Spec 29 (merged)               |

---

## Problem

The UI opens directly into the last-used agent's chat. There's no overview of what needs doing across the system. Each agent operates in its own silo — no shared visibility into pending work, priorities, or who owns what. The human has to remember context across sessions and manually route tasks.

The current workflow: the operator thinks of something, opens the right agent, types it. If it's for a different agent, they switch. If it's for themselves, they write it down somewhere else. There's no single surface where "merge those PRs" (Syn), "respond to James" (operator), and "dig into inventory" (Demi) all live together.

---

## Vision

A homepage that's the first thing you see. A shared task board where tasks belong to agents *or* the human. When an agent starts a session, it reads its assigned tasks as context. When work completes, tasks update. The human sees the full picture at a glance.

```
┌──────────────────────────────────────────────────────────────┐
│ Aletheia  ●Syn Idle  ◉Demi Working  ●Syl Idle     Files ... │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Good evening.                               Feb 22, 5:45 PM │
│                                                              │
│  ┌─ Tasks ──────────────────────────────────────────────┐    │
│  │                                                      │    │
│  │  ☐ Merge open PRs and clean branches       → Syn     │    │
│  │  ☐ Respond to James about Q3 timeline      → Me      │    │
│  │  ☐ Dig into inventory details              → Demi    │    │
│  │  ☐ Update trailer wiring diagram           → Akron   │    │
│  │                                                      │    │
│  │  [+ Add task]                                        │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌─ Recent Activity ───────────────────────────────────┐    │
│  │  Syn — Merged PR #158 (agent bar)        2 hours ago │    │
│  │  Demi — Spec 29 complete                 3 hours ago │    │
│  │  Syl — Updated meal plan                  yesterday  │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌─ System ────────────────────────────────────────────┐    │
│  │  Uptime: 14d 3h  │  Tokens: 2.4M  │  Cost: $47.20  │    │
│  │  4 agents online  │  3 services ok │  Next cron: 8m │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

---

## Architecture (Skeleton)

### Phase 1 — Task Backend

**Data model:**

```typescript
interface Task {
  id: string;              // task_xxxxxxxxxxxx
  title: string;           // "Merge open PRs and clean branches"
  assignee: string;        // agent ID ("syn", "demi") or "human"
  createdBy: string;       // who created it — agent ID or "human"
  status: "open" | "done" | "cancelled";
  priority: "low" | "medium" | "high";
  context?: string;        // optional details, markdown
  createdAt: string;       // ISO timestamp
  completedAt?: string;    // ISO timestamp
  tags?: string[];         // optional categorization
}
```

**Storage:** SQLite table in the existing session store (`mneme`). Tasks are system-level, not per-session.

**API endpoints:**

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/tasks` | List tasks (filter by assignee, status) |
| `POST` | `/api/tasks` | Create task |
| `PATCH` | `/api/tasks/:id` | Update task (status, assignee, title) |
| `DELETE` | `/api/tasks/:id` | Delete task |

**Agent access:** Built-in tools `task_list`, `task_create`, `task_complete` so agents can manage tasks from within their sessions. When an agent's turn starts, its assigned open tasks are injected into the system prompt (like prosoche attention items).

---

### Phase 2 — Homepage View

**New view:** `HomeView.svelte` — the default landing page, replaces chat as the initial view.

**Sections:**

1. **Greeting** — Time-aware ("Good morning/afternoon/evening"), current date/time
2. **Task Board** — Open tasks grouped or sorted by assignee. Inline add, checkbox to complete, drag to reorder (stretch), click to expand context. Assignee selector dropdown (list of agents + "Me").
3. **Recent Activity** — Last N events from the event bus (turns completed, tools called, sessions created). Condensed timeline.
4. **System Summary** — Compact metrics cards: uptime, token usage, cost, agent count, services health. Links to full Metrics view.

**Navigation:** "Home" becomes the first nav item (or the brand name itself is clickable to return home). Chat is accessed by clicking an agent pill.

---

### Phase 3 — Agent Task Injection

When a session turn starts (`context` pipeline stage):
1. Query open tasks assigned to this agent
2. Format as a task list in the system prompt
3. Agent sees: "You have 2 pending tasks: [1] Merge open PRs... [2] ..."

When an agent completes a task (via `task_complete` tool):
1. Mark task done in the store
2. Emit `task:completed` event
3. Homepage updates via SSE

---

### Phase 4 — Cross-Agent Task Creation

Agents can create tasks for other agents or the human:
- `task_create("Update trailer wiring diagram", assignee: "akron", priority: "medium")`
- `task_create("Review vendor pricing", assignee: "human", priority: "high")`

The human creates tasks from the homepage UI — type title, pick assignee, optional priority/context.

---

## Open Questions

1. **Task vs. Taskwarrior.** Agents already have `tw` (Taskwarrior) for their own task management. Is this a replacement, a layer on top, or parallel? The key difference: `tw` is per-agent workspace files. This is a shared system with UI visibility.

2. **Task granularity.** Are these high-level objectives ("dig into inventory") or specific actions ("run `tw project:craft.leather`")? Probably the former — agents decompose into their own subtasks.

3. **Persistence vs. ephemerality.** Do completed tasks stay visible (like a done list) or archive after N days? Probably show last 5 completed, archive the rest.

4. **Agent initiative.** Can agents self-assign tasks they discover? ("I noticed the belt inventory is low" → creates task for Demi.) Or only human-created?

5. **Notification.** When a task is assigned to an agent, does it get notified immediately (sessions_send) or just sees it next time it wakes up?

6. **Homepage as default.** Does the app always open to Home, or remember last view? Probably always Home — that's the point.

---

## Non-Goals

- Kanban board / drag-and-drop columns (too heavy for this use case)
- Due dates / scheduling (keep it simple — open or done)
- Subtasks / dependencies (agents handle their own decomposition)
- Comments / discussion threads on tasks (that's what chat is for)
- External integrations (GitHub Issues, Linear, etc.)

---

## Estimated Effort

| Phase | Scope | Effort |
|-------|-------|--------|
| 1 | Task backend (schema, API, built-in tools) | Large — new store table, 4 API routes, 3 built-in tools |
| 2 | Homepage UI (view, sections, SSE wiring) | Large — new view, task board component, activity feed |
| 3 | Agent task injection (context stage) | Medium — pipeline modification, prompt formatting |
| 4 | Cross-agent task creation | Small — already possible via Phase 1 tools |

Total: ~2-3 focused sessions. Phase 1 is the foundation everything else depends on.
