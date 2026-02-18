<script lang="ts">
  import { onMount } from "svelte";
  import * as THREE from "three";
  import {
    getGraphData, getLoading, getError, getSelectedNodeId,
    getSelectedNode, getNodeEdges, getConnectedNodes, getCommunityIds,
    getHighlightedCommunity, getSearchQuery, getLoadedMode, getLoadedLimit,
    getTotalNodes,
    setSelectedNodeId, setHighlightedCommunity, setSearchQuery,
    loadGraph,
  } from "../../stores/graph.svelte";

  let container: HTMLDivElement;
  let graph: any = null;
  let hoverNode: string | null = $state(null);

  // Community color palette — 20 distinct hues
  const PALETTE = [
    "#58a6ff", "#3fb950", "#d29922", "#f85149", "#bc8cff",
    "#f778ba", "#79c0ff", "#56d4dd", "#e3b341", "#db6d28",
    "#8b949e", "#7ee787", "#a5d6ff", "#ffa657", "#ff7b72",
    "#d2a8ff", "#ffd8b5", "#89dceb", "#f9e2af", "#a6e3a1",
  ];

  function communityColor(community: number): string {
    if (community < 0) return "#30363d";
    return PALETTE[community % PALETTE.length]!;
  }

  function pagerankSize(pr: number): number {
    const clamped = Math.max(pr, 0.0001);
    const scaled = (Math.log(clamped) + 10) / 10;
    return Math.max(2, Math.min(14, scaled * 12));
  }

  function edgeColor(relType: string): string {
    const social = ["KNOWS", "LIVES_WITH", "FAMILY_OF", "WORKS_WITH", "COMMUNICATES_WITH"];
    const structural = ["PART_OF", "CONTAINS", "DEPENDS_ON", "INSTANCE_OF", "LOCATED_AT"];
    const temporal = ["PRECEDES", "FOLLOWS", "OCCURRED_AT", "MENTIONS"];

    if (social.includes(relType)) return "rgba(63, 185, 80, 0.35)";
    if (structural.includes(relType)) return "rgba(88, 166, 255, 0.35)";
    if (temporal.includes(relType)) return "rgba(210, 153, 34, 0.35)";
    return "rgba(139, 148, 158, 0.25)";
  }

  // --- Community Cloud System ---

  const CLOUD_GEOMETRY = new THREE.SphereGeometry(1, 24, 16);
  const cloudMeshes = new Map<number, THREE.Mesh>();
  const cloudLabels = new Map<number, THREE.Sprite>();
  let cloudScene: THREE.Scene | null = null;

  function createCloudMaterial(color: string, opacity: number): THREE.MeshBasicMaterial {
    return new THREE.MeshBasicMaterial({
      color: new THREE.Color(color),
      transparent: true,
      opacity,
      depthWrite: false,
      side: THREE.BackSide,
    });
  }

  function createLabelSprite(text: string, color: string): THREE.Sprite {
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d")!;
    canvas.width = 512;
    canvas.height = 64;
    ctx.font = "bold 28px system-ui, -apple-system, sans-serif";
    ctx.fillStyle = color;
    ctx.globalAlpha = 0.8;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const label = text.length > 30 ? text.slice(0, 27) + "..." : text;
    ctx.fillText(label, 256, 32);

    const texture = new THREE.CanvasTexture(canvas);
    const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthWrite: false });
    const sprite = new THREE.Sprite(material);
    sprite.scale.set(60, 8, 1);
    sprite.renderOrder = 10;
    return sprite;
  }

  function updateClouds(runtimeNodes: any[]) {
    if (!cloudScene) return;

    // Group runtime nodes by community (only those with positions)
    const byCommunity = new Map<number, any[]>();
    for (const node of runtimeNodes) {
      if (node.community == null || node.community < 0) continue;
      if (node.x == null) continue;
      const list = byCommunity.get(node.community) || [];
      list.push(node);
      byCommunity.set(node.community, list);
    }

    // Remove stale meshes
    for (const [cid, mesh] of cloudMeshes) {
      if (!byCommunity.has(cid)) {
        cloudScene.remove(mesh);
        (mesh.material as THREE.Material).dispose();
        cloudMeshes.delete(cid);
      }
    }
    for (const [cid, sprite] of cloudLabels) {
      if (!byCommunity.has(cid)) {
        cloudScene.remove(sprite);
        (sprite.material as THREE.SpriteMaterial).map?.dispose();
        sprite.material.dispose();
        cloudLabels.delete(cid);
      }
    }

    const hl = getHighlightedCommunity();

    for (const [cid, members] of byCommunity) {
      if (members.length < 3) continue;

      // Compute centroid
      let cx = 0, cy = 0, cz = 0;
      for (const m of members) { cx += m.x; cy += m.y; cz += m.z; }
      cx /= members.length; cy /= members.length; cz /= members.length;

      // Compute radius: 1.5 * stddev of distances from centroid
      let sumSqDist = 0;
      for (const m of members) {
        const dx = m.x - cx, dy = m.y - cy, dz = m.z - cz;
        sumSqDist += dx * dx + dy * dy + dz * dz;
      }
      const stddev = Math.sqrt(sumSqDist / members.length);
      const radius = Math.max(stddev * 1.5, 25);

      // Cloud mesh
      let mesh = cloudMeshes.get(cid);
      if (!mesh) {
        const color = communityColor(cid);
        mesh = new THREE.Mesh(CLOUD_GEOMETRY, createCloudMaterial(color, 0.06));
        mesh.renderOrder = -1;
        cloudScene.add(mesh);
        cloudMeshes.set(cid, mesh);
      }
      mesh.position.set(cx, cy, cz);
      mesh.scale.setScalar(radius);

      // Adjust opacity based on highlight
      const mat = mesh.material as THREE.MeshBasicMaterial;
      if (hl === null) {
        mat.opacity = 0.06;
      } else if (cid === hl) {
        mat.opacity = 0.12;
      } else {
        mat.opacity = 0.02;
      }

      // Label sprite — positioned above the cloud
      let sprite = cloudLabels.get(cid);
      if (!sprite) {
        // Use the highest-pagerank node name as label
        const topNode = members.reduce((a: any, b: any) =>
          (a.pagerank || 0) > (b.pagerank || 0) ? a : b
        );
        sprite = createLabelSprite(topNode.id, communityColor(cid));
        cloudScene.add(sprite);
        cloudLabels.set(cid, sprite);
      }
      sprite.position.set(cx, cy + radius * 0.9, cz);
      // Fade label with highlight
      (sprite.material as THREE.SpriteMaterial).opacity = hl === null ? 0.6 : (cid === hl ? 0.9 : 0.15);
    }
  }

  function disposeClouds() {
    if (!cloudScene) return;
    for (const [, mesh] of cloudMeshes) {
      cloudScene.remove(mesh);
      (mesh.material as THREE.Material).dispose();
    }
    for (const [, sprite] of cloudLabels) {
      cloudScene.remove(sprite);
      (sprite.material as THREE.SpriteMaterial).map?.dispose();
      sprite.material.dispose();
    }
    cloudMeshes.clear();
    cloudLabels.clear();
    cloudScene = null;
  }

  // --- Graph initialization ---

  function focusOnNode(nodeId: string) {
    if (!graph) return;
    const gd = graph.graphData();
    const runtimeNode = gd.nodes.find((n: any) => n.id === nodeId);
    if (!runtimeNode) return;

    const distance = 120;
    const distRatio = 1 + distance / Math.hypot(runtimeNode.x || 0, runtimeNode.y || 0, runtimeNode.z || 0);
    graph.cameraPosition(
      { x: (runtimeNode.x || 0) * distRatio, y: (runtimeNode.y || 0) * distRatio, z: (runtimeNode.z || 0) * distRatio },
      runtimeNode,
      1000,
    );
    setSelectedNodeId(nodeId);
  }

  function handleSearch(e: Event) {
    const input = e.target as HTMLInputElement;
    setSearchQuery(input.value);
    if (!input.value.trim()) return;
    const data = getGraphData();
    const q = input.value.toLowerCase();
    const match = data.nodes.find((n) => n.id.toLowerCase().includes(q));
    if (match) focusOnNode(match.id);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleSearch(e);
  }

  function handleModeChange(e: Event) {
    const mode = (e.target as HTMLSelectElement).value as "top" | "all";
    reloadGraph({ mode });
  }

  function handleLoadMore() {
    const newLimit = getLoadedLimit() + 100;
    reloadGraph({ mode: "top", limit: newLimit });
  }

  function handleCommunityClick(cid: number) {
    if (getHighlightedCommunity() === cid) {
      setHighlightedCommunity(null);
      reloadGraph();
    } else {
      setHighlightedCommunity(cid);
      reloadGraph({ mode: "community", community: cid });
    }
  }

  async function reloadGraph(params?: { mode?: "top" | "community" | "all"; limit?: number; community?: number }) {
    // Clear stale clouds before reload
    disposeClouds();

    await loadGraph(params);

    if (!graph || !container) return;

    const data = getGraphData();
    const graphInput = {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.source, target: e.target, rel_type: e.rel_type })),
    };
    graph.graphData(graphInput);

    // Re-init clouds with the fresh scene
    cloudScene = graph.scene();

    setTimeout(() => {
      if (graph) graph.zoomToFit(500, 50);
    }, 2000);
  }

  async function initGraph() {
    await loadGraph();
    if (!container) return;

    const ForceGraph3D = (await import("3d-force-graph")).default;
    const data = getGraphData();

    const graphInput = {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.source, target: e.target, rel_type: e.rel_type })),
    };

    graph = new ForceGraph3D(container)
      .backgroundColor("#0a0a0f")
      .showNavInfo(false)
      .width(container.clientWidth)
      .height(container.clientHeight)
      .graphData(graphInput)
      .nodeId("id")
      .nodeVal((node: any) => pagerankSize(node.pagerank || 0.001))
      .nodeColor((node: any) => {
        if (hoverNode && node.id === hoverNode) return "#ffffff";
        if (getSelectedNodeId() === node.id) return "#ffffff";
        const hl = getHighlightedCommunity();
        if (hl !== null && node.community !== hl) return "rgba(48, 54, 61, 0.4)";
        return communityColor(node.community ?? -1);
      })
      .nodeLabel((node: any) => {
        const pr = node.pagerank ? node.pagerank.toFixed(4) : "—";
        const comm = node.community >= 0 ? node.community : "—";
        return `<span style="color:#e6edf3;font-family:system-ui;font-size:12px">
          <b>${node.id}</b><br/>
          Community ${comm} · PR ${pr}
        </span>`;
      })
      .nodeOpacity(0.9)
      .linkSource("source")
      .linkTarget("target")
      .linkColor((link: any) => edgeColor(link.rel_type))
      .linkWidth(0.5)
      .linkOpacity(0.6)
      .linkDirectionalArrowLength(3)
      .linkDirectionalArrowRelPos(1)
      .linkLabel((link: any) => `<span style="color:#8b949e;font-size:11px">${link.rel_type}</span>`)
      .onNodeClick((node: any) => {
        setSelectedNodeId(node.id);
        focusOnNode(node.id);
      })
      .onNodeHover((node: any) => {
        hoverNode = node?.id ?? null;
        container.style.cursor = node ? "pointer" : "default";
      })
      .onBackgroundClick(() => {
        setSelectedNodeId(null);
        setHighlightedCommunity(null);
      })
      .warmupTicks(100)
      .cooldownTime(3000)
      .onEngineTick(() => {
        const gd = graph.graphData();
        updateClouds(gd.nodes);
      });

    // Init cloud system
    cloudScene = graph.scene();

    // Handle resize
    const resizeObserver = new ResizeObserver(() => {
      if (graph && container) {
        graph.width(container.clientWidth).height(container.clientHeight);
      }
    });
    resizeObserver.observe(container);

    // Fit to view after simulation settles
    setTimeout(() => {
      if (graph) graph.zoomToFit(500, 50);
    }, 3500);

    return () => {
      resizeObserver.disconnect();
      disposeClouds();
      CLOUD_GEOMETRY.dispose();
      if (graph) graph._destructor();
    };
  }

  onMount(() => {
    let cleanup: (() => void) | undefined;
    initGraph().then((c) => { cleanup = c; });
    return () => cleanup?.();
  });

  // Reactive helpers
  let selectedNode = $derived(getSelectedNode());
  let selectedEdges = $derived(getSelectedNodeId() ? getNodeEdges(getSelectedNodeId()!) : []);
  let connectedNodes = $derived(getSelectedNodeId() ? getConnectedNodes(getSelectedNodeId()!) : []);
  let communityIds = $derived(getCommunityIds());
