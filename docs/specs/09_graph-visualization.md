# Spec: Graph Visualization — From Demo to Tool

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

The graph visualization tab in the webchat renders the Neo4j knowledge graph as a 3D force-directed network using `3d-force-graph` (Three.js). It looks impressive but has real problems:

### Performance

- **Initial load is expensive.** Fetching 200 nodes + edges from Neo4j via the Python sidecar → gateway proxy takes 2-5 seconds. Then the force simulation runs 100 warmup ticks + 3 seconds of cooldown animation before the graph is stable enough to interact with. Total time-to-interactive: 5-8 seconds.
- **WebGL is heavy.** Three.js + the force simulation consume significant GPU/CPU. On a laptop or phone, the graph tab causes noticeable fan spin and battery drain. The community cloud system (transparent spheres per cluster) adds continuous per-frame computation via `onEngineTick`.
- **Scales poorly.** At 200 nodes it's manageable. The "All nodes" mode on a real graph (1000+ nodes) is unusable — the force simulation never converges, the frame rate drops, and the visualization becomes a jittery ball of dots.
- **Always 3D.** 3D force-directed graphs are visually dramatic but functionally inferior to 2D for most graph analysis tasks. Rotating and navigating 3D space requires constant camera management. Nodes hide behind other nodes. Labels are unreadable at most angles. The third dimension adds zero information — it's cosmetic.

### Utility

- **Read-only.** You can click nodes to see their connections in a small info panel, but that's it. You can't edit, delete, merge, or annotate nodes or edges.
- **No semantic context.** Nodes are labeled with entity names but there's no way to see *what* the system knows about that entity — the memories, facts, and conversations associated with it.
- **Community pills are opaque.** The toolbar shows community IDs (0, 1, 2...) but doesn't tell you what those communities represent. Is community 3 "work stuff" or "truck maintenance"? You have to click through to find out.
- **Search is basic.** The search bar does substring matching on node IDs. No filtering by relationship type, community, entity type, or memory content.

### What it should be

The graph view should answer questions like:
- "What does the system know about my truck?"
- "What decisions have I made about the MBA project?"
- "Are there contradicting memories about X?"
- "When did the system learn Y?"
- "What's connected to Z that I might have forgotten about?"

Currently it answers: "Look, nodes with lines."

---

## Design

### Switch to 2D by default

Replace the 3D force-directed graph with a 2D layout as the primary view. Benefits:

- **Readable labels.** Every node label is always visible and oriented correctly.
- **Faster.** No WebGL overhead for the default view. Canvas 2D rendering is dramatically lighter.
- **Better navigation.** Pan and zoom vs. 3D camera rotation. Familiar from every map/diagram tool.
- **Mobile-friendly.** Touch pan/zoom works naturally in 2D.

**Implementation:** Use `2d-force-graph` (same author as `3d-force-graph`, similar API) or `d3-force` directly for maximum control. The 3D view can remain as a toggle for when you want the dramatic visualization, but 2D is the default workhorse.

```svelte
<div class="view-toggle">
  <button class:active={viewMode === '2d'} onclick={() => viewMode = '2d'}>2D</button>
  <button class:active={viewMode === '3d'} onclick={() => viewMode = '3d'}>3D</button>
</div>
```

### Lazy-load the 3D renderer

Three.js is ~150KB+ gzipped. The 3D view should be dynamically imported only when the user switches to 3D mode:

```svelte
{#if viewMode === '3d'}
  {#await import('./Graph3D.svelte') then { default: Graph3D }}
    <Graph3D data={graphData} />
  {/await}
{:else}
  <Graph2D data={graphData} />
{/if}
```

This means the graph tab loads fast (2D only) and 3D is opt-in.

### Progressive loading

Instead of fetching 200 nodes upfront, load progressively:

1. **Immediate:** Show the top 20 nodes by pagerank with their connections. This loads in <500ms and gives an instant overview of the most important entities.
2. **Background:** Fetch the next 80 nodes and animate them into the graph.
3. **On-demand:** Additional nodes load when the user pans, zooms, or searches.

The force simulation runs incrementally — new nodes are added to the existing layout without resetting the entire simulation.

### Semantic node cards

Clicking a node opens a rich detail card instead of the current minimal info panel:

```
┌─────────────────────────────────────┐
│  🔵 Cummins ISB 6.7               │
│  Community: Truck & Vehicle         │
│  PageRank: 0.0234 · 12 connections  │
│                                     │
│  ┌─ Memories ────────────────────┐  │
│  │ • Turbo needs inspection       │  │
│  │   (confidence: 0.92, 3 days)   │  │
│  │ • DEF system rebuilt Feb 2026  │  │
│  │   (confidence: 0.98, 2 weeks)  │  │
│  │ • Oil change interval: 10K mi  │  │
│  │   (confidence: 0.85, 1 month)  │  │
│  └────────────────────────────────┘  │
│                                     │
│  ┌─ Connections ─────────────────┐  │
│  │ PART_OF → Honda Passport       │  │
│  │ MAINTAINED_BY → Cody           │  │
│  │ DISCUSSED_IN → 3 sessions      │  │
│  └────────────────────────────────┘  │
│                                     │
│  [Edit] [Merge] [Delete] [Sessions] │
└─────────────────────────────────────┘
```

