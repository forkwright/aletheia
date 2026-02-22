<script lang="ts">
  import type { DriftData } from "../../lib/api";

  let {
    drift,
    loading = false,
    onNodeClick,
    onRefresh,
    onDeleteEntity,
  }: {
    drift: DriftData | null;
    loading?: boolean;
    onNodeClick?: (id: string) => void;
    onRefresh?: () => void;
    onDeleteEntity?: (name: string) => Promise<void>;
  } = $props();

  let activeSection = $state<"suggestions" | "orphans" | "stale" | "clusters">("suggestions");
  let expanded = $state(true);
  let pendingAction = $state<string | null>(null);

  async function executeDelete(entity: string) {
    if (!onDeleteEntity) return;
    pendingAction = entity;
    try {
      await onDeleteEntity(entity);
      onRefresh?.();
    } finally {
      pendingAction = null;
    }
  }
</script>

<div class="drift-panel" class:collapsed={!expanded}>
  <div class="panel-header" onclick={() => expanded = !expanded}>
    <span class="panel-title">
      🔍 Drift Detection
      {#if drift?.suggestion_count}
        <span class="badge">{drift.suggestion_count}</span>
      {/if}
    </span>
    <span class="chevron">{expanded ? "▾" : "▸"}</span>
  </div>

  {#if expanded}
    {#if loading}
      <div class="panel-loading">Analyzing graph…</div>
    {:else if drift}
      <div class="section-tabs">
        <button
          class="section-tab"
          class:active={activeSection === "suggestions"}
          onclick={() => activeSection = "suggestions"}
        >
          Actions ({drift.suggestions.length})
        </button>
        <button
          class="section-tab"
          class:active={activeSection === "orphans"}
          onclick={() => activeSection = "orphans"}
        >
          Orphans ({drift.orphaned_nodes.length})
        </button>
        <button
          class="section-tab"
          class:active={activeSection === "stale"}
          onclick={() => activeSection = "stale"}
        >
          Stale ({drift.stale_entities.length})
        </button>
        <button
          class="section-tab"
          class:active={activeSection === "clusters"}
          onclick={() => activeSection = "clusters"}
        >
          Isolated ({drift.small_clusters.length})
        </button>
      </div>

      <div class="section-content">
        {#if activeSection === "suggestions"}
          {#if drift.suggestions.length === 0}
            <div class="empty">No suggested actions — graph looks healthy ✓</div>
          {:else}
            {#each drift.suggestions as suggestion}
              <div class="suggestion-item">
                <span class="suggestion-type" class:delete={suggestion.type === "delete"} class:review={suggestion.type === "review"} class:merge={suggestion.type === "merge_or_delete"}>
                  {suggestion.type === "delete" ? "🗑" : suggestion.type === "review" ? "👁" : "🔗"}
                </span>
                <div class="suggestion-body">
                  <button class="entity-link" onclick={() => onNodeClick?.(suggestion.entity)}>
                    {suggestion.entity}
                  </button>
                  <span class="suggestion-reason">{suggestion.reason}</span>
                  {#if onDeleteEntity && (suggestion.type === "delete" || suggestion.type === "merge_or_delete")}
                    <button
                      class="action-btn delete-btn"
                      disabled={pendingAction === suggestion.entity}
                      onclick={() => executeDelete(suggestion.entity)}
                    >
                      {pendingAction === suggestion.entity ? "…" : "Delete"}
                    </button>
                  {/if}
                </div>
              </div>
            {/each}
          {/if}

        {:else if activeSection === "orphans"}
          {#if drift.orphaned_nodes.length === 0}
            <div class="empty">No orphaned nodes</div>
          {:else}
            {#each drift.orphaned_nodes as node}
              <div class="drift-row">
                <button class="entity-link" onclick={() => onNodeClick?.(node.name)}>{node.name}</button>
                <span class="drift-meta">PR {(node.pagerank || 0).toFixed(4)}</span>
              </div>
            {/each}
          {/if}

        {:else if activeSection === "stale"}
          {#if drift.stale_entities.length === 0}
            <div class="empty">No stale entities (all within 30 days)</div>
          {:else}
            {#each drift.stale_entities.slice(0, 20) as entity}
              <div class="drift-row">
                <button class="entity-link" onclick={() => onNodeClick?.(entity.name)}>{entity.name}</button>
                <span class="drift-meta stale-age">{entity.age_days}d ago</span>
              </div>
            {/each}
          {/if}

        {:else if activeSection === "clusters"}
          {#if drift.small_clusters.length === 0}
            <div class="empty">No isolated clusters</div>
          {:else}
            {#each drift.small_clusters as cluster}
              <div class="cluster-row">
                <span class="cluster-id">C{cluster.comm}</span>
                <div class="cluster-members">
                  {#each cluster.members as member}
                    <button class="entity-link" onclick={() => onNodeClick?.(member)}>{member}</button>
                  {/each}
                </div>
              </div>
            {/each}
          {/if}
        {/if}
      </div>

      {#if onRefresh}
        <button class="panel-refresh" onclick={onRefresh} disabled={loading}>
          Refresh Analysis
        </button>
      {/if}
    {:else}
      <div class="panel-loading">No drift data available</div>
    {/if}
  {/if}
</div>

<style>
  .drift-panel {
    position: absolute;
    bottom: 12px;
    right: 12px;
    width: 300px;
    max-height: 400px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius, 8px);
    z-index: 20;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .drift-panel.collapsed {
    max-height: none;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    cursor: pointer;
    border-bottom: 1px solid var(--border);
    user-select: none;
  }

  .panel-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .badge {
    background: var(--status-warning);
    color: #000;
    font-size: var(--text-2xs);
    font-weight: 700;
    padding: 1px 5px;
    border-radius: var(--radius);
    min-width: 16px;
    text-align: center;
  }

  .chevron {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .section-tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }

  .section-tab {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-2xs);
    padding: 5px 4px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: all var(--transition-quick);
  }
  .section-tab:hover { color: var(--text-secondary); }
  .section-tab.active {
    color: var(--text);
    border-bottom-color: var(--accent, #9A7B4F);
  }

  .section-content {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    min-height: 60px;
    max-height: 260px;
  }

  .suggestion-item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .suggestion-item:last-child { border-bottom: none; }

  .suggestion-type {
    font-size: var(--text-sm);
    flex-shrink: 0;
    margin-top: 1px;
  }

  .suggestion-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .suggestion-reason {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .entity-link {
    background: none;
    border: none;
    color: var(--accent, #9A7B4F);
    font-size: var(--text-sm);
    cursor: pointer;
    padding: 0;
    text-align: left;
  }
  .entity-link:hover { text-decoration: underline; }

  .action-btn {
    font-size: var(--text-2xs);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    border: 1px solid;
    margin-top: 2px;
    align-self: flex-start;
  }
  .delete-btn {
    background: rgba(248, 81, 73, 0.1);
    border-color: var(--status-error);
    color: var(--status-error);
  }
  .delete-btn:hover:not(:disabled) { background: rgba(248, 81, 73, 0.25); }
  .delete-btn:disabled { opacity: 0.4; cursor: default; }

  .drift-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .drift-row:last-child { border-bottom: none; }

  .drift-meta {
    font-size: var(--text-2xs);
    font-family: var(--font-mono, monospace);
    color: var(--text-muted);
  }

  .stale-age {
    color: var(--status-warning);
  }

  .cluster-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .cluster-row:last-child { border-bottom: none; }

  .cluster-id {
    font-size: var(--text-2xs);
    font-family: var(--font-mono, monospace);
    color: var(--text-muted);
    min-width: 24px;
  }

  .cluster-members {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .empty {
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-xs);
    padding: 16px 0;
  }

  .panel-loading {
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-xs);
    padding: 16px 8px;
  }

  .panel-refresh {
    background: var(--surface);
    border: none;
    border-top: 1px solid var(--border);
    color: var(--text-secondary);
    font-size: var(--text-xs);
    padding: 6px;
    cursor: pointer;
    text-align: center;
  }
  .panel-refresh:hover { color: var(--text); }
  .panel-refresh:disabled { opacity: 0.4; cursor: default; }

  @media (max-width: 768px) {
    .drift-panel {
      width: calc(100% - 24px);
      left: 12px;
      bottom: 8px;
    }
  }
</style>
