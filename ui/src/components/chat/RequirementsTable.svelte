<script lang="ts">
  interface Requirement {
    id: string;
    reqId: string;
    description: string;
    category: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale: string | null;
    status: string;
  }

  let { requirements }: {
    requirements: Requirement[];
  } = $props();

  let tierFilter = $state<"all" | "v1" | "v2" | "out-of-scope">("all");
  let categoryFilter = $state<string>("all");

  let categories = $derived(() => {
    const cats = [...new Set(requirements.map(r => r.category))];
    return cats.sort();
  });

  let filteredRequirements = $derived(() => {
    return requirements.filter(req => {
      if (tierFilter !== "all" && req.tier !== tierFilter) return false;
      if (categoryFilter !== "all" && req.category !== categoryFilter) return false;
      return true;
    });
  });

  let tierCounts = $derived(() => {
    return {
      v1: requirements.filter(r => r.tier === "v1").length,
      v2: requirements.filter(r => r.tier === "v2").length,
      outOfScope: requirements.filter(r => r.tier === "out-of-scope").length,
    };
  });

  function tierStyle(tier: string): string {
    switch (tier) {
      case "v1": return "tier-v1";
      case "v2": return "tier-v2";
      case "out-of-scope": return "tier-out-of-scope";
      default: return "";
    }
  }

  function tierLabel(tier: string): string {
    switch (tier) {
      case "v1": return "v1";
      case "v2": return "v2";
      case "out-of-scope": return "Out of Scope";
      default: return tier;
    }
  }
</script>

<div class="requirements-table">
  <div class="table-header">
    <h4>Requirements Overview</h4>
    <div class="filters">
      <div class="filter-group">
        <label for="tier-filter">Tier:</label>
        <select id="tier-filter" bind:value={tierFilter}>
          <option value="all">All ({requirements.length})</option>
          <option value="v1">v1 ({tierCounts.v1})</option>
          <option value="v2">v2 ({tierCounts.v2})</option>
          <option value="out-of-scope">Out of Scope ({tierCounts.outOfScope})</option>
        </select>
      </div>
      <div class="filter-group">
        <label for="category-filter">Category:</label>
        <select id="category-filter" bind:value={categoryFilter}>
          <option value="all">All Categories</option>
          {#each categories as category}
            <option value={category}>{category}</option>
          {/each}
        </select>
      </div>
    </div>
  </div>

  {#if filteredRequirements.length === 0}
    <div class="empty-state">
      <span>No requirements match the current filters</span>
      <button onclick={() => { tierFilter = "all"; categoryFilter = "all"; }}>
        Clear Filters
      </button>
    </div>
  {:else}
    <div class="table-container">
      <table class="requirements-grid">
        <thead>
          <tr>
            <th class="col-id">ID</th>
            <th class="col-description">Description</th>
            <th class="col-category">Category</th>
            <th class="col-tier">Tier</th>
            <th class="col-rationale">Rationale</th>
          </tr>
        </thead>
        <tbody>
          {#each filteredRequirements as req (req.id)}
            <tr class="requirement-row">
              <td class="col-id">
                <code class="req-id">{req.reqId}</code>
              </td>
              <td class="col-description">
                <span class="description-text">{req.description}</span>
              </td>
              <td class="col-category">
                <span class="category-badge">{req.category}</span>
              </td>
              <td class="col-tier">
                <span class="tier-badge {tierStyle(req.tier)}">{tierLabel(req.tier)}</span>
              </td>
              <td class="col-rationale">
                {#if req.rationale}
                  <span class="rationale-text">{req.rationale}</span>
                {:else}
                  <span class="no-rationale">—</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <div class="table-footer">
    <span class="results-count">
      Showing {filteredRequirements.length} of {requirements.length} requirements
    </span>
  </div>
</div>

<style>
  .requirements-table {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .table-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .table-header h4 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .filters {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }

  .filter-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .filter-group label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .filter-group select {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-size: var(--text-sm);
    min-width: 120px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 40px;
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .empty-state button {
    padding: 8px 16px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: var(--text-sm);
    transition: background var(--transition-quick);
  }

  .empty-state button:hover {
    background: var(--surface-hover);
  }

  .table-container {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
    background: var(--surface);
  }

  .requirements-grid {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .requirements-grid th {
    background: var(--bg-elevated);
    padding: 12px;
    text-align: left;
    font-weight: 600;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }

  .requirements-grid td {
    padding: 12px;
    border-bottom: 1px solid rgba(var(--border-rgb), 0.5);
    vertical-align: top;
  }

  .requirement-row:hover {
    background: rgba(var(--surface-hover-rgb), 0.5);
  }

  .requirement-row:last-child td {
    border-bottom: none;
  }

  .col-id {
    width: 80px;
  }

  .col-description {
    width: 40%;
    min-width: 200px;
  }

  .col-category {
    width: 120px;
  }

  .col-tier {
    width: 100px;
  }

  .col-rationale {
    width: 30%;
    min-width: 150px;
  }

  .req-id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--bg-elevated);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    color: var(--text);
    border: 1px solid var(--border);
  }

  .description-text {
    color: var(--text);
    line-height: 1.4;
  }

  .category-badge {
    background: var(--surface);
    color: var(--text-secondary);
    padding: 4px 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    border: 1px solid var(--border);
  }

  .tier-badge {
    padding: 4px 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .tier-badge.tier-v1 {
    background: var(--status-success-bg);
    color: var(--status-success);
    border: 1px solid var(--status-success-border);
  }

  .tier-badge.tier-v2 {
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
    border: 1px solid rgba(154, 123, 79, 0.3);
  }

  .tier-badge.tier-out-of-scope {
    background: var(--surface);
    color: var(--text-muted);
    border: 1px solid var(--border);
  }

  .rationale-text {
    color: var(--text-secondary);
    line-height: 1.4;
    font-style: italic;
  }

  .no-rationale {
    color: var(--text-muted);
  }

  .table-footer {
    display: flex;
    justify-content: center;
    padding: 12px;
    border-top: 1px solid var(--border);
    background: var(--bg-elevated);
    border-radius: var(--radius-md);
  }

  .results-count {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  @media (max-width: 768px) {
    .table-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }

    .filters {
      width: 100%;
      flex-direction: column;
      gap: 12px;
    }

    .filter-group select {
      min-width: 200px;
      width: 100%;
    }

    .table-container {
      overflow-x: auto;
    }

    .requirements-grid {
      min-width: 700px;
    }

    .col-description {
      min-width: 250px;
    }

    .col-rationale {
      min-width: 200px;
    }
  }
</style>