<script lang="ts">
  /**
   * CategoryProposal — EDIT-04
   *
   * Dedicated UI for the category proposal flow. Replaces chat-based tables
   * with interactive cards where users can approve/adjust feature tiers.
   *
   * Flow: present → user reviews → persist decisions
   * API: POST /categories/present, POST /categories/persist, PATCH /categories/:code
   */
  import Spinner from "../shared/Spinner.svelte";
  import { authFetch } from "./api";

  interface FeatureProposal {
    name: string;
    description: string;
    isTableStakes: boolean;
    proposedTier: "v1" | "v2" | "out-of-scope";
    proposedRationale?: string;
  }

  interface CategoryData {
    category: string;
    categoryName: string;
    tableStakes: FeatureProposal[];
    differentiators: FeatureProposal[];
  }

  interface Decision {
    name: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale: string;
    isTableStakes: boolean;
    changed: boolean; // user overrode the proposed tier
  }

  let { projectId, category, onComplete }: {
    projectId: string;
    category: CategoryData;
    onComplete?: () => void;
  } = $props();

  // Initialize decisions from proposal
  let decisions = $state<Decision[]>(
    [...category.tableStakes, ...category.differentiators].map(f => ({
      name: f.name,
      tier: f.proposedTier,
      rationale: f.proposedRationale ?? "",
      isTableStakes: f.isTableStakes,
      changed: false,
    }))
  );

  let persisting = $state(false);
  let error = $state<string | null>(null);
  let editingRationale = $state<string | null>(null);

  // Derived
  let v1Count = $derived(decisions.filter(d => d.tier === "v1").length);
  let v2Count = $derived(decisions.filter(d => d.tier === "v2").length);
  let oosCount = $derived(decisions.filter(d => d.tier === "out-of-scope").length);
  let changedCount = $derived(decisions.filter(d => d.changed).length);
  let hasTableStakesViolation = $derived(
    decisions.some(d => d.isTableStakes && d.tier === "out-of-scope" && !d.rationale.trim())
  );

  function changeTier(index: number, newTier: "v1" | "v2" | "out-of-scope") {
    const all = [...category.tableStakes, ...category.differentiators];
    const original = all[index]?.proposedTier;
    decisions = decisions.map((d, i) =>
      i === index ? { ...d, tier: newTier, changed: newTier !== original } : d
    );
  }

  function updateRationale(index: number, rationale: string) {
    decisions = decisions.map((d, i) =>
      i === index ? { ...d, rationale } : d
    );
  }

  function approveAll() {
    decisions = decisions.map(d => ({ ...d, changed: false }));
  }

  function allToV1() {
    const all = [...category.tableStakes, ...category.differentiators];
    decisions = decisions.map((d, i) => ({
      ...d,
      tier: "v1" as const,
      changed: all[i]?.proposedTier !== "v1",
    }));
  }

  async function persist() {
    if (hasTableStakesViolation) {
      error = "Table-stakes features marked out-of-scope require a rationale.";
      return;
    }

    persisting = true;
    error = null;

    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/categories/persist`, {
        method: "POST",
        body: JSON.stringify({
          category: {
            category: category.category,
            categoryName: category.categoryName,
            tableStakes: category.tableStakes,
            differentiators: category.differentiators,
          },
          decisions: decisions.map(d => ({
            name: d.name,
            tier: d.tier,
            ...(d.rationale.trim() && { rationale: d.rationale }),
          })),
        }),
      });

      if (!res.ok) {
        const data = await res.json().catch(() => ({})) as { error?: string };
        throw new Error(data.error ?? `Persist failed: ${res.status}`);
      }

      onComplete?.();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      persisting = false;
    }
  }

  function tierColor(tier: "v1" | "v2" | "out-of-scope"): string {
    if (tier === "v1") return "var(--status-success)";
    if (tier === "v2") return "var(--status-warning)";
    return "var(--text-muted)";
  }

  function tierLabel(tier: "v1" | "v2" | "out-of-scope"): string {
    if (tier === "v1") return "V1";
    if (tier === "v2") return "V2";
    return "Out";
  }
</script>

<div class="category-proposal">
  <div class="proposal-header">
    <h3>{category.categoryName} <span class="category-code">({category.category})</span></h3>
    <div class="tier-summary">
      <span class="tier-badge v1">{v1Count} V1</span>
      <span class="tier-badge v2">{v2Count} V2</span>
      <span class="tier-badge oos">{oosCount} Out</span>
      {#if changedCount > 0}
        <span class="changes-badge">{changedCount} changed</span>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <!-- Table Stakes -->
  {#if category.tableStakes.length > 0}
    <div class="feature-group">
      <div class="group-label">
        <span class="group-icon">🔒</span>
        Table Stakes
        <span class="group-count">{category.tableStakes.length}</span>
      </div>
      {#each category.tableStakes as feature, i}
        {@const decision = decisions[i]!}
        <div class="feature-card" class:changed={decision.changed} class:violation={decision.isTableStakes && decision.tier === "out-of-scope" && !decision.rationale.trim()}>
          <div class="feature-main">
            <div class="feature-info">
              <span class="feature-name">{feature.name}</span>
              <span class="feature-desc">{feature.description}</span>
            </div>
            <div class="tier-selector">
              {#each ["v1", "v2", "out-of-scope"] as tier}
                <button
                  class="tier-btn"
                  class:active={decision.tier === tier}
                  style:--tier-color={tierColor(tier as "v1" | "v2" | "out-of-scope")}
                  onclick={() => changeTier(i, tier as "v1" | "v2" | "out-of-scope")}
                >
                  {tierLabel(tier as "v1" | "v2" | "out-of-scope")}
                </button>
              {/each}
            </div>
          </div>
          {#if decision.tier === "out-of-scope" && decision.isTableStakes}
            <div class="rationale-required">
              <span class="warning-icon">⚠️</span>
              <input
                type="text"
                placeholder="Rationale required for table-stakes exclusion..."
                value={decision.rationale}
                oninput={(e) => updateRationale(i, (e.target as HTMLInputElement).value)}
                class="rationale-input"
              />
            </div>
          {:else if editingRationale === decision.name}
            <div class="rationale-edit">
              <input
                type="text"
                placeholder="Add rationale..."
                value={decision.rationale}
                oninput={(e) => updateRationale(i, (e.target as HTMLInputElement).value)}
                onkeydown={(e) => { if (e.key === "Enter" || e.key === "Escape") editingRationale = null; }}
                class="rationale-input"
              />
              <button class="rationale-done" onclick={() => { editingRationale = null; }}>✓</button>
            </div>
          {:else}
            <button class="add-rationale" onclick={() => { editingRationale = decision.name; }}>
              {decision.rationale ? `📝 ${decision.rationale}` : "+ rationale"}
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Differentiators -->
  {#if category.differentiators.length > 0}
    <div class="feature-group">
      <div class="group-label">
        <span class="group-icon">✨</span>
        Differentiators
        <span class="group-count">{category.differentiators.length}</span>
      </div>
      {#each category.differentiators as feature, j}
        {@const i = category.tableStakes.length + j}
        {@const decision = decisions[i]!}
        <div class="feature-card" class:changed={decision.changed}>
          <div class="feature-main">
            <div class="feature-info">
              <span class="feature-name">{feature.name}</span>
              <span class="feature-desc">{feature.description}</span>
            </div>
            <div class="tier-selector">
              {#each ["v1", "v2", "out-of-scope"] as tier}
                <button
                  class="tier-btn"
                  class:active={decision.tier === tier}
                  style:--tier-color={tierColor(tier as "v1" | "v2" | "out-of-scope")}
                  onclick={() => changeTier(i, tier as "v1" | "v2" | "out-of-scope")}
                >
                  {tierLabel(tier as "v1" | "v2" | "out-of-scope")}
                </button>
              {/each}
            </div>
          </div>
          {#if editingRationale === decision.name}
            <div class="rationale-edit">
              <input
                type="text"
                placeholder="Add rationale..."
                value={decision.rationale}
                oninput={(e) => updateRationale(i, (e.target as HTMLInputElement).value)}
                onkeydown={(e) => { if (e.key === "Enter" || e.key === "Escape") editingRationale = null; }}
                class="rationale-input"
              />
              <button class="rationale-done" onclick={() => { editingRationale = null; }}>✓</button>
            </div>
          {:else}
            <button class="add-rationale" onclick={() => { editingRationale = decision.name; }}>
              {decision.rationale ? `📝 ${decision.rationale}` : "+ rationale"}
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Actions -->
  <div class="proposal-actions">
    <div class="quick-actions">
      <button class="action-btn secondary" onclick={approveAll}>Approve as proposed</button>
      <button class="action-btn secondary" onclick={allToV1}>All to V1</button>
    </div>
    <button
      class="action-btn primary"
      disabled={persisting || hasTableStakesViolation}
      onclick={persist}
    >
      {#if persisting}
        <Spinner size={14} /> Persisting...
      {:else}
        Confirm {decisions.length} decisions
      {/if}
    </button>
  </div>
</div>

<style>
  .category-proposal {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .proposal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--border);
  }

  .proposal-header h3 {
    margin: 0;
    font-size: var(--text-lg);
    color: var(--text);
  }

  .category-code {
    color: var(--text-muted);
    font-weight: normal;
    font-size: var(--text-sm);
  }

  .tier-summary {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .tier-badge {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .tier-badge.v1 { background: color-mix(in srgb, var(--status-success) 20%, transparent); color: var(--status-success); }
  .tier-badge.v2 { background: color-mix(in srgb, var(--status-warning) 20%, transparent); color: var(--status-warning); }
  .tier-badge.oos { background: color-mix(in srgb, var(--text-muted) 20%, transparent); color: var(--text-muted); }
  .changes-badge { background: color-mix(in srgb, var(--status-active) 20%, transparent); color: var(--status-active); padding: 2px 8px; border-radius: 10px; font-size: var(--text-xs); font-weight: 600; }

  .error-banner {
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--status-error) 15%, transparent);
    color: var(--status-error);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }

  .feature-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .group-label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .group-icon { font-size: var(--text-base); }
  .group-count {
    margin-left: auto;
    font-weight: normal;
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--bg);
    border: 1px solid var(--border);
  }

  .feature-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    transition: border-color 0.15s;
  }

  .feature-card.changed {
    border-color: var(--status-active);
    background: color-mix(in srgb, var(--status-active) 5%, var(--bg));
  }

  .feature-card.violation {
    border-color: var(--status-error);
    background: color-mix(in srgb, var(--status-error) 5%, var(--bg));
  }

  .feature-main {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-3);
  }

  .feature-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .feature-name {
    font-weight: 600;
    font-size: var(--text-sm);
    color: var(--text);
  }

  .feature-desc {
    font-size: var(--text-xs);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tier-selector {
    display: flex;
    gap: 2px;
    background: var(--bg-elevated);
    border-radius: var(--radius);
    padding: 2px;
    flex-shrink: 0;
  }

  .tier-btn {
    padding: 3px 10px;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tier-btn:hover {
    background: color-mix(in srgb, var(--tier-color) 15%, transparent);
    color: var(--tier-color);
  }

  .tier-btn.active {
    background: color-mix(in srgb, var(--tier-color) 20%, transparent);
    color: var(--tier-color);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tier-color) 30%, transparent);
  }

  .rationale-required {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .warning-icon { font-size: var(--text-sm); }

  .rationale-input {
    flex: 1;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 8px;
    font-size: var(--text-xs);
    color: var(--text);
  }

  .rationale-input:focus {
    outline: none;
    border-color: var(--status-active);
  }

  .rationale-edit {
    display: flex;
    gap: var(--space-1);
    align-items: center;
  }

  .rationale-done {
    padding: 4px 8px;
    background: var(--status-success);
    color: white;
    border: none;
    border-radius: var(--radius);
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .add-rationale {
    background: none;
    border: none;
    padding: 2px 0;
    font-size: var(--text-xs);
    color: var(--text-muted);
    cursor: pointer;
    text-align: left;
  }

  .add-rationale:hover {
    color: var(--text);
  }

  .proposal-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .quick-actions {
    display: flex;
    gap: var(--space-2);
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 14px;
    border: none;
    border-radius: var(--radius);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
  }

  .action-btn.primary {
    background: var(--status-active);
    color: white;
  }

  .action-btn.primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .action-btn.primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.secondary {
    background: var(--bg-elevated);
    color: var(--text-muted);
    border: 1px solid var(--border);
  }

  .action-btn.secondary:hover {
    color: var(--text);
    border-color: var(--text-muted);
  }

  @media (max-width: 600px) {
    .feature-main {
      flex-direction: column;
      align-items: flex-start;
    }

    .tier-selector {
      width: 100%;
      justify-content: center;
    }

    .proposal-actions {
      flex-direction: column;
      gap: var(--space-2);
    }

    .quick-actions {
      width: 100%;
    }

    .action-btn {
      flex: 1;
      justify-content: center;
    }
  }
</style>
