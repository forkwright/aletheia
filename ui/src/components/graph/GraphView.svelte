<script lang="ts">
  import { onMount } from "svelte";
  import Graph2D from "./Graph2D.svelte";
  import {
    getGraphData, getLoading, getError, getSelectedNodeId,
    getSelectedNode, getNodeEdges, getConnectedNodes, getCommunityIds,
    getHighlightedCommunity, getSearchQuery, getLoadedMode, getLoadedLimit,
    getTotalNodes, getEntityDetail, getEntityLoading, getCommunityMeta,
    getHiddenEdgeTypes, getEdgeTypes, toggleEdgeType, getFilteredEdges,
    searchGraph, getSearchResults, getSearchLoading, clearSearchResults,
    setSelectedNodeId, setHighlightedCommunity, setSearchQuery,
    loadGraph, loadEntityDetail, removeEntity, mergeEntityNodes,
  } from "../../stores/graph.svelte";

  type ViewMode = "2d" | "3d";

  let viewMode = $state<ViewMode>("2d");
  let graph2d = $state<Graph2D | null>(null);
  let graph3d = $state<any>(null);
  let progressivePhase = $state<"initial" | "full">("initial");

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

  // --- Progressive loading ---
  async function initialLoad() {
    progressivePhase = "initial";
    await loadGraph({ mode: "top", limit: 20 });
    progressivePhase = "full";
    await loadGraph({ mode: "top", limit: 200 });
  }

  // --- Handlers ---
  function focusOnNode(nodeId: string) {
    setSelectedNodeId(nodeId);
    if (viewMode === "2d" && graph2d) {
      graph2d.focusOnNode(nodeId);
    } else if (viewMode === "3d" && graph3d) {
      graph3d.focusOnNode(nodeId);
    }
  }

  function handleNodeClick(nodeId: string) {
    focusOnNode(nodeId);
    loadEntityDetail(nodeId);
    confirmDelete = false;
    mergeTarget = "";
  }

  function handleBackgroundClick() {
    setSelectedNodeId(null);
    setHighlightedCommunity(null);
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
    await loadGraph(params);
  }

  function switchView(mode: ViewMode) {
    viewMode = mode;
  }

  onMount(() => {
    initialLoad();
  });

  // Reactive helpers
  let graphData = $derived(getGraphData());
  let selectedNode = $derived(getSelectedNode());
  let selectedEdges = $derived(getSelectedNodeId() ? getNodeEdges(getSelectedNodeId()!) : []);
  let connectedNodes = $derived(getSelectedNodeId() ? getConnectedNodes(getSelectedNodeId()!) : []);
  let communityIds = $derived(getCommunityIds());
  let hoverNodeId = $state<string | null>(null);
  let confirmDelete = $state(false);
  let mergeTarget = $state("");
  let edgeTypes = $derived(getEdgeTypes());
  let hiddenEdges = $derived(getHiddenEdgeTypes());
  let communityMeta = $derived(getCommunityMeta());

  function communityLabel(cid: number): string {
    const meta = communityMeta.find((m) => m.id === cid);
    if (meta && "name" in meta) return (meta as unknown as { name: string }).name;
    return String(cid);
  }
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
    <div class="view-toggle">
      <button class="toggle-btn" class:active={viewMode === "2d"} onclick={() => switchView("2d")}>2D</button>
      <button class="toggle-btn" class:active={viewMode === "3d"} onclick={() => switchView("3d")}>3D</button>
    </div>
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
        >{communityLabel(cid)}</button>
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
      {graphData.nodes.length}{getTotalNodes() > 0 ? ` of ${getTotalNodes()}` : ""} nodes · {graphData.edges.length} edges
    </span>
  </div>

  <div class="graph-container">
    {#if getLoading() && graphData.nodes.length === 0}
      <div class="graph-loading">Loading graph...</div>
    {/if}
    {#if getError()}
      <div class="graph-error">{getError()}</div>
    {/if}

    {#if viewMode === "2d"}
      <Graph2D
        bind:this={graph2d}
        {graphData}
        selectedNodeId={getSelectedNodeId()}
        highlightedCommunity={getHighlightedCommunity()}
        bind:hoverNodeId
        onNodeClick={handleNodeClick}
        onBackgroundClick={handleBackgroundClick}
      />
    {:else}
      {#await import("./Graph3D.svelte") then { default: Graph3D }}
        <Graph3D
          bind:this={graph3d}
          {graphData}
          selectedNodeId={getSelectedNodeId()}
          highlightedCommunity={getHighlightedCommunity()}
          bind:hoverNodeId
          onNodeClick={handleNodeClick}
          onBackgroundClick={handleBackgroundClick}
        />
      {:catch}
        <div class="graph-error">Failed to load 3D renderer</div>
      {/await}
    {/if}
  </div>

  {#if selectedNode}
    {@const detail = getEntityDetail()}
    {@const detailLoading = getEntityLoading()}
    <div class="info-panel">
      <div class="info-header">
        <span class="info-dot" style="background: {communityColor(selectedNode.community)}"></span>
        <strong>{selectedNode.id}</strong>
        {#if detail}
          <span class="confidence-dot confidence-{detail.confidence}" title="Confidence: {detail.confidence}"></span>
        {/if}
        <span class="info-meta">
          Community {selectedNode.community >= 0 ? selectedNode.community : "\u2014"} ·
          PR {selectedNode.pagerank.toFixed(4)}
        </span>
        <button class="info-close" onclick={() => { setSelectedNodeId(null); confirmDelete = false; mergeTarget = ""; }}>&times;</button>
      </div>
      {#if selectedNode.labels.length > 0}
        <div class="info-labels">
          {#each selectedNode.labels as label}
            <span class="label-tag">{label}</span>
          {/each}
        </div>
      {/if}

      {#if detailLoading}
        <div class="detail-loading">Loading details...</div>
      {/if}

      {#if connectedNodes.length > 0}
        <div class="info-section-title">Relationships ({selectedEdges.length})</div>
        <div class="info-connections">
          {#each selectedEdges.slice(0, 20) as edge}
            <div class="connection-row">
              <span class="rel-type">{edge.rel_type}</span>
              <button
                class="connection-target"
                onclick={() => { handleNodeClick(edge.source === selectedNode?.id ? edge.target : edge.source); }}
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

      {#if detail?.memories && detail.memories.length > 0}
        <div class="info-section-title">Memories ({detail.memories.length})</div>
        <div class="info-memories">
          {#each detail.memories.slice(0, 5) as mem}
            <div class="memory-row">
              <span class="memory-score">{(mem.score * 100).toFixed(0)}%</span>
              <span class="memory-text">{mem.text}</span>
            </div>
          {/each}
          {#if detail.memories.length > 5}
            <div class="connection-overflow">+{detail.memories.length - 5} more</div>
          {/if}
        </div>
      {/if}

      <div class="info-actions">
        {#if confirmDelete}
          <span class="confirm-label">Delete this entity?</span>
          <button class="action-btn danger" onclick={async () => { await removeEntity(selectedNode!.id); confirmDelete = false; }}>Confirm</button>
          <button class="action-btn" onclick={() => { confirmDelete = false; }}>Cancel</button>
        {:else}
          <button class="action-btn danger" onclick={() => { confirmDelete = true; }}>Delete</button>
        {/if}
        <div class="merge-row">
          <input class="merge-input" type="text" placeholder="Merge into..." bind:value={mergeTarget} />
          <button
            class="action-btn"
            disabled={!mergeTarget.trim()}
            onclick={async () => { const ok = await mergeEntityNodes(selectedNode!.id, mergeTarget.trim()); if (ok) mergeTarget = ""; }}
          >Merge</button>
        </div>
      </div>
    </div>
  {/if}

  {#if edgeTypes.length > 0}
    <div class="edge-filter-panel">
      <h4 class="panel-heading">Edge Types</h4>
      {#each edgeTypes as type}
        {@const hidden = hiddenEdges.has(type)}
        <label class="edge-toggle">
          <input type="checkbox" checked={!hidden} onchange={() => toggleEdgeType(type)} />
          <span class:muted={hidden}>{type}</span>
        </label>
      {/each}
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

  .view-toggle {
    display: flex;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    flex-shrink: 0;
  }
  .toggle-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    padding: 3px 10px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .toggle-btn:hover {
    color: var(--text);
  }
  .toggle-btn.active {
    background: var(--accent);
    color: #fff;
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

  .confidence-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .confidence-high { background: #3fb950; }
  .confidence-medium { background: #d29922; }
  .confidence-low { background: #f85149; }

  .detail-loading {
    font-size: 11px;
    color: var(--text-muted);
    padding: 4px 0;
  }

  .info-section-title {
    font-size: 11px;
    color: var(--text-muted);
    font-weight: 600;
    margin-top: 8px;
    margin-bottom: 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .info-memories {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .memory-row {
    display: flex;
    gap: 6px;
    font-size: 11px;
    line-height: 1.3;
  }
  .memory-score {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    flex-shrink: 0;
    min-width: 30px;
  }
  .memory-text {
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .info-actions {
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .confirm-label {
    font-size: 12px;
    color: var(--red);
    font-weight: 600;
  }
  .action-btn {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 3px 10px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    cursor: pointer;
  }
  .action-btn:hover { color: var(--text); border-color: var(--text-muted); }
  .action-btn:disabled { opacity: 0.4; cursor: default; }
  .action-btn.danger { border-color: var(--red); color: var(--red); }
  .action-btn.danger:hover { background: var(--red); color: #fff; }

  .merge-row {
    display: flex;
    gap: 4px;
    width: 100%;
    margin-top: 4px;
  }
  .merge-input {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 3px 8px;
    font-size: 11px;
    font-family: var(--font-sans);
  }
  .merge-input:focus { outline: none; border-color: var(--accent); }

  .edge-filter-panel {
    position: absolute;
    top: 60px;
    right: 12px;
    width: 180px;
    max-height: 280px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
    overflow-y: auto;
    z-index: 20;
  }
  .panel-heading {
    font-size: 0.75rem;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 6px;
  }
  .edge-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8rem;
    cursor: pointer;
    padding: 2px 0;
  }
  .edge-toggle .muted {
    opacity: 0.4;
    text-decoration: line-through;
  }

  @media (max-width: 768px) {
    .info-panel {
      width: calc(100% - 24px);
      bottom: 8px;
      left: 8px;
    }
  }
</style>
