<script lang="ts">
  import { onMount } from "svelte";
  import Graph2D from "./Graph2D.svelte";
  import NodeCard from "./NodeCard.svelte";
  import HealthBar from "./HealthBar.svelte";
  import DriftPanel from "./DriftPanel.svelte";
  import TimelineSlider from "./TimelineSlider.svelte";
  import ContextLookup from "./ContextLookup.svelte";
  import {
    getGraphData, getLoading, getError, getSelectedNodeId,
    getSelectedNode, getNodeEdges, getCommunityIds,
    getHighlightedCommunity, getSearchQuery, getLoadedMode, getLoadedLimit,
    getTotalNodes, getEntityDetail, getEntityLoading, getCommunityMeta,
    getHiddenEdgeTypes, getEdgeTypes, toggleEdgeType, getFilteredEdges,
    searchGraph, getSearchResults, getSearchLoading, clearSearchResults,
    setSelectedNodeId, setHighlightedCommunity, setSearchQuery,
    loadGraph, loadEntityDetail, removeEntity, mergeEntityNodes,
    // Graph Intelligence (Phases 8-13)
    getMemoryHealth, getHealthLoading, loadMemoryHealth,
    getAgentOverlay, getAgentOverlayLoading, loadAgentOverlay,
    getDriftData, getDriftLoading, loadDriftData,
    getActiveOverlay, setActiveOverlay,
    getSelectedAgentFilter, setSelectedAgentFilter,
    loadTimeline,
  } from "../../stores/graph.svelte";

  type ViewMode = "2d" | "3d";

  let viewMode = $state<ViewMode>("2d");
  let graph2d = $state<Graph2D | null>(null);
  let graph3d = $state<any>(null);
  let progressivePhase = $state<"initial" | "full">("initial");
  let showContextLookup = $state(false);
  let showTimeline = $state(false);
  let showEdgeFilter = $state(true);

  const PALETTE = [
    "#58a6ff", "#3fb950", "#d29922", "#f85149", "#bc8cff",
    "#f778ba", "#79c0ff", "#56d4dd", "#e3b341", "#db6d28",
    "#8b949e", "#7ee787", "#a5d6ff", "#ffa657", "#ff7b72",
    "#d2a8ff", "#ffd8b5", "#89dceb", "#f9e2af", "#a6e3a1",
  ];

  const AGENT_COLORS: Record<string, string> = {
    syn: "#58a6ff",
    demiurge: "#d29922",
    syl: "#f778ba",
    akron: "#3fb950",
    eiron: "#bc8cff",
    arbor: "#56d4dd",
    unknown: "#8b949e",
  };

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
    // Load health data in background
    loadMemoryHealth();
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

  // Overlay mode toggles
  function toggleOverlay(mode: "agents" | "drift" | "timeline") {
    const current = getActiveOverlay();
    if (current === mode) {
      setActiveOverlay("none");
      if (mode === "timeline") {
        showTimeline = false;
        reloadGraph(); // Reload full graph
      }
      if (mode === "drift") {
        // Just hide
      }
      if (mode === "agents") {
        setSelectedAgentFilter(null);
      }
    } else {
      setActiveOverlay(mode);
      if (mode === "agents") loadAgentOverlay();
      if (mode === "drift") loadDriftData();
      if (mode === "timeline") showTimeline = true;
    }
  }

  function handleContextSearch(q: string) {
    searchGraph(q);
  }

  async function handleFlag(name: string, flagged: boolean) {
    const { flagEntity } = await import("../../lib/api");
    try {
      await flagEntity(name, flagged);
    } catch (e) {
      console.error("Flag failed:", e);
    }
  }

  function handleTimelineApply(since: string, until: string) {
    loadTimeline(since, until);
  }

  function handleTimelineClear() {
    showTimeline = false;
    setActiveOverlay("none");
    reloadGraph();
  }

  onMount(() => {
    initialLoad();
  });

  // Reactive helpers
  let graphData = $derived(getGraphData());
  let selectedNode = $derived(getSelectedNode());
  let selectedEdges = $derived(getSelectedNodeId() ? getNodeEdges(getSelectedNodeId()!) : []);

  let communityIds = $derived(getCommunityIds());
  let hoverNodeId = $state<string | null>(null);
  let edgeTypes = $derived(getEdgeTypes());
  let hiddenEdges = $derived(getHiddenEdgeTypes());
  let communityMeta = $derived(getCommunityMeta());

  let memoryHealth = $derived(getMemoryHealth());
  let healthLoading = $derived(getHealthLoading());
  let agentOverlay = $derived(getAgentOverlay());
  let driftData = $derived(getDriftData());
  let driftLoading = $derived(getDriftLoading());
  let activeOverlay = $derived(getActiveOverlay());
  let selectedAgentFilter = $derived(getSelectedAgentFilter());

  function communityLabel(cid: number): string {
    const meta = communityMeta.find((m) => m.id === cid);
    if (meta && "name" in meta) return (meta as unknown as { name: string }).name;
    return String(cid);
  }

  // Drift notification badge
  let driftBadge = $derived(driftData?.suggestion_count ?? 0);
</script>

