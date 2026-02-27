# Spec 43: A2UI Live Canvas — Agent-Writable Dynamic UI Surface

**Status:** Draft
**Origin:** Issue #319
**Author:** Syn
**Date:** 2026-02-28
**Spec:** 43

---

## Problem

Everything agents communicate goes through the conversation stream as text/markdown. A 20-step plan rendered as markdown in chat works but is not interactive, not live-updating, and competes with the conversation for attention. Agents have no "whiteboard" — no surface for visualizations, dashboards, progress indicators, or structured data that updates in real time independently of the chat.

## Vision

A separate UI surface that agents write to directly via structured payloads — decoupled from the conversation stream. The agent renders visualizations, dashboards, progress indicators, and structured data without polluting the chat.

Directly useful for: Dianoia planning visualization, competence dashboards, long-running task progress, memory graph exploration, cost tracking.

---

## Architecture

### Server Component

A lightweight SSE server (integrated into pylon as `/canvas/*` routes, or standalone on port 18790) that:
- Accepts JSONL payloads from agents via HTTP POST
- Broadcasts surface updates to connected WebUI clients via SSE
- Maintains current surface state in memory (for reconnecting clients)

```
POST /api/canvas/surface        → create or update a surface
POST /api/canvas/surface/:id    → update existing surface
DELETE /api/canvas/surface/:id  → remove surface
GET /api/canvas/events          → SSE stream of surface updates
GET /api/canvas/surfaces        → current state of all surfaces
```

### Surface Types

```typescript
type CanvasSurface =
  | { type: "progress"; id: string; title: string; steps: ProgressStep[] }
  | { type: "table"; id: string; title: string; columns: string[]; rows: unknown[][] }
  | { type: "metrics"; id: string; title: string; metrics: Metric[] }
  | { type: "markdown"; id: string; title: string; content: string }
  | { type: "graph"; id: string; title: string; nodes: Node[]; edges: Edge[] }
```

### Agent Tool

```typescript
// organon/built-in/canvas.ts
{
  name: "canvas_update",
  description: "Write structured data to the live canvas — visible to the user in real-time without appearing in the conversation.",
  input_schema: {
    surface_id: string,      // unique identifier for this surface
    surface_type: string,    // "progress" | "table" | "metrics" | "markdown" | "graph"
    title: string,
    data: object,            // surface-type-specific payload
    expires_minutes: number  // auto-remove after N minutes (default: 60)
  }
}
```

### WebUI Integration

A collapsible canvas panel in the WebUI (right sidebar or bottom panel) that renders active surfaces. Updates live via SSE. Empty canvas = panel hidden. Agent creates a surface = panel appears automatically.

---

## Concrete Use Cases

**Dianoia plan progress:**
```json
{
  "surface_type": "progress",
  "title": "Restructure Workspace (Phase 3)",
  "data": {
    "steps": [
      { "label": "Create deploy/ directory", "status": "complete" },
      { "label": "Move ergon-config/ contents", "status": "running", "elapsed": "12s" },
      { "label": "Update config paths", "status": "pending" }
    ]
  }
}
```

**Competence dashboard:**
```json
{
  "surface_type": "metrics",
  "title": "Agent Competence Scores",
  "data": {
    "metrics": [
      { "label": "sql", "value": 0.82, "trend": "up" },
      { "label": "roi", "value": 0.74, "trend": "stable" },
      { "label": "infra", "value": 0.61, "trend": "up" }
    ]
  }
}
```

---

## Relationship to Existing UI

The canvas is additive — it doesn't replace the conversation view. It's the agent's "whiteboard" alongside the conversation "notepad." The WebUI serves both from the same origin.

---

## Phases

### Phase 1: Server + Tool
- Canvas routes in pylon (or standalone server)
- `canvas_update` tool registered in organon
- In-memory surface state with TTL expiration
- SSE event stream for connected clients

### Phase 2: WebUI Panel
- Collapsible canvas panel (right sidebar or bottom panel)
- Renderers for: progress, table, metrics, markdown
- Auto-show on first surface, auto-hide when all expire
- Reconnect recovery (fetch current state on reconnect)

### Phase 3: Graph Surface + Interactivity
- Graph renderer (nodes + edges) for memory/knowledge visualization
- Click-to-expand on surface items
- Surface pinning (prevent auto-expire)

---

## Open Questions

- Integrated into pylon or separate process? Pylon is simpler but adds surface area to the main server.
- Surface persistence: in-memory only, or backed by sessions.db for history?
- Multi-agent surfaces: can multiple agents write to the same surface ID?
- Canvas position: right sidebar vs. bottom panel vs. user-configurable?

---

## Acceptance Criteria

- [ ] Canvas server routes operational (create, update, delete, SSE stream)
- [ ] `canvas_update` tool available to all nous
- [ ] WebUI renders canvas panel when surfaces are active
- [ ] Progress, table, metrics, and markdown surface types implemented
- [ ] Surfaces expire and auto-remove (configurable TTL)
- [ ] Canvas state recovered on client reconnect
