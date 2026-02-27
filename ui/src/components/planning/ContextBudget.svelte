<script lang="ts">
  import { authFetch } from "./api";

  let { projectId }: { projectId: string } = $props();

  interface BudgetAllocation {
    totalBudget: number;
    projectTokens: number;
    roadmapTokens: number;
    phaseStatusTokens: number;
    handoffTokens: number;
    totalConsumed: number;
    remaining: number;
    utilizationPercent: number;
    overBudget: boolean;
    breakdown: Array<{ label: string; tokens: number; percent: number }>;
    warnings: string[];
  }

  let budget = $state<BudgetAllocation | null>(null);
  let expanded = $state(false);

  async function fetchBudget() {
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/budget`);
      if (res.ok) {
        budget = await res.json() as BudgetAllocation;
      }
    } catch { /* best effort */ }
  }

  function formatTokens(n: number): string {
    if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
    return String(n);
  }

  function barColor(pct: number): string {
    if (pct > 90) return "#ff4444";
    if (pct > 70) return "#ffaa00";
    return "#51cf66";
  }

  $effect(() => {
    void projectId;
    fetchBudget();
    const iv = setInterval(fetchBudget, 15000);
    return () => clearInterval(iv);
  });
</script>

{#if budget}
  <div class="budget-indicator">
    <button class="budget-bar-btn" onclick={() => { expanded = !expanded; }}>
      <div class="bar-label">
        <span class="budget-icon">📊</span>
        <span>Context Budget</span>
        <span class="budget-summary" class:over={budget.overBudget}>
          {formatTokens(budget.totalConsumed)} / {formatTokens(budget.totalBudget)}
        </span>
        {#if budget.warnings.length > 0}
          <span class="warning-badge">⚠️ {budget.warnings.length}</span>
        {/if}
      </div>
      <div class="bar-track">
        <div
          class="bar-fill"
          style="width: {Math.min(budget.utilizationPercent, 100)}%; background: {barColor(budget.utilizationPercent)}"
        ></div>
      </div>
    </button>

    {#if expanded}
      <div class="budget-details">
        {#if budget.breakdown}
          {#each budget.breakdown as item (item.label)}
            <div class="detail-row">
              <span class="detail-label">{item.label}</span>
              <span class="detail-tokens">{formatTokens(item.tokens)}</span>
              <div class="mini-bar">
                <div class="mini-fill" style="width: {item.percent}%"></div>
              </div>
              <span class="detail-pct">{item.percent.toFixed(0)}%</span>
            </div>
          {/each}
        {/if}
        <div class="detail-row remaining">
          <span class="detail-label">Remaining</span>
          <span class="detail-tokens" class:over={budget.overBudget}>{formatTokens(budget.remaining)}</span>
        </div>
        {#if budget.warnings.length > 0}
          <div class="warnings">
            {#each budget.warnings as warning}
              <div class="warning-item">⚠️ {warning}</div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .budget-indicator {
    border: 1px solid var(--border, #333);
    border-radius: 6px;
    background: var(--bg-secondary, #1a1a2e);
    overflow: hidden;
  }

  .budget-bar-btn {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
  }

  .budget-bar-btn:hover {
    background: var(--bg-tertiary, #0f0f23);
  }

  .bar-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8rem;
    color: var(--text-secondary, #999);
  }

  .budget-icon {
    font-size: 0.85rem;
  }

  .budget-summary {
    margin-left: auto;
    font-family: monospace;
    font-size: 0.75rem;
    color: var(--text-primary, #e0e0e0);
  }

  .budget-summary.over {
    color: #ff4444;
    font-weight: 700;
  }

  .warning-badge {
    font-size: 0.7rem;
  }

  .bar-track {
    width: 100%;
    height: 4px;
    background: var(--bg-tertiary, #0f0f23);
    border-radius: 2px;
    overflow: hidden;
  }

  .bar-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .budget-details {
    border-top: 1px solid var(--border, #333);
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .detail-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.75rem;
  }

  .detail-label {
    color: var(--text-secondary, #999);
    min-width: 100px;
  }

  .detail-tokens {
    color: var(--text-primary, #e0e0e0);
    font-family: monospace;
    min-width: 50px;
    text-align: right;
  }

  .detail-tokens.over {
    color: #ff4444;
  }

  .mini-bar {
    flex: 1;
    height: 3px;
    background: var(--bg-tertiary, #0f0f23);
    border-radius: 2px;
    overflow: hidden;
  }

  .mini-fill {
    height: 100%;
    background: var(--accent, #6c63ff);
    border-radius: 2px;
  }

  .detail-pct {
    color: var(--text-secondary, #666);
    font-size: 0.65rem;
    min-width: 30px;
    text-align: right;
  }

  .remaining {
    margin-top: 4px;
    padding-top: 4px;
    border-top: 1px solid var(--border, #222);
  }

  .warnings {
    margin-top: 4px;
    padding-top: 4px;
    border-top: 1px solid var(--border, #222);
  }

  .warning-item {
    color: #ffaa00;
    font-size: 0.7rem;
    padding: 2px 0;
  }
</style>
