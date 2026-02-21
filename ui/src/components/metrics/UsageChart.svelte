<script lang="ts">
  import { fetchDailyCosts } from "../../lib/api";
  import { formatCost } from "../../lib/format";
  import type { DailyCost } from "../../lib/types";
  import { onMount } from "svelte";

  let data = $state<DailyCost[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let maxCost = $derived(Math.max(...data.map((d) => d.cost), 0.001));

  onMount(async () => {
    try {
      data = await fetchDailyCosts(30);
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to load";
    } finally {
      loading = false;
    }
  });
</script>

<div class="usage-chart">
  <h3>Daily Cost (30d)</h3>
  {#if loading}
    <div class="chart-empty">Loading...</div>
  {:else if error}
    <div class="chart-empty chart-error">{error}</div>
  {:else if data.length === 0}
    <div class="chart-empty">No usage data</div>
  {:else}
    <div class="chart-container">
      <div class="chart-bars">
        {#each data as day}
          {@const pct = (day.cost / maxCost) * 100}
          <div class="bar-col" title="{day.date}: {formatCost(day.cost)} · {day.turns} turns">
            <div class="bar" style="height: {Math.max(pct, 2)}%"></div>
          </div>
        {/each}
      </div>
      <div class="chart-labels">
        <span>{data[0]?.date.slice(5)}</span>
        <span>{data[data.length - 1]?.date.slice(5)}</span>
      </div>
      <div class="chart-summary">
        Total: {formatCost(data.reduce((s, d) => s + d.cost, 0))} · {data.reduce((s, d) => s + d.turns, 0)} turns
      </div>
    </div>
  {/if}
</div>

<style>
  .usage-chart h3 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 12px;
  }
  .chart-empty {
    padding: 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
  .chart-error {
    color: var(--red);
  }
  .chart-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .chart-bars {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 120px;
    padding: 0 2px;
  }
  .bar-col {
    flex: 1;
    display: flex;
    align-items: flex-end;
    height: 100%;
    min-width: 0;
  }
  .bar {
    width: 100%;
    background: var(--accent);
    border-radius: 2px 2px 0 0;
    min-height: 2px;
    transition: height 0.3s ease;
    opacity: 0.8;
  }
  .bar-col:hover .bar {
    opacity: 1;
  }
  .chart-labels {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    padding: 0 2px;
  }
  .chart-summary {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
    margin-top: 4px;
  }
</style>
