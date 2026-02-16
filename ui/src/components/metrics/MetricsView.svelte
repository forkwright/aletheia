<script lang="ts">
  import { fetchMetrics, fetchCostSummary } from "../../lib/api";
  import { formatTokens, formatUptime, formatCost, formatTimeSince } from "../../lib/format";
  import Badge from "../shared/Badge.svelte";
  import type { MetricsData, CostSummary } from "../../lib/types";

  let metrics = $state<MetricsData | null>(null);
  let costs = $state<CostSummary | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    try {
      const [m, c] = await Promise.all([fetchMetrics(), fetchCostSummary()]);
      metrics = m;
      costs = c;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  $effect(() => {
    load();
    const iv = setInterval(load, 15000);
    return () => clearInterval(iv);
  });
</script>

<div class="metrics-view">
  {#if error}
    <div class="error">{error}</div>
  {:else if !metrics}
    <div class="loading">Loading metrics...</div>
  {:else}
    <div class="grid">
      <div class="card">
        <div class="card-label">Uptime</div>
        <div class="card-value">{formatUptime(metrics.uptime)}</div>
      </div>
      <div class="card">
        <div class="card-label">Tokens</div>
        <div class="card-value">{formatTokens(metrics.usage.totalInputTokens + metrics.usage.totalOutputTokens)}</div>
        <div class="card-sub">{formatTokens(metrics.usage.totalInputTokens)} in / {formatTokens(metrics.usage.totalOutputTokens)} out</div>
      </div>
      <div class="card">
        <div class="card-label">Cache Hit Rate</div>
        <div class="card-value">{metrics.usage.cacheHitRate}%</div>
        <div class="card-sub">{formatTokens(metrics.usage.totalCacheReadTokens)} cached</div>
      </div>
      <div class="card">
        <div class="card-label">Turns</div>
        <div class="card-value">{metrics.usage.turnCount}</div>
      </div>
      {#if costs}
        <div class="card">
          <div class="card-label">Total Cost</div>
          <div class="card-value">{formatCost(costs.totalCost)}</div>
        </div>
      {/if}
      <div class="card">
        <div class="card-label">Services</div>
        <div class="card-value">
          {#if metrics.services.length > 0}
            {metrics.services.filter(s => s.healthy).length}/{metrics.services.length}
          {:else}
            -
          {/if}
        </div>
        <div class="card-sub badges">
          {#each metrics.services as svc}
            <Badge text={svc.name} variant={svc.healthy ? "success" : "error"} />
          {/each}
        </div>
      </div>
    </div>

    <div class="section">
      <h3>Agents</h3>
      <table>
        <thead>
          <tr>
            <th>Agent</th>
            <th>Sessions</th>
            <th>Messages</th>
            <th>Last Activity</th>
            <th>Tokens In</th>
            <th>Turns</th>
            {#if costs}
              <th>Cost</th>
            {/if}
          </tr>
        </thead>
        <tbody>
          {#each metrics.nous as agent}
            {@const agentCost = costs?.agents.find(a => a.agentId === agent.id)}
            <tr>
              <td class="agent-name">{agent.name}</td>
              <td>{agent.activeSessions}</td>
              <td>{agent.totalMessages}</td>
              <td class="muted">{formatTimeSince(agent.lastActivity)}</td>
              <td>{agent.tokens ? formatTokens(agent.tokens.input) : "-"}</td>
              <td>{agent.tokens?.turns ?? "-"}</td>
              {#if costs}
                <td>{agentCost ? formatCost(agentCost.totalCost || agentCost.cost) : "-"}</td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if metrics.cron.length > 0}
      <div class="section">
        <h3>Cron Jobs</h3>
        <table>
          <thead>
            <tr><th>Job</th><th>Schedule</th><th>Next Run</th><th>Last Run</th></tr>
          </thead>
          <tbody>
            {#each metrics.cron as job}
              <tr>
                <td class="mono">{job.id}</td>
                <td class="mono">{job.cron}</td>
                <td class="muted">{formatTimeSince(job.nextRun)}</td>
                <td class="muted">{formatTimeSince(job.lastRun)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
</div>

<style>
  .metrics-view {
    padding: 24px;
    overflow-y: auto;
    height: 100%;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }
  .card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
  }
  .card-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .card-value {
    font-size: 28px;
    font-weight: 700;
  }
  .card-sub {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 4px;
  }
  .badges {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    margin-top: 6px;
  }
  .section {
    margin-bottom: 24px;
  }
  .section h3 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    text-align: left;
    padding: 8px 12px;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: 12px;
  }
  td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  .agent-name {
    font-weight: 500;
  }
  .muted {
    color: var(--text-muted);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .loading, .error {
    padding: 32px;
    text-align: center;
    color: var(--text-muted);
  }
  .error {
    color: var(--red);
  }
</style>
