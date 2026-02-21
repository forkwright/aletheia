<script lang="ts">
  import { fetchCostSummary, fetchMetrics } from "../lib/api";
  import { formatTokens, formatCost } from "../lib/format";
  import type { CostSummary, MetricsData } from "../lib/types";

  let costs = $state<CostSummary | null>(null);
  let metrics = $state<MetricsData | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      const [c, m] = await Promise.all([fetchCostSummary(), fetchMetrics()]);
      costs = c;
      metrics = m;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });
</script>

<div class="cost-dashboard">
  <div class="cost-header">
    <h3>Token Usage</h3>
    <button class="refresh-btn" onclick={load} disabled={loading}>
      {loading ? "Loading..." : "Refresh"}
    </button>
  </div>

  {#if error}
    <p class="cost-error">{error}</p>
  {:else if costs && metrics}
    <div class="cost-grid">
      <div class="card">
        <span class="card-label">Total Cost</span>
        <span class="card-value">{formatCost(costs.totalCost)}</span>
      </div>
      <div class="card">
        <span class="card-label">Total Input</span>
        <span class="card-value">{formatTokens(metrics.usage.totalInputTokens)}</span>
      </div>
      <div class="card">
        <span class="card-label">Total Output</span>
        <span class="card-value">{formatTokens(metrics.usage.totalOutputTokens)}</span>
      </div>
      <div class="card">
        <span class="card-label">Cache Hit Rate</span>
        <span class="card-value">{metrics.usage.cacheHitRate}%</span>
      </div>
      <div class="card">
        <span class="card-label">Total Turns</span>
        <span class="card-value">{metrics.usage.turnCount}</span>
      </div>
      <div class="card">
        <span class="card-label">Cache Read</span>
        <span class="card-value">{formatTokens(metrics.usage.totalCacheReadTokens)}</span>
      </div>
    </div>

    {#if costs.agents.length > 0}
      <h4 class="section-title">Per Agent</h4>
      <div class="agent-costs">
        {#each costs.agents as agent}
          {@const nousMetrics = metrics.nous.find(n => n.id === agent.agentId)}
          <div class="agent-row">
            <span class="agent-name">{nousMetrics?.name ?? agent.agentId}</span>
            <span class="agent-stat">{formatCost(agent.totalCost || agent.cost)}</span>
            {#if nousMetrics?.tokens}
              <span class="agent-stat muted">{formatTokens(nousMetrics.tokens.input)} in</span>
              <span class="agent-stat muted">{formatTokens(nousMetrics.tokens.output)} out</span>
            {/if}
            <span class="agent-stat muted">{agent.turns} turns</span>
          </div>
        {/each}
      </div>
    {/if}
  {:else if loading}
    <p class="cost-loading">Loading cost data...</p>
  {/if}
</div>

<style>
  .cost-dashboard {
    padding: 24px;
    overflow-y: auto;
    height: 100%;
  }
  .cost-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }
  .cost-header h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .refresh-btn {
    padding: 6px 12px;
    font-size: 12px;
    border: 1px solid var(--border);
    background: var(--bg-elevated);
    color: var(--text);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-sans);
  }
  .refresh-btn:hover:not(:disabled) {
    background: var(--surface);
  }
  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .cost-error {
    color: var(--red);
    padding: 32px;
    text-align: center;
  }
  .cost-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 16px;
    background: var(--bg-elevated);
    border-radius: var(--radius);
    border: 1px solid var(--border);
  }
  .card-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  .card-value {
    font-size: 28px;
    font-weight: 700;
  }
  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 0 0 8px;
  }
  .agent-costs {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .agent-row {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 8px 12px;
    font-size: 13px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .agent-name {
    flex: 1;
    font-weight: 500;
  }
  .agent-stat {
    min-width: 60px;
    text-align: right;
  }
  .muted {
    color: var(--text-muted);
  }
  .cost-loading {
    color: var(--text-muted);
    font-size: 13px;
    padding: 32px;
    text-align: center;
  }
</style>