</script>

<div class="graph-view">
  <div class="graph-toolbar">
    <input
      class="graph-search"
      type="text"
      placeholder="Search nodes..."
      value={getSearchQuery()}
      oninput={handleSearch}
      onkeydown={handleKeydown}
    />
    <div class="community-pills">
      <button
        class="pill"
        class:active={getHighlightedCommunity() === null}
        onclick={() => { setHighlightedCommunity(null); reloadGraph(); }}
      >All</button>
      {#each communityIds.slice(0, 12) as cid}
        <button
          class="pill"
          class:active={getHighlightedCommunity() === cid}
          style="--pill-color: {communityColor(cid)}"
          onclick={() => handleCommunityClick(cid)}
        >{cid}</button>
      {/each}
    </div>
    <div class="load-controls">
      <select class="mode-select" onchange={handleModeChange}>
        <option value="top" selected={getLoadedMode() === "top"}>Top nodes</option>
        <option value="all" selected={getLoadedMode() === "all"}>All nodes</option>
      </select>
      {#if getLoadedMode() === "top"}
        <button class="pill load-more" onclick={handleLoadMore} disabled={getLoading()}>
          + More
        </button>
      {/if}
    </div>
    <button
      class="pill refresh-btn"
      onclick={() => reloadGraph()}
      disabled={getLoading()}
      title="Reload graph data"
    >{getLoading() ? "..." : "Refresh"}</button>
    <span class="graph-stats">
      {getGraphData().nodes.length}{getTotalNodes() > 0 ? ` of ${getTotalNodes()}` : ""} nodes · {getGraphData().edges.length} edges
    </span>
  </div>

  <div class="graph-container" bind:this={container}>
    {#if getLoading()}
      <div class="graph-loading">Loading graph...</div>
    {/if}
    {#if getError()}
      <div class="graph-error">{getError()}</div>
    {/if}
  </div>

  {#if selectedNode}
    <div class="info-panel">
      <div class="info-header">
        <span class="info-dot" style="background: {communityColor(selectedNode.community)}"></span>
        <strong>{selectedNode.id}</strong>
        <span class="info-meta">
          Community {selectedNode.community >= 0 ? selectedNode.community : "—"} ·
          PR {selectedNode.pagerank.toFixed(4)}
        </span>
        <button class="info-close" onclick={() => setSelectedNodeId(null)}>✕</button>
      </div>
      {#if selectedNode.labels.length > 0}
        <div class="info-labels">
          {#each selectedNode.labels as label}
            <span class="label-tag">{label}</span>
          {/each}
        </div>
      {/if}
      {#if connectedNodes.length > 0}
        <div class="info-connections">
          {#each selectedEdges.slice(0, 20) as edge}
            <div class="connection-row">
              <span class="rel-type">{edge.rel_type}</span>
              <button
                class="connection-target"
                onclick={() => focusOnNode(edge.source === selectedNode?.id ? edge.target : edge.source)}
              >
                {edge.source === selectedNode?.id ? edge.target : edge.source}
              </button>
            </div>
          {/each}
          {#if selectedEdges.length > 20}
            <div class="connection-overflow">+{selectedEdges.length - 20} more</div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .graph-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    position: relative;
    background: #0a0a0f;
  }

  .graph-toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .graph-search {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 5px 10px;
    font-size: 13px;
    font-family: var(--font-sans);
    width: 180px;
    flex-shrink: 0;
  }
  .graph-search:focus {
    outline: none;
    border-color: var(--accent);
  }

  .community-pills {
    display: flex;
    gap: 4px;
    flex-wrap: nowrap;
  }

  .pill {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 11px;
    white-space: nowrap;
    cursor: pointer;
    transition: all 0.15s;
  }
  .pill:hover {
    border-color: var(--pill-color, var(--text-muted));
    color: var(--text);
  }
  .pill.active {
    background: var(--pill-color, var(--accent));
    border-color: var(--pill-color, var(--accent));
    color: #fff;
  }
  .pill:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .load-controls {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .mode-select {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    padding: 2px 6px;
    font-size: 11px;
    cursor: pointer;
  }

  .load-more {
    font-size: 10px;
    padding: 2px 6px;
  }

  .graph-stats {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .graph-container {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }
  .graph-container :global(canvas) {
    display: block;
  }

  .graph-loading, .graph-error {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    font-size: 14px;
    z-index: 10;
    pointer-events: none;
  }
  .graph-loading {
    color: var(--text-secondary);
  }
  .graph-error {
    color: var(--red);
  }

  .info-panel {
    position: absolute;
    bottom: 12px;
    left: 12px;
    width: 320px;
    max-height: 300px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
    overflow-y: auto;
    z-index: 20;
  }

  .info-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .info-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .info-header strong {
    font-size: 14px;
  }
  .info-meta {
    font-size: 11px;
    color: var(--text-muted);
  }
  .info-close {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    padding: 2px 6px;
  }
  .info-close:hover {
    color: var(--text);
  }

  .info-labels {
    display: flex;
    gap: 4px;
    margin-bottom: 8px;
  }
  .label-tag {
    background: var(--surface);
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .info-connections {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .connection-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .rel-type {
    color: var(--text-muted);
    font-size: 10px;
    min-width: 80px;
    font-family: var(--font-mono);
  }
  .connection-target {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
    text-align: left;
  }
  .connection-target:hover {
    text-decoration: underline;
  }
  .connection-overflow {
    color: var(--text-muted);
    font-size: 11px;
    padding-top: 4px;
  }

  @media (max-width: 768px) {
    .info-panel {
      width: calc(100% - 24px);
      bottom: 8px;
      left: 8px;
    }
  }
</style>
