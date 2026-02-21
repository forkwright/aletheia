<script lang="ts">
  import type { GraphNode, EntityDetail, EntityRelationship, EntityMemory } from "../../lib/types";

  let {
    node,
    detail,
    detailLoading,
    communityName,
    communityColor,
    edges,
    onNodeClick,
    onClose,
    onDelete,
    onMerge,
  }: {
    node: GraphNode;
    detail: EntityDetail | null;
    detailLoading: boolean;
    communityName: string;
    communityColor: string;
    edges: Array<{ rel_type: string; source: string; target: string }>;
    onNodeClick: (id: string) => void;
    onClose: () => void;
    onDelete: (name: string) => Promise<boolean>;
    onMerge: (source: string, target: string) => Promise<boolean>;
  } = $props();

  let confirmDelete = $state(false);
  let mergeTarget = $state("");
  let showAllMemories = $state(false);
  let showAllConnections = $state(false);
  let activeTab = $state<"memories" | "connections">("memories");

  // Group relationships by type
  function groupedRelationships(rels: EntityRelationship[]): Map<string, EntityRelationship[]> {
    const map = new Map<string, EntityRelationship[]>();
    for (const r of rels) {
      const list = map.get(r.type) || [];
      list.push(r);
      map.set(r.type, list);
    }
    return map;
  }

  function relativeTime(dateStr: string | null | undefined): string {
    if (!dateStr) return "";
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return "just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHrs = Math.floor(diffMins / 60);
    if (diffHrs < 24) return `${diffHrs}h ago`;
    const diffDays = Math.floor(diffHrs / 24);
    if (diffDays < 30) return `${diffDays}d ago`;
    const diffWeeks = Math.floor(diffDays / 7);
    if (diffWeeks < 8) return `${diffWeeks}w ago`;
    const diffMonths = Math.floor(diffDays / 30);
    return `${diffMonths}mo ago`;
  }

  function confidenceLabel(c: "high" | "medium" | "low"): string {
    return c === "high" ? "Strong" : c === "medium" ? "Moderate" : "Weak";
  }

  function confidenceIcon(c: "high" | "medium" | "low"): string {
    return c === "high" ? "🟢" : c === "medium" ? "🟡" : "🔴";
  }

  function sourceIcon(source: string | undefined): string {
    return source === "stated" ? "▐" : "┊";
  }

  function sourceLabel(source: string | undefined): string {
    return source === "stated" ? "Stated" : "Inferred";
  }

  let displayMemories = $derived(
    detail?.memories
      ? showAllMemories ? detail.memories : detail.memories.slice(0, 5)
      : []
  );

  let displayEdges = $derived(
    showAllConnections ? edges : edges.slice(0, 15)
  );

  let relGroups = $derived(
    detail?.relationships ? groupedRelationships(detail.relationships) : new Map()
  );
</script>

