<script lang="ts">
  import type { MemoryHealth } from "../../lib/api";

  let {
    health,
    loading = false,
    onRefresh,
  }: {
    health: MemoryHealth | null;
    loading?: boolean;
    onRefresh?: () => void;
  } = $props();
</script>

<div class="health-bar">
  {#if loading}
    <span class="health-loading">Loading health…</span>
  {:else if health}
    <div class="health-stats">
      <div class="stat" title="Total memories in vector store">
        <span class="stat-value">{health.total.toLocaleString()}</span>
        <span class="stat-label">total</span>
      </div>
      <div class="stat-divider"></div>
      <div class="stat" class:warn={health.stale > 10} title="Memories older than 30 days">
        <span class="stat-icon">🕐</span>
        <span class="stat-value">{health.stale}</span>
        <span class="stat-label">stale</span>
      </div>
      <div class="stat" class:warn={health.conflicts > 5} title="Low-confidence (potentially contradicting)">
        <span class="stat-icon">⚠️</span>
        <span class="stat-value">{health.conflicts}</span>
        <span class="stat-label">conflicts</span>
      </div>
      <div class="stat" class:warn={health.flagged > 0} title="Manually flagged for review">
        <span class="stat-icon">🚩</span>
        <span class="stat-value">{health.flagged}</span>
        <span class="stat-label">flagged</span>
      </div>
      <div class="stat-divider"></div>
      <div class="stat" title="Average confidence across sampled memories">
        <span class="stat-icon">{health.avg_confidence >= 0.7 ? "🟢" : health.avg_confidence >= 0.4 ? "🟡" : "🔴"}</span>
        <span class="stat-value">{(health.avg_confidence * 100).toFixed(0)}%</span>
        <span class="stat-label">confidence</span>
      </div>
      {#if health.forgotten > 0}
        <div class="stat muted" title="Soft-deleted memories">
          <span class="stat-value">{health.forgotten}</span>
          <span class="stat-label">forgotten</span>
        </div>
      {/if}
    </div>
    {#if health.by_agent && Object.keys(health.by_agent).length > 0}
      <div class="agent-breakdown">
        {#each Object.entries(health.by_agent).slice(0, 5) as [agent, count]}
          <span class="agent-chip" title="{agent}: {count} memories">{agent} <b>{count}</b></span>
        {/each}
      </div>
    {/if}
  {:else}
    <span class="health-empty">No health data</span>
  {/if}
  {#if onRefresh}
    <button class="refresh-btn" onclick={onRefresh} disabled={loading} title="Refresh health data">↻</button>
  {/if}
</div>

<style>
  .health-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    background: var(--bg-elevated, #161b22);
    border-bottom: 1px solid var(--border, #30363d);
    font-size: 12px;
    min-height: 28px;
    flex-wrap: wrap;
  }

  .health-stats {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .stat {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--text-secondary, #8b949e);
  }

  .stat-icon {
    font-size: 11px;
  }

  .stat-value {
    font-weight: 600;
    color: var(--text, #e6edf3);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .stat-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    opacity: 0.7;
  }

  .stat.warn .stat-value {
    color: var(--yellow, #d29922);
  }

  .stat.muted {
    opacity: 0.5;
  }

  .stat-divider {
    width: 1px;
    height: 14px;
    background: var(--border, #30363d);
  }

  .agent-breakdown {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }

  .agent-chip {
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    padding: 1px 6px;
    border-radius: 8px;
    font-size: 10px;
    color: var(--text-secondary, #8b949e);
    white-space: nowrap;
  }
  .agent-chip b {
    color: var(--text, #e6edf3);
    font-weight: 600;
    margin-left: 2px;
  }

  .health-loading, .health-empty {
    color: var(--text-muted, #484f58);
    font-size: 11px;
  }

  .refresh-btn {
    background: none;
    border: 1px solid var(--border, #30363d);
    color: var(--text-secondary, #8b949e);
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 12px;
    cursor: pointer;
    margin-left: auto;
  }
  .refresh-btn:hover { color: var(--text, #e6edf3); }
  .refresh-btn:disabled { opacity: 0.4; cursor: default; }
</style>
