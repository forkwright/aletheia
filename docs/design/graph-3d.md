# 3D Force-Directed Knowledge Graph

Architecture for the Dioxus desktop 3D graph visualization.

## Scope

The 3D force-directed graph is a Dioxus desktop app feature, not a TUI feature. The TUI provides text/table-based entity browsing; the desktop app provides spatial graph exploration.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Dioxus Desktop App (theatron/desktop)                   │
│                                                          │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐ │
│  │ Graph3DView  │   │ ForceEngine  │   │ WgpuRenderer │ │
│  │  (Dioxus     │──▸│  (sim loop)  │──▸│  (GPU draw)  │ │
│  │   component) │   │              │   │              │ │
│  └──────┬───────┘   └──────────────┘   └──────────────┘ │
│         │                                                │
│  ┌──────▼───────┐   ┌──────────────┐                    │
│  │ GraphState   │◂──│ ApiClient    │                    │
│  │  (entities,  │   │  (REST fetch │                    │
│  │   relations, │   │   entities,  │                    │
│  │   scores)    │   │   relations) │                    │
│  └──────────────┘   └──────────────┘                    │
└──────────────────────────────────────────────────────────┘
```

### Data flow

1. `ApiClient` fetches entities, relationships, and graph scores from the backend API
2. `GraphState` holds the graph topology, PageRank scores, and Louvain community assignments
3. `ForceEngine` runs a force-directed layout simulation each frame
4. `WgpuRenderer` draws nodes, edges, labels, and community clouds to a GPU surface
5. `Graph3DView` Dioxus component owns the render surface, handles input events (pan, zoom, rotate, select)

## Force simulation

The simulation applies four forces per tick:

| Force | Purpose | Parameters |
|-------|---------|------------|
| Repulsion | Prevent node overlap | Coulomb constant, min distance |
| Attraction | Pull connected nodes together | Spring constant per edge weight |
| Centering | Keep the graph centered in view | Strength toward origin |
| Damping | Converge to stable layout | Velocity decay factor |

Nodes have mass proportional to their PageRank score. Higher-PageRank nodes resist movement and attract neighbors more strongly.

### Tick budget

Target 60fps. The simulation runs on a background thread, producing position snapshots that the renderer consumes. If a tick exceeds 2ms, reduce iteration count or switch to Barnes-Hut approximation for repulsion (O(n log n) instead of O(n^2)).

## Visual encoding

### Nodes

- **Size**: proportional to PageRank (min 4px, max 24px)
- **Color**: mapped to entity type (person, tool, project, concept, etc.)
- **Label**: entity name, rendered as billboard sprites facing the camera
- **Selection**: highlighted outline, expanded info card on click

### Edges

- **Width**: proportional to relationship weight
- **Color**: mapped to relationship type
- **Direction**: arrowhead at destination end
- **Label**: relationship type on hover

### Communities

- **Semi-transparent spheres** enclose each Louvain community cluster
- Sphere radius = bounding radius of member nodes + padding
- Sphere color = community color (generated from community ID via golden-angle hue spacing)
- Opacity: 0.1 base, 0.3 when any member node is hovered

### Z-axis

- Primary layout is XY (force-directed)
- Z-axis encodes temporal depth: recently-updated entities float higher, stale entities sink
- Z range is compressed (max 20% of XY extent) to prevent visual flattening

## Camera

- **Default position**: elevated 45-degree angle looking at graph centroid
- **Navigation**: orbit (left drag), pan (right drag), zoom (scroll wheel)
- **Focus**: double-click entity to animate camera to face it, with depth-of-field blur on background
- **Reset**: keyboard shortcut to return to default view

## Rust crate candidates

| three.js concept | Rust replacement | Notes |
|-------------------|------------------|-------|
| three.js renderer | `wgpu` | Low-level GPU abstraction, cross-platform |
| Scene graph | `bevy_ecs` or custom | ECS for node/edge/label entities |
| 3d-force-graph | Custom Rust impl | Port the force simulation; ~200 lines |
| OrbitControls | `dolly` or custom | Camera rig with orbit/pan/zoom |
| CSS2DRenderer (labels) | `wgpu` billboard quads | Render text to texture atlas, draw as quads |
| TransformControls | `egui` overlay | For debug/inspector UI |
| Stats.js | `tracing` + `egui` | Frame time overlay |

### Alternative: bevy_egui stack

For faster iteration, the graph could use `bevy` with `bevy_egui` for UI overlay:

- `bevy` handles the render loop, scene graph, and camera
- `bevy_egui` provides immediate-mode UI panels for entity inspection
- Custom `bevy` systems implement the force simulation
- `bevy_mod_picking` handles ray-cast entity selection

Trade-off: larger binary and dependency surface, but faster time-to-interactive.

## Integration with theatron/desktop

The desktop app (Dioxus) embeds the graph as a custom element:

1. Dioxus component creates a `wgpu::Surface` from the window handle
2. Graph renderer runs on its own thread, receives state updates via channel
3. Mouse/keyboard events flow from Dioxus to the graph via event channel
4. Selection events flow back: user selects entity in graph, desktop app shows detail panel

The graph component is a leaf in the Dioxus component tree. It does not use Dioxus virtual DOM for rendering; it directly owns a GPU surface.

## Performance targets

| Metric | Target |
|--------|--------|
| Entities | 10,000 nodes without frame drops |
| Edges | 50,000 edges with LOD culling |
| Frame rate | 60fps on integrated GPU |
| Initial layout convergence | <2s for 1,000 nodes |
| Memory | <100MB for 10k node graph |

For graphs exceeding 10k nodes, implement level-of-detail: collapse distant communities into single super-nodes, expand on zoom.

## Phased delivery

| Phase | Scope |
|-------|-------|
| 1 | 2D force layout in a Dioxus canvas element (no GPU) |
| 2 | wgpu renderer with 3D positioning and basic camera |
| 3 | Community spheres, z-axis temporal encoding |
| 4 | Performance: Barnes-Hut, LOD, instanced rendering |
