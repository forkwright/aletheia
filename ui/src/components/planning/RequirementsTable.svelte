<script lang="ts">
  interface Requirement {
    id: string;
    name: string;
    description: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale?: string;
    category: string;
  }

  let { requirements }: { requirements: Requirement[] } = $props();

  let showV1 = $state(true);
  let showV2 = $state(true);
  let showOutOfScope = $state(false);
  let selectedCategory = $state<string | null>(null);
  let expandedReq = $state<string | null>(null);

  let filteredRequirements = $derived.by(() => {
    return requirements.filter(req => {
      // Tier filter
      if (req.tier === "v1" && !showV1) return false;
      if (req.tier === "v2" && !showV2) return false;
      if (req.tier === "out-of-scope" && !showOutOfScope) return false;
      
      // Category filter
      if (selectedCategory && req.category !== selectedCategory) return false;
      
      return true;
    });
  });

  let categories = $derived.by(() => {
    const cats = [...new Set(requirements.map(r => r.category))].sort();
    return cats;
  });

  let tierCounts = $derived.by(() => {
    return {
      v1: requirements.filter(r => r.tier === "v1").length,
      v2: requirements.filter(r => r.tier === "v2").length,
      outOfScope: requirements.filter(r => r.tier === "out-of-scope").length,
    };
  });

  function getTierColor(tier: "v1" | "v2" | "out-of-scope"): string {
    switch (tier) {
      case "v1": return "var(--status-success)";
      case "v2": return "var(--status-warning)";
      case "out-of-scope": return "var(--text-muted)";
      default: return "var(--text-muted)";
    }
  }

  function getTierLabel(tier: "v1" | "v2" | "out-of-scope"): string {
    switch (tier) {
      case "v1": return "V1";
      case "v2": return "V2";
      case "out-of-scope": return "Out of Scope";
      default: return tier;
    }
  }

  function toggleExpanded(reqId: string) {
    expandedReq = expandedReq === reqId ? null : reqId;
  }
</script>

