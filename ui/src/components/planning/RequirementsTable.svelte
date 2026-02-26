<script lang="ts">
  import { updateRequirement, createRequirement, deleteRequirement } from "../../stores/planning.svelte";

  interface Requirement {
    id: string;
    name: string;
    description: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale?: string;
    category: string;
  }

  let { requirements, projectId }: {
    requirements: Requirement[];
    projectId?: string;
  } = $props();

  let showV1 = $state(true);
  let showV2 = $state(true);
  let showOutOfScope = $state(false);
  let selectedCategory = $state<string | null>(null);
  let expandedReq = $state<string | null>(null);

  // Inline editing state
  let editingField = $state<{ reqId: string; field: "description" | "rationale" | "tier" } | null>(null);
  let editValue = $state("");
  let saving = $state(false);

  // Add requirement state
  let showAddForm = $state(false);
  let newReq = $state({ description: "", category: "", tier: "v1" as const });
  let addingSaving = $state(false);

  let filteredRequirements = $derived.by(() => {
    return requirements.filter(req => {
      if (req.tier === "v1" && !showV1) return false;
      if (req.tier === "v2" && !showV2) return false;
      if (req.tier === "out-of-scope" && !showOutOfScope) return false;
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

  // --- Inline editing ---

  function startEdit(reqId: string, field: "description" | "rationale", currentValue: string) {
    editingField = { reqId, field };
    editValue = currentValue;
  }

  function cancelEdit() {
    editingField = null;
    editValue = "";
  }

  async function saveEdit() {
    if (!editingField || !projectId || saving) return;
    saving = true;
    try {
      await updateRequirement(editingField.reqId, { [editingField.field]: editValue });
      editingField = null;
      editValue = "";
    } finally {
      saving = false;
    }
  }

  async function handleTierChange(reqId: string, newTier: "v1" | "v2" | "out-of-scope") {
    if (!projectId) return;
    await updateRequirement(reqId, { tier: newTier });
  }

  async function handleDelete(reqId: string, reqName: string) {
    if (!projectId) return;
    if (!confirm(`Delete requirement "${reqName}"?`)) return;
    await deleteRequirement(reqId);
    if (expandedReq === reqId) expandedReq = null;
  }

  async function handleAdd() {
    if (!projectId || addingSaving || !newReq.description.trim() || !newReq.category.trim()) return;
    addingSaving = true;
    try {
      await createRequirement({
        description: newReq.description.trim(),
        category: newReq.category.trim(),
        tier: newReq.tier,
      });
      newReq = { description: "", category: "", tier: "v1" };
      showAddForm = false;
    } finally {
      addingSaving = false;
    }
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      saveEdit();
    } else if (e.key === "Escape") {
      cancelEdit();
    }
  }
</script>

<div class="requirements-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">📋</span>
      Requirements
      <span class="requirement-count">({filteredRequirements.length})</span>
    </h2>
    {#if projectId}
      <button class="add-btn" onclick={() => { showAddForm = !showAddForm; }} title="Add requirement">
        {showAddForm ? "✕" : "+"}
      </button>
    {/if}
  </div>

  <!-- Add Requirement Form -->
  {#if showAddForm && projectId}
    <div class="add-form">
      <input
        type="text"
        class="add-input"
        placeholder="Description..."
        bind:value={newReq.description}
        onkeydown={(e) => { if (e.key === "Enter") handleAdd(); if (e.key === "Escape") showAddForm = false; }}
      />
      <div class="add-row">
        <input
          type="text"
          class="add-input small"
          placeholder="Category (e.g. EDIT)"
          bind:value={newReq.category}
          list="category-suggestions"
        />
        <datalist id="category-suggestions">
          {#each categories as cat}
            <option value={cat}></option>
          {/each}
        </datalist>
        <select class="add-select" bind:value={newReq.tier}>
          <option value="v1">V1</option>
          <option value="v2">V2</option>
          <option value="out-of-scope">Out of Scope</option>
        </select>
        <button class="add-submit" onclick={handleAdd} disabled={addingSaving || !newReq.description.trim() || !newReq.category.trim()}>
          {addingSaving ? "…" : "Add"}
        </button>
      </div>
    </div>
  {/if}

  <!-- Filter Controls -->
  <div class="filter-controls">
    <div class="tier-toggles">
      <label class="tier-toggle">
        <input type="checkbox" bind:checked={showV1} />
        <span class="tier-badge" style="--tier-color: {getTierColor('v1')}">V1 ({tierCounts.v1})</span>
      </label>
      <label class="tier-toggle">
        <input type="checkbox" bind:checked={showV2} />
        <span class="tier-badge" style="--tier-color: {getTierColor('v2')}">V2 ({tierCounts.v2})</span>
      </label>
      <label class="tier-toggle">
        <input type="checkbox" bind:checked={showOutOfScope} />
        <span class="tier-badge" style="--tier-color: {getTierColor('out-of-scope')}">Out of Scope ({tierCounts.outOfScope})</span>
      </label>
    </div>

    {#if categories.length > 1}
      <div class="category-filter">
        <select bind:value={selectedCategory}>
          <option value={null}>All Categories</option>
          {#each categories as category (category)}
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
            <div
              class="requirement-main"
              role="button"
              tabindex="0"
              onclick={() => toggleExpanded(req.id)}
              onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggleExpanded(req.id); } }}
            >
              <div class="requirement-content">
                <div class="requirement-header">
                  <span class="requirement-name">{req.name}</span>
                  <!-- EDIT-01: Tier dropdown -->
                  {#if projectId}
                    <select
                      class="tier-select"
                      style="--tier-color: {getTierColor(req.tier)}"
                      value={req.tier}
                      onclick={(e) => e.stopPropagation()}
                      onchange={(e) => handleTierChange(req.id, (e.currentTarget as HTMLSelectElement).value as "v1" | "v2" | "out-of-scope")}
                    >
                      <option value="v1">V1</option>
                      <option value="v2">V2</option>
                      <option value="out-of-scope">Out of Scope</option>
                    </select>
                  {:else}
                    <span class="tier-badge small" style="--tier-color: {getTierColor(req.tier)}">
                      {getTierLabel(req.tier)}
                    </span>
                  {/if}
                </div>
                <!-- EDIT-02: Inline description editing -->
                {#if editingField?.reqId === req.id && editingField.field === "description"}
                  <textarea
                    class="inline-edit"
                    bind:value={editValue}
                    onkeydown={handleEditKeydown}
                    onblur={saveEdit}
                    onclick={(e) => e.stopPropagation()}
                    rows="2"
                  ></textarea>
                {:else}
                  <div
                    class="requirement-description"
                    class:editable={!!projectId}
                    ondblclick={(e) => { if (projectId) { e.stopPropagation(); startEdit(req.id, "description", req.description); } }}
                    title={projectId ? "Double-click to edit" : ""}
                  >{req.description}</div>
                {/if}
                <div class="requirement-meta">
                  <span class="requirement-category">{req.category}</span>
                  <span class="requirement-id">{req.id}</span>
                </div>
              </div>
              <div class="row-actions">
                {#if projectId}
                  <button
                    class="delete-btn"
                    onclick={(e) => { e.stopPropagation(); handleDelete(req.id, req.name); }}
                    title="Delete requirement"
                  >🗑</button>
                {/if}
                <div class="expand-icon" class:rotated={expandedReq === req.id}>
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                    <path d="M6 12l4-4-4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                </div>
              </div>
            </div>
            
            {#if expandedReq === req.id}
              <div class="requirement-details">
                <div class="rationale-section">
                  <strong>Rationale:</strong>
                  {#if editingField?.reqId === req.id && editingField.field === "rationale"}
                    <textarea
                      class="inline-edit"
                      bind:value={editValue}
                      onkeydown={handleEditKeydown}
                      onblur={saveEdit}
                      rows="3"
                    ></textarea>
                  {:else}
                    <p
                      class:editable={!!projectId}
                      ondblclick={() => { if (projectId) startEdit(req.id, "rationale", req.rationale ?? ""); }}
                      title={projectId ? "Double-click to edit" : ""}
                    >{req.rationale || "No rationale provided"}</p>
                  {/if}
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
    display: flex;
    align-items: center;
    justify-content: space-between;
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

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-lg);
    font-weight: 600;
    cursor: pointer;
    transition: background var(--transition-quick);
    line-height: 1;
  }

  .add-btn:hover {
    background: var(--accent-hover);
  }

  /* Add form */
  .add-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    margin-bottom: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
  }

  .add-input {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }

  .add-input.small {
    flex: 1;
    min-width: 0;
  }

  .add-input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .add-row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .add-select {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: var(--space-2);
    font-size: var(--text-sm);
  }

  .add-submit {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .add-submit:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Tier dropdown (EDIT-01) */
  .tier-select {
    appearance: none;
    -webkit-appearance: none;
    background: color-mix(in srgb, var(--tier-color) 15%, transparent);
    color: var(--tier-color);
    border: 1px solid color-mix(in srgb, var(--tier-color) 30%, transparent);
    border-radius: var(--radius-pill);
    padding: 1px var(--space-2) 1px var(--space-1);
    font-size: var(--text-2xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .tier-select:hover {
    border-color: var(--tier-color);
  }

  .tier-select:focus {
    outline: none;
    border-color: var(--accent);
  }

  /* Inline editing (EDIT-02) */
  .inline-edit {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
    font-family: inherit;
    line-height: 1.4;
    resize: vertical;
  }

  .inline-edit:focus {
    outline: none;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 25%, transparent);
  }

  .editable {
    cursor: text;
    border-radius: var(--radius-sm);
    padding: 1px 2px;
    margin: -1px -2px;
    transition: background var(--transition-quick);
  }

  .editable:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .row-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
    margin-left: var(--space-2);
  }

  .delete-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    opacity: 0;
    transition: all var(--transition-quick);
  }

  .requirement-row:hover .delete-btn {
    opacity: 0.6;
  }

  .delete-btn:hover {
    opacity: 1 !important;
    background: color-mix(in srgb, var(--status-error) 10%, transparent);
    border-color: var(--status-error);
    color: var(--status-error);
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

    .add-row {
      flex-wrap: wrap;
    }
  }
</style>