**Where the memories come from:** When a node is selected, query Qdrant for memories that mention the entity name. This is a secondary fetch (not loaded for all nodes upfront).

### Named communities

Replace numeric community IDs with auto-generated labels based on the highest-pagerank entities in each community:

| Before | After |
|--------|-------|
| Community 0 | Aletheia & Infrastructure |
| Community 3 | Truck & Vehicle |
| Community 5 | Leather & Craft |
| Community 7 | Family & Home |

The label is generated from the top 2-3 nodes in each community by pagerank, combined with any shared labels (entity types). Store the label in community metadata so it doesn't need to be recalculated on every load.

The community pills in the toolbar now show these names:

```
[All] [Aletheia] [Truck] [Leather] [Family] [Work] [MBA] ...
```

### Relationship-type filtering

Add filter controls for edge types:

```
Show: [✓ KNOWS] [✓ PART_OF] [  MENTIONED_IN] [✓ DEPENDS_ON] ...
```

Toggling relationship types on/off filters which edges are visible and re-runs the layout. This lets you answer questions like "show me only the DEPENDS_ON relationships" to understand system architecture, or "show me only FAMILY_OF" to see the family tree.

### Search overhaul

Replace the simple substring search with a multi-faceted search:

```
Search: [cummins                    ] [Nodes ▼]
        community:truck engine oil
        type:entity relationship:PART_OF
```

- **Text search:** matches node names, memory content, relationship labels
- **Filters:** `community:X`, `type:X` (entity label), `relationship:X`
- **Results list:** shows matching nodes below the search bar with snippets, click to focus
- **Keyboard navigation:** arrow keys to move through results, Enter to select

### Edit capabilities

The detail card for each node includes edit actions:

- **Edit** — Rename the entity, change its community assignment, edit its properties
- **Merge** — Combine two nodes that represent the same entity (e.g., "Cody" and "Cody Kickertz")
- **Delete** — Remove the node and optionally its connections. Confirmation required.
- **Flag** — Mark as incorrect, needs review, or deprecated. Flagged nodes show a warning indicator.

These actions call existing sidecar endpoints (or new ones as needed) and refresh the graph.

### Performance budget

| Metric | Current | Target |
|--------|---------|--------|
| Time to first render | 5-8s | <1s (2D with top 20 nodes) |
| Time to full graph (200 nodes) | 5-8s | <3s (progressive load) |
| Frame rate (200 nodes, 2D) | N/A (3D only) | 60fps |
| Frame rate (200 nodes, 3D) | 30-60fps | 60fps (opt-in only) |
| Bundle size impact (graph tab) | ~250KB (Three.js always loaded) | ~30KB (2D default), +200KB (3D lazy) |
| Memory usage | 100MB+ (Three.js + textures) | <30MB (2D Canvas) |

---

## Implementation Order

| Phase | Effort | Impact |
|-------|--------|--------|
| **1: 2D default + 3D toggle** | Medium | Immediate performance win, better usability |
| **2: Lazy-load 3D** | Small | Bundle size reduction |
| **3: Named communities** | Small | Makes the pills actually useful |
| **4: Progressive loading** | Medium | Fast initial render |
| **5: Semantic node cards** | Medium | The graph becomes a knowledge browser |
| **6: Search overhaul** | Medium | Answering real questions |
| **7: Edit capabilities** | Medium | User can correct the graph |
| **8: Relationship filtering** | Small | Edge-type exploration |

---

## Testing

- **Performance:** Graph tab renders first meaningful content in <1 second on a standard laptop. 200 nodes at 60fps in 2D mode.
- **3D lazy load:** Opening the graph tab doesn't load Three.js. Switching to 3D mode triggers the import. Switching back to 2D disposes the 3D renderer.
- **Progressive load:** Initial render shows top 20 nodes. Within 3 seconds, 200 nodes are visible. No layout jumps — new nodes animate into position.
- **Node cards:** Click any node → detail card shows memories, connections, and edit actions. Memory fetch completes in <500ms.
- **Named communities:** Every community pill shows a descriptive name, not a number.
- **Search:** Searching "cummins" highlights the node, centers it, and shows a results list with related memories.
- **Edit:** Merge two nodes → graph updates, both names resolve to one node. Delete a node → it disappears with its orphaned edges.
- **Mobile:** Graph tab is usable on phone/tablet. Pan, zoom, tap-to-select all work in 2D.

---

## Success Criteria

- **The graph answers questions.** A user can find what the system knows about any topic in <10 seconds.
- **Performance:** No more fan spin or battery drain from the graph tab.
- **Actually used.** The graph tab goes from "I looked at it once" to a regular part of the workflow — checking what the system knows, correcting mistakes, exploring connections.