<div class="requirements-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">📋</span>
      Requirements
      <span class="requirement-count">({filteredRequirements.length})</span>
    </h2>
  </div>

  <!-- Filter Controls -->
  <div class="filter-controls">
    <div class="tier-toggles">
      <label class="tier-toggle">
        <input 
          type="checkbox" 
          bind:checked={showV1}
        />
        <span class="tier-badge" style="--tier-color: {getTierColor('v1')}">
          V1 ({tierCounts.v1})
        </span>
      </label>

      <label class="tier-toggle">
        <input 
          type="checkbox" 
          bind:checked={showV2}
        />
        <span class="tier-badge" style="--tier-color: {getTierColor('v2')}">
          V2 ({tierCounts.v2})
        </span>
      </label>

      <label class="tier-toggle">
        <input 
          type="checkbox" 
          bind:checked={showOutOfScope}
        />
        <span class="tier-badge" style="--tier-color: {getTierColor('out-of-scope')}">
          Out of Scope ({tierCounts.outOfScope})
        </span>
      </label>
    </div>

    {#if categories.length > 1}
      <div class="category-filter">
        <select bind:value={selectedCategory}>
          <option value={null}>All Categories</option>
          {#each categories as category}
            <option value={category}>{category}</option>
          {/each}
        </select>
      </div>
    {/if}
  </div>

  <!-- Requirements Table -->
  <div class="requirements-table-container">
    {#if filteredRequirements.length === 0}
      <div class="empty-requirements">
        <span class="empty-icon">🔍</span>
        <span>No requirements match the current filters</span>
      </div>
    {:else}
      <div class="requirements-table">
        {#each filteredRequirements as req (req.id)}
          <div class="requirement-row" class:expanded={expandedReq === req.id}>
            <div class="requirement-main" onclick={() => toggleExpanded(req.id)}>
              <div class="requirement-content">
                <div class="requirement-header">
                  <span class="requirement-name">{req.name}</span>
                  <span class="tier-badge small" style="--tier-color: {getTierColor(req.tier)}">
                    {getTierLabel(req.tier)}
                  </span>
                </div>
                <div class="requirement-description">{req.description}</div>
                <div class="requirement-meta">
                  <span class="requirement-category">{req.category}</span>
                  <span class="requirement-id">{req.id}</span>
                </div>
              </div>
              <div class="expand-icon" class:rotated={expandedReq === req.id}>
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                  <path 
                    d="M6 12l4-4-4-4" 
                    stroke="currentColor" 
                    stroke-width="1.5" 
                    stroke-linecap="round" 
                    stroke-linejoin="round"
                  />
                </svg>
              </div>
            </div>
            
            {#if expandedReq === req.id && req.rationale}
              <div class="requirement-details">
                <div class="rationale-section">
                  <strong>Rationale:</strong>
                  <p>{req.rationale}</p>
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .requirements-section {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .section-header {
    margin-bottom: var(--space-3);
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
    margin: 0;
  }

  .title-icon {
    font-size: var(--text-xl);
  }

  .requirement-count {
    color: var(--text-muted);
    font-weight: 400;
  }

  .filter-controls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  .tier-toggles {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .tier-toggle {
    display: flex;
    align-items: center;
    cursor: pointer;
  }

  .tier-toggle input[type="checkbox"] {
    display: none;
  }

  .tier-badge {
    display: inline-flex;
    align-items: center;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: color-mix(in srgb, var(--tier-color) 15%, transparent);
    color: var(--tier-color);
    border: 1px solid color-mix(in srgb, var(--tier-color) 30%, transparent);
    transition: all var(--transition-quick);
  }

  .tier-badge.small {
    padding: 1px var(--space-1);
    font-size: var(--text-2xs);
  }

  .tier-toggle input[type="checkbox"]:not(:checked) + .tier-badge {
    background: var(--surface);
    color: var(--text-muted);
    border-color: var(--border);
    opacity: 0.5;
  }

  .tier-toggle:hover input[type="checkbox"]:not(:checked) + .tier-badge {
    opacity: 0.8;
  }

  .category-filter select {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .category-filter select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .requirements-table-container {
    flex: 1;
    overflow-y: auto;
  }

  .empty-requirements {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-6);
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .empty-icon {
    font-size: var(--text-lg);
  }

  .requirements-table {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .requirement-row {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    transition: border-color var(--transition-quick);
  }

  .requirement-row.expanded {
    border-color: var(--accent);
  }

  .requirement-main {
    display: flex;
    align-items: center;
    padding: var(--space-3);
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .requirement-main:hover {
    background: var(--surface);
  }

  .requirement-content {
    flex: 1;
    min-width: 0;
  }

  .requirement-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .requirement-name {
    font-weight: 600;
    color: var(--text);
    flex: 1;
    min-width: 0;
  }

  .requirement-description {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    line-height: 1.4;
    margin-bottom: var(--space-1);
  }

  .requirement-meta {
    display: flex;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .requirement-category {
    font-weight: 500;
  }

  .requirement-id {
    font-family: var(--font-mono);
  }

  .expand-icon {
    color: var(--text-muted);
    transition: transform var(--transition-quick), color var(--transition-quick);
    flex-shrink: 0;
    margin-left: var(--space-2);
  }

  .expand-icon.rotated {
    transform: rotate(90deg);
    color: var(--accent);
  }

  .requirement-details {
    border-top: 1px solid var(--border);
    padding: var(--space-3);
    background: var(--surface);
  }

  .rationale-section strong {
    color: var(--text);
    font-weight: 600;
  }

  .rationale-section p {
    margin: var(--space-1) 0 0 0;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  @media (max-width: 768px) {
    .filter-controls {
      flex-direction: column;
      gap: var(--space-2);
    }

    .requirement-main {
      padding: var(--space-2);
    }

    .requirement-header {
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-1);
    }

    .requirement-meta {
      flex-direction: column;
      gap: var(--space-1);
    }
  }
</style>