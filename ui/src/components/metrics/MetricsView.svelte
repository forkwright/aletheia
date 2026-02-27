<script lang="ts">
  import { fetchMetrics, fetchCredentialInfo } from "../../lib/api";
  import type { CredentialInfo } from "../../lib/api";
  import { formatTokens, formatUptime, formatTimeSince } from "../../lib/format";
  import Badge from "../shared/Badge.svelte";
  import UsageChart from "./UsageChart.svelte";
  import type { MetricsData } from "../../lib/types";

  let metrics = $state<MetricsData | null>(null);
  let creds = $state<CredentialInfo | null>(null);
  let error = $state<string | null>(null);

  function credStatus(c: CredentialInfo): { label: string; variant: "success" | "warning" | "error" } {
    if (c.primary.isExpired) return { label: "expired", variant: "error" };
    if (c.primary.expiresInMs !== undefined && c.primary.expiresInMs < 86_400_000) return { label: "expiring", variant: "warning" };
    if (c.backups.length === 0) return { label: "no backup", variant: "warning" };
    return { label: "ok", variant: "success" };
  }

  async function load() {
    try {
      const [m, cr] = await Promise.all([fetchMetrics(), fetchCredentialInfo()]);
      metrics = m;
      creds = cr;
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
        <div class="card-sub">{formatTokens(metrics.usage.totalCacheReadTokens)} from cache</div>
      </div>
      <div class="card">
        <div class="card-label">Turns</div>
        <div class="card-value">{metrics.usage.turnCount}</div>
      </div>
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
          {#each metrics.services as svc (svc.name)}
            <Badge text={svc.name} variant={svc.healthy ? "success" : "error"} />
          {/each}
        </div>
      </div>
      {#if creds}
        {@const status = credStatus(creds)}
        <div class="card">
          <div class="card-label">Credentials</div>
          <div class="card-value card-value-sm">
            <span class="mono">{creds.primary.label}</span>
          </div>
          <div class="card-sub">
            <Badge text={creds.primary.type} variant="default" />
            <Badge text={status.label} variant={status.variant} />
          </div>
          {#if creds.backups.length > 0}
            <div class="card-sub" style="margin-top: 4px">
              {creds.backups.length} backup{creds.backups.length > 1 ? "s" : ""}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="section">
      <UsageChart />
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
          </tr>
        </thead>
        <tbody>
          {#each metrics.nous as agent (agent.name)}
            <tr>
              <td class="agent-name">{agent.name}</td>
              <td>{agent.activeSessions}</td>
              <td>{agent.totalMessages}</td>
              <td class="muted">{formatTimeSince(agent.lastActivity)}</td>
              <td>{agent.tokens ? formatTokens(agent.tokens.input) : "-"}</td>
              <td>{agent.tokens?.turns ?? "-"}</td>
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
            {#each metrics.cron as job (job.id)}
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
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .card-value {
    font-size: var(--text-3xl);
    font-weight: 700;
  }
  .card-value-sm {
    font-size: var(--text-lg);
  }
  .card-sub {
    font-size: var(--text-sm);
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
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th {
    text-align: left;
    padding: 8px 12px;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: var(--text-sm);
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
    font-size: var(--text-sm);
  }
  .loading, .error {
    padding: 32px;
    text-align: center;
    color: var(--text-muted);
  }
  .error {
    color: var(--status-error);
  }
</style>