<div class="graph-view">
  <!-- Health Bar (Phase 8) -->
  <HealthBar
    health={memoryHealth}
    loading={healthLoading}
    onRefresh={() => loadMemoryHealth()}
  />

  <!-- Timeline Slider (Phase 9) -->
  {#if showTimeline}
    <TimelineSlider
      dateRange={memoryHealth?.date_range ?? null}
      onRangeChange={handleTimelineApply}
      onClear={handleTimelineClear}
    />
  {/if}

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

    <!-- Overlay toggles (Phases 9-11) -->
    <div class="overlay-toggles">
      <button
        class="overlay-btn"
        class:active={activeOverlay === "timeline"}
        onclick={() => toggleOverlay("timeline")}
        title="Timeline filter"
      >📅</button>
      <button
        class="overlay-btn"
        class:active={activeOverlay === "agents"}
        onclick={() => toggleOverlay("agents")}
        title="Agent overlay"
      >👥</button>
      <button
        class="overlay-btn"
        class:active={activeOverlay === "drift"}
        onclick={() => toggleOverlay("drift")}
        title="Drift detection"
      >
        🔍
        {#if driftBadge > 0 && activeOverlay !== "drift"}
          <span class="drift-badge">{driftBadge}</span>
        {/if}
      </button>
      <button
        class="overlay-btn"
        class:active={showContextLookup}
        onclick={() => showContextLookup = !showContextLookup}
        title="Context lookup"
      >🔎</button>
    </div>

    <!-- Agent filter pills (Phase 10) -->
    {#if activeOverlay === "agents" && agentOverlay?.all_agents}
      <div class="agent-pills">
        <button
          class="pill agent-pill"
          class:active={selectedAgentFilter === null}
          onclick={() => setSelectedAgentFilter(null)}
        >All agents</button>
        {#each agentOverlay.all_agents as agent}
          <button
            class="pill agent-pill"
            class:active={selectedAgentFilter === agent}
            style="--pill-color: {AGENT_COLORS[agent] ?? '#8b949e'}"
            onclick={() => setSelectedAgentFilter(selectedAgentFilter === agent ? null : agent)}
          >{agent}</button>
        {/each}
      </div>
    {/if}

    <!-- Community pills (default mode) -->
    {#if activeOverlay === "none"}
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
    {/if}

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
        agentOverlay={activeOverlay === "agents" ? agentOverlay : null}
        agentFilter={selectedAgentFilter}
        agentColors={AGENT_COLORS}
        driftData={activeOverlay === "drift" ? driftData : null}
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

    <!-- Context Lookup (Phase 13) -->
    {#if showContextLookup}
      <ContextLookup
        searchQuery={getSearchQuery()}
        searchResults={getSearchResults()}
        searchLoading={getSearchLoading()}
        entityDetail={getEntityDetail()}
        entityLoading={getEntityLoading()}
        onSearch={handleContextSearch}
        onNodeClick={handleNodeClick}
        onClose={() => showContextLookup = false}
      />
    {/if}

    <!-- Drift Panel (Phase 11) -->
    {#if activeOverlay === "drift"}
      <DriftPanel
        drift={driftData}
        loading={driftLoading}
        onNodeClick={handleNodeClick}
        onRefresh={() => loadDriftData()}
        onDeleteEntity={async (name) => { await removeEntity(name); }}
      />
    {/if}
  </div>

  {#if selectedNode}
    <NodeCard
      node={selectedNode}
      detail={getEntityDetail()}
      detailLoading={getEntityLoading()}
      communityName={communityLabel(selectedNode.community)}
      communityColor={communityColor(selectedNode.community)}
      edges={selectedEdges}
      onNodeClick={handleNodeClick}
      onClose={() => { setSelectedNodeId(null); }}
      onDelete={removeEntity}
      onMerge={mergeEntityNodes}
      onFlag={handleFlag}
    />
  {/if}

  {#if edgeTypes.length > 0 && showEdgeFilter}
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
  .toggle-btn:hover { color: var(--text); }
  .toggle-btn.active {
    background: var(--accent);
    color: #fff;
  }

  /* Overlay toggles */
  .overlay-toggles {
    display: flex;
    gap: 2px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    flex-shrink: 0;
  }

  .overlay-btn {
    background: none;
    border: none;
    padding: 3px 8px;
    font-size: 12px;
    cursor: pointer;
    position: relative;
    transition: background 0.15s;
    line-height: 1;
  }
  .overlay-btn:hover { background: var(--surface); }
  .overlay-btn.active {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .drift-badge {
    position: absolute;
    top: -2px;
    right: 0;
    background: var(--yellow, #d29922);
    color: #000;
    font-size: 8px;
    font-weight: 700;
    padding: 1px 3px;
    border-radius: 6px;
    min-width: 12px;
    text-align: center;
    line-height: 1.2;
  }

  /* Agent pills */
  .agent-pills {
    display: flex;
    gap: 4px;
    flex-wrap: nowrap;
  }

  .agent-pill {
    text-transform: capitalize;
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
  .graph-loading { color: var(--text-secondary); }
  .graph-error { color: var(--red); }

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
</style>
