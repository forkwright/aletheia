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
- "Is the system's understanding of X getting stale?"
- "Does Demiurge know about this, or do I need to tell him?"
- "What did we discuss last Tuesday?"
- "Which memories are things I said vs. things the system inferred?"
- "What needs cleanup?"

It should be three things: a **knowledge browser** (explore what the system knows), a **quality control dashboard** (find and fix what's wrong), and a **pre-conversation tool** (check context before asking).

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

### Memory auditing

The graph becomes the **quality control dashboard** for the entire memory layer. Right now there's no way to see what the system believes about you — what's stale, what's wrong, what contradicts what.

**Visual confidence decay.** Node opacity and saturation map to confidence score. A node last reinforced yesterday is vivid; one untouched for 3 months is ghostly. At a glance you see what the system is confident about and what's fading. Edges work the same way — a relationship that hasn't been referenced fades toward invisible.

```
[Vivid]  ●━━━━● Recently reinforced, high confidence
[Normal] ●────● Active, moderate confidence  
[Faded]  ○┄┄┄┄○ Stale, low confidence — needs review or removal
[Red]    ◉╌╌╌╌◉ Conflicting memories detected
```

**Contradiction detection.** When two memories about the same entity conflict (e.g., "oil change interval is 10K" vs. "oil change interval is 7.5K"), the node gets a red conflict indicator. Clicking it shows both memories side-by-side with timestamps, so you can resolve which is correct. This surfaces errors that would otherwise silently degrade agent responses.

**Bulk operations.** Select multiple faded nodes → "Archive all" or "Re-verify." Select a cluster of near-duplicate nodes → "Merge all into one." The graph makes batch cleanup visual and fast instead of hunting through text lists.

**Health summary bar.** A persistent bar at the top of the graph view:

```
Memories: 847 total · 12 conflicting · 43 stale (>30 days) · 3 flagged
[Review conflicts] [Archive stale] [Show flagged]
```

This gives you a single-glance system health read without needing to explore individual nodes.

### Conversation archaeology

Every memory traces back to a conversation. The graph should surface *when* things were learned and from what context.

**Timeline mode.** A slider or scrubber at the bottom of the graph that controls a temporal filter:

```
[|━━━━━━━━━━━━━━━━━━━━━━━━━━|]
Jan 28          Feb 10          Feb 19
           ▲ "DEF delete discussed"
```

Drag it to see the graph at any point in time — which nodes existed, what was connected to what. Animate it to watch the knowledge graph grow over days/weeks. This answers "what did we talk about last Tuesday?" visually.

**Session provenance.** Each memory in a node card links back to the session where it was learned. Click "3 sessions" on a connection → see a list of conversations where that relationship was discussed, with timestamps and snippets. This is conversation search, but spatial — you start from an entity and trace back to the conversations, instead of searching conversations and hoping to find the entity.

**"Learned" vs. "Stated" markers.** Distinguish between memories extracted from conversation (inferred by the system) and facts explicitly stated by the user. Inferred memories are more likely to be wrong. Visual indicator: solid border = user-stated, dashed border = system-inferred.

```
┌─ Memories ─────────────────────────────┐
│ ▐ DEF system rebuilt Feb 2026          │  ← solid: you said this
│   (stated, 0.98, 2 weeks ago)          │
│ ┊ Turbo might need replacement         │  ← dashed: system inferred
│   (inferred, 0.71, 1 week ago)         │
└────────────────────────────────────────┘
```

### Cross-agent visibility

Right now there's no way to see what each agent knows. The graph can partition or color by agent domain to reveal knowledge distribution and gaps.

**Agent domain overlay.** Toggle an overlay that colors nodes by which agent(s) have memories about them:

```
🔵 Syn    🟠 Demiurge    🟢 Syl    🔴 Akron    ⚪ Unowned
```

A node colored with multiple agents' colors (split or blended) means multiple agents have context on that entity. A node that's ⚪ means nobody has specific domain knowledge — it's in the shared graph but no agent has claimed expertise.

**Knowledge gap detection.** The overlay immediately reveals imbalances: "Demiurge knows a lot about leather but nothing about thread suppliers" or "Akron has zero memories about the radio install, even though that's his domain." These gaps are actionable — you can tell an agent to pay attention to something, or realize a domain assignment is wrong.

**Agent filter pills.** Like community pills, but for agents:

```
[All] [🔵 Syn] [🟠 Demiurge] [🟢 Syl] [🔴 Akron]
```

Select one to see only that agent's knowledge. Select two to see overlap. This answers "does Demiurge know about X?" before you ask Demiurge about X.

### Drift detection

The graph becomes **proactive** — it tells you what needs attention rather than waiting for you to notice.

**Staleness alerts.** Nodes that haven't been referenced or reinforced in a configurable period (default 30 days) get flagged automatically. The health bar surfaces these. But beyond just flagging, drift detection looks for patterns:

- **Orphaned clusters.** A group of interconnected nodes that has zero connections to the rest of the graph. Likely an abandoned topic or a data quality issue.
- **Confidence divergence.** An entity where some memories are high-confidence and others are low — the system's understanding is fragmenting.
- **Temporal clusters.** A burst of nodes all created in one session, never referenced again. Probably a one-off conversation that generated noise rather than knowledge.

**Decay timeline.** For any node, show a sparkline of its confidence over time. Is it stable? Declining? Recently reinforced? This tells you whether the system's understanding is improving or degrading.

```
Cummins ISB 6.7  ▁▂▃▅▇▇█▇▆▅  (trending down — last update 12 days ago)
Leather tooling  ▁▁▂▃▄▅▆▇██  (trending up — active domain)
```

**Suggested actions.** Based on drift patterns, the graph suggests specific actions:

- "5 nodes in 'Truck' community haven't been referenced in 30+ days. Review?"
- "2 conflicting memories about 'oil change interval'. Resolve?"
- "'Cody' and 'Cody Kickertz' appear to be the same entity. Merge?"

These appear as a notification badge on the graph tab: `Graph (3)` — three items need attention.

### Context before conversation

The graph serves as a **pre-conversation lookup** — check what the system already knows before starting a conversation with an agent.

**"What does the system know about X?" flow:**

1. Open graph tab
2. Search "cummins" or click the Truck community
3. See all memories, connections, confidence scores, which agents know what
4. Decide: "It already knows about the DEF delete, I can just ask about the turbo"
5. Switch to chat with full context awareness

This eliminates the "let me tell you the whole backstory" preamble that wastes tokens and time. You know what context exists, so you can reference it directly.

**Quick-lookup from chat.** A future integration: type `@graph cummins` in chat to get an inline summary of the graph node without switching tabs. Or hover over an entity mention in a chat message to see a tooltip with the graph card.

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

### Foundation (Performance)

| Phase | Effort | Impact |
|-------|--------|--------|
| **1: 2D default + 3D toggle** | Medium | Immediate performance win, better usability |
| **2: Lazy-load 3D** | Small | Bundle size reduction |
| **3: Progressive loading** | Medium | Fast initial render |

### Making It Useful (Knowledge Browser)

| Phase | Effort | Impact |
|-------|--------|--------|
| **4: Named communities** | Small | Makes the pills actually useful |
| **5: Semantic node cards** | Medium | The graph becomes a knowledge browser |
| **6: Search overhaul** | Medium | Answering real questions |
| **7: Relationship filtering** | Small | Edge-type exploration |

### Value Add (System Intelligence)

| Phase | Effort | Impact |
|-------|--------|--------|
| **8: Memory auditing** | Medium | Visual confidence decay, contradiction detection, bulk operations, health bar |
| **9: Conversation archaeology** | Medium | Timeline mode, session provenance, stated vs. inferred markers |
| **10: Cross-agent visibility** | Medium | Agent domain overlay, knowledge gap detection, agent filter pills |
| **11: Drift detection** | Medium | Staleness alerts, orphaned clusters, decay sparklines, suggested actions |
| **12: Edit capabilities** | Medium | Rename, merge, delete, flag — closes the feedback loop |
| **13: Context before conversation** | Small | Pre-conversation lookup, future `@graph` inline integration |

---

## Testing

### Foundation
- **Performance:** Graph tab renders first meaningful content in <1 second on a standard laptop. 200 nodes at 60fps in 2D mode.
- **3D lazy load:** Opening the graph tab doesn't load Three.js. Switching to 3D mode triggers the import. Switching back to 2D disposes the 3D renderer.
- **Progressive load:** Initial render shows top 20 nodes. Within 3 seconds, 200 nodes are visible. No layout jumps — new nodes animate into position.
- **Mobile:** Graph tab is usable on phone/tablet. Pan, zoom, tap-to-select all work in 2D.

### Knowledge Browser
- **Named communities:** Every community pill shows a descriptive name, not a number.
- **Node cards:** Click any node → detail card shows memories, connections, confidence scores, and source attribution. Memory fetch completes in <500ms.
- **Search:** Searching "cummins" highlights the node, centers it, and shows a results list with related memories. Filter by community, relationship type, or agent.
- **Relationship filtering:** Toggle edge types on/off → layout updates, only selected relationship types visible.

### System Intelligence
- **Memory auditing:** Nodes visually reflect confidence (opacity/saturation). Conflicting memories show red indicator. Health bar shows totals (conflicts, stale, flagged). Bulk select → archive/merge works.
- **Conversation archaeology:** Timeline scrubber filters graph to a date range. Click a memory → links to the originating session. Stated vs. inferred memories are visually distinct.
- **Cross-agent visibility:** Agent overlay colors nodes by domain owner. Agent filter pills show/hide per-agent knowledge. Knowledge gaps are visually obvious (uncolored nodes in an agent's domain).
- **Drift detection:** Stale nodes (>30 days) are auto-flagged. Orphaned clusters are identified. Decay sparklines show confidence trends. Suggested actions appear as notification badge on graph tab.
- **Edit:** Merge two nodes → graph updates, both names resolve to one node. Delete a node → it disappears with its orphaned edges. Flag a node → warning indicator visible.
- **Context lookup:** Search an entity → see full system knowledge before starting a conversation about it.

---

## Success Criteria

- **The graph answers questions.** A user can find what the system knows about any topic in <10 seconds.
- **Performance:** No more fan spin or battery drain from the graph tab.
- **Memory quality improves.** Contradictions, staleness, and duplicates are surfaced and resolved through the graph interface. Measurable: conflict count decreases over time, average confidence increases.
- **Pre-conversation context.** Users check the graph before asking agents about complex topics at least occasionally. Token usage for backstory decreases.
- **Cross-agent awareness.** Knowledge gaps between agents are visible and actionable. No more "I didn't know Akron didn't know about X."
- **Actually used.** The graph tab goes from "I looked at it once" to a regular part of the workflow — checking what the system knows, correcting mistakes, exploring connections, auditing memory quality.