<div class="node-card">
  <!-- Header -->
  <div class="card-header">
    <div class="header-main">
      <span class="community-dot" style="background: {communityColor}"></span>
      <h3 class="entity-name">{node.id}</h3>
      {#if detail}
        <span class="confidence-badge" title="{confidenceLabel(detail.confidence)} confidence">
          {confidenceIcon(detail.confidence)}
        </span>
      {/if}
      <button class="close-btn" onclick={onClose}>&times;</button>
    </div>
    <div class="header-meta">
      <span class="meta-community">{communityName}</span>
      <span class="meta-separator">·</span>
      <span class="meta-pagerank" title="PageRank">PR {node.pagerank.toFixed(4)}</span>
      <span class="meta-separator">·</span>
      <span class="meta-connections">{edges.length} connections</span>
    </div>
    {#if node.labels.length > 0}
      <div class="header-labels">
        {#each node.labels as label}
          <span class="label-tag">{label}</span>
        {/each}
      </div>
    {/if}
  </div>

  {#if detailLoading}
    <div class="loading-indicator">
      <span class="loading-dot"></span>
      Loading entity details...
    </div>
  {/if}

  <!-- Tab navigation -->
  <div class="tab-bar">
    <button
      class="tab-btn"
      class:active={activeTab === "memories"}
      onclick={() => activeTab = "memories"}
    >
      Memories {detail?.memories?.length ? `(${detail.memories.length})` : ""}
    </button>
    <button
      class="tab-btn"
      class:active={activeTab === "connections"}
      onclick={() => activeTab = "connections"}
    >
      Connections ({edges.length})
    </button>
  </div>

  <!-- Memories tab -->
  {#if activeTab === "memories"}
    <div class="tab-content">
      {#if displayMemories.length > 0}
        <div class="memory-list">
          {#each displayMemories as mem}
            <div class="memory-item" class:stated={mem.source === "stated"}>
              <div class="memory-header">
                <span class="memory-source" title="{sourceLabel(mem.source)}">
                  {sourceIcon(mem.source)}
                </span>
                <span class="memory-confidence">{(mem.score * 100).toFixed(0)}%</span>
                {#if mem.created_at}
                  <span class="memory-age">{relativeTime(mem.created_at)}</span>
                {/if}
                {#if mem.agent_id}
                  <span class="memory-agent">{mem.agent_id}</span>
                {/if}
              </div>
              <p class="memory-text">{mem.text}</p>
            </div>
          {/each}
        </div>
        {#if detail && detail.memories.length > 5}
          <button class="show-more-btn" onclick={() => showAllMemories = !showAllMemories}>
            {showAllMemories ? "Show less" : `Show all ${detail.memories.length}`}
          </button>
        {/if}
      {:else if !detailLoading}
        <div class="empty-state">No memories found for this entity</div>
      {/if}
    </div>
  {/if}

  <!-- Connections tab -->
  {#if activeTab === "connections"}
    <div class="tab-content">
      {#if detail && relGroups.size > 0}
        <div class="relationship-groups">
          {#each [...relGroups.entries()] as [type, rels]}
            <div class="rel-group">
              <div class="rel-group-header">
                <span class="rel-type-badge">{type}</span>
                <span class="rel-count">{rels.length}</span>
              </div>
              <div class="rel-targets">
                {#each rels.slice(0, showAllConnections ? rels.length : 5) as rel}
                  <button
                    class="rel-target"
                    class:incoming={rel.direction === "in"}
                    onclick={() => onNodeClick(rel.target)}
                  >
                    <span class="rel-arrow">{rel.direction === "out" ? "→" : "←"}</span>
                    {rel.target}
                  </button>
                {/each}
                {#if rels.length > 5 && !showAllConnections}
                  <span class="overflow-label">+{rels.length - 5} more</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <!-- Fallback to edge-based view if detail not loaded -->
        <div class="connection-list">
          {#each displayEdges as edge}
            <div class="connection-row">
              <span class="edge-type">{edge.rel_type}</span>
              <button
                class="connection-target"
                onclick={() => onNodeClick(edge.source === node.id ? edge.target : edge.source)}
              >
                {edge.source === node.id ? edge.target : edge.source}
              </button>
            </div>
          {/each}
          {#if edges.length > 15 && !showAllConnections}
            <button class="show-more-btn" onclick={() => showAllConnections = true}>
              Show all {edges.length}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Actions -->
  <div class="card-actions">
    {#if confirmDelete}
      <div class="confirm-row">
        <span class="confirm-label">Delete "{node.id}"?</span>
        <button class="action-btn danger" onclick={async () => { await onDelete(node.id); confirmDelete = false; }}>
          Confirm
        </button>
        <button class="action-btn" onclick={() => confirmDelete = false}>Cancel</button>
      </div>
    {:else}
      <button class="action-btn danger-outline" onclick={() => confirmDelete = true}>Delete</button>
    {/if}
    <div class="merge-row">
      <input
        class="merge-input"
        type="text"
        placeholder="Merge into..."
        bind:value={mergeTarget}
      />
      <button
        class="action-btn primary"
        disabled={!mergeTarget.trim()}
        onclick={async () => {
          const ok = await onMerge(node.id, mergeTarget.trim());
          if (ok) mergeTarget = "";
        }}
      >Merge</button>
    </div>
  </div>
</div>

<style>
  .node-card {
    position: absolute;
    bottom: 12px;
    left: 12px;
    width: 360px;
    max-height: calc(100vh - 120px);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    z-index: 20;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  /* Header */
  .card-header {
    padding: 14px 14px 10px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .header-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .community-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .entity-name {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .confidence-badge {
    font-size: 12px;
    flex-shrink: 0;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    flex-shrink: 0;
  }
  .close-btn:hover { color: var(--text); }

  .header-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .meta-separator { opacity: 0.4; }

  .meta-community {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .meta-pagerank {
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .header-labels {
    display: flex;
    gap: 4px;
    margin-top: 6px;
    flex-wrap: wrap;
  }

  .label-tag {
    background: var(--surface);
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px;
    color: var(--text-secondary);
  }

  /* Loading */
  .loading-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .loading-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.3; }
    50% { opacity: 1; }
  }

  /* Tabs */
  .tab-bar {
    display: flex;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .tab-btn {
    flex: 1;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-muted);
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tab-btn:hover {
    color: var(--text);
    background: var(--surface);
  }

  .tab-btn.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  /* Tab content */
  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding: 10px 14px;
    min-height: 80px;
    max-height: 300px;
  }

  /* Memories */
  .memory-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .memory-item {
    padding: 8px 10px;
    background: var(--surface);
    border-radius: var(--radius-sm);
    border-left: 3px solid var(--border);
  }

  .memory-item.stated {
    border-left-color: var(--accent);
  }

  .memory-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
    font-size: 10px;
  }

  .memory-source {
    font-family: var(--font-mono);
    font-size: 14px;
    line-height: 1;
    color: var(--text-muted);
  }

  .memory-confidence {
    font-family: var(--font-mono);
    color: var(--text-muted);
    font-weight: 600;
  }

  .memory-age {
    color: var(--text-muted);
    margin-left: auto;
  }

  .memory-agent {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    padding: 0 4px;
    border-radius: 3px;
    font-size: 9px;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .memory-text {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-secondary);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
  }

  /* Relationships */
  .relationship-groups {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rel-group-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .rel-type-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-secondary);
    background: var(--surface);
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    font-weight: 600;
  }

  .rel-count {
    font-size: 10px;
    color: var(--text-muted);
  }

  .rel-targets {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding-left: 4px;
  }

  .rel-target {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 12px;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: 3px;
    transition: background 0.1s;
  }

  .rel-target:hover {
    background: var(--surface);
    text-decoration: underline;
  }

  .rel-target.incoming {
    color: var(--text-secondary);
  }

  .rel-arrow {
    font-size: 10px;
    margin-right: 2px;
    opacity: 0.5;
  }

  .overflow-label {
    font-size: 11px;
    color: var(--text-muted);
    padding: 2px 4px;
  }

  /* Fallback connection list */
  .connection-list {
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

  .edge-type {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-muted);
    min-width: 80px;
  }

  .connection-target {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  .connection-target:hover { text-decoration: underline; }

  /* Empty */
  .empty-state {
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
    padding: 20px 0;
  }

  /* Show more */
  .show-more-btn {
    display: block;
    width: 100%;
    background: none;
    border: none;
    color: var(--accent);
    font-size: 11px;
    cursor: pointer;
    padding: 6px 0;
    text-align: center;
  }
  .show-more-btn:hover { text-decoration: underline; }

  /* Actions */
  .card-actions {
    padding: 10px 14px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-shrink: 0;
  }

  .confirm-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .confirm-label {
    font-size: 12px;
    color: var(--red);
    font-weight: 600;
    flex: 1;
  }

  .action-btn {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 4px 12px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    cursor: pointer;
    transition: all 0.15s;
  }
  .action-btn:hover { color: var(--text); border-color: var(--text-muted); }
  .action-btn:disabled { opacity: 0.4; cursor: default; }

  .action-btn.danger {
    background: var(--red);
    border-color: var(--red);
    color: #fff;
  }

  .action-btn.danger-outline {
    border-color: var(--red);
    color: var(--red);
    background: none;
  }
  .action-btn.danger-outline:hover {
    background: var(--red);
    color: #fff;
  }

  .action-btn.primary {
    border-color: var(--accent);
    color: var(--accent);
  }
  .action-btn.primary:hover {
    background: var(--accent);
    color: #fff;
  }

  .merge-row {
    display: flex;
    gap: 6px;
  }

  .merge-input {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 4px 8px;
    font-size: 11px;
    font-family: var(--font-sans);
  }
  .merge-input:focus { outline: none; border-color: var(--accent); }

  @media (max-width: 768px) {
    .node-card {
      width: calc(100% - 24px);
      bottom: 8px;
      left: 8px;
      max-height: 60vh;
    }
  }
</style>
