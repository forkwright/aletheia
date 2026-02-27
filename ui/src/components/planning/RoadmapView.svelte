<script lang="ts">
  import { updatePhase, deletePhase, reorderPhase } from "../../stores/planning.svelte";
  import AnnotationPanel from "./AnnotationPanel.svelte";

  interface Phase {
    id: string;
    name: string;
    goal: string;
    dependencies: string[];
    requirements: string[];
    state: "pending" | "active" | "complete" | "blocked";
    order: number;
  }

  let { phases, currentState, projectId }: { 
    phases: Phase[];
    currentState: string;
    projectId?: string;
  } = $props();

  let sortedPhases = $derived.by(() => {
    return [...phases].sort((a, b) => a.order - b.order);
  });

  let expandedPhase = $state<string | null>(null);

  // Inline editing state
  let editingField = $state<{ phaseId: string; field: "name" | "goal" } | null>(null);
  let editValue = $state("");
  let saving = $state(false);

  // Drag reorder state
  let dragPhaseId = $state<string | null>(null);
  let dragOverPhaseId = $state<string | null>(null);

  function getPhaseStatus(phase: Phase, _index: number): "current" | "completed" | "pending" | "blocked" {
    if (phase.state === "blocked") return "blocked";
    if (phase.state === "complete") return "completed";
    if (phase.state === "active") return "current";
    if (currentState === "discussing" || currentState === "planning" || currentState === "executing" || currentState === "verifying") {
      if (_index === 0) return "current";
    }
    return "pending";
  }

  function getStatusColor(status: "current" | "completed" | "pending" | "blocked"): string {
    switch (status) {
      case "completed": return "var(--status-success)";
      case "current": return "var(--status-active)";
      case "blocked": return "var(--status-error)";
      case "pending": return "var(--text-muted)";
      default: return "var(--text-muted)";
    }
  }

  function getStatusIcon(status: "current" | "completed" | "pending" | "blocked"): string {
    switch (status) {
      case "completed": return "✅";
      case "current": return "🔄";
      case "blocked": return "⚠️";
      case "pending": return "⏸️";
      default: return "⚪";
    }
  }

  function toggleExpanded(phaseId: string) {
    expandedPhase = expandedPhase === phaseId ? null : phaseId;
  }

  // --- Inline editing (EDIT-06) ---

  function startEdit(phaseId: string, field: "name" | "goal", currentValue: string) {
    editingField = { phaseId, field };
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
      await updatePhase(editingField.phaseId, { [editingField.field]: editValue });
      editingField = null;
      editValue = "";
    } finally {
      saving = false;
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

  async function handleDelete(phaseId: string, phaseName: string) {
    if (!projectId) return;
    if (!confirm(`Delete phase "${phaseName}"? Requirements will be unassigned.`)) return;
    await deletePhase(phaseId);
    if (expandedPhase === phaseId) expandedPhase = null;
  }

  // --- Drag reorder (EDIT-03) ---

  function handleDragStart(e: DragEvent, phaseId: string) {
    if (!projectId) return;
    dragPhaseId = phaseId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", phaseId);
    }
  }

  function handleDragOver(e: DragEvent, phaseId: string) {
    if (!projectId || !dragPhaseId || dragPhaseId === phaseId) return;
    e.preventDefault();
    dragOverPhaseId = phaseId;
  }

  function handleDragLeave() {
    dragOverPhaseId = null;
  }

  async function handleDrop(e: DragEvent, targetPhaseId: string) {
    e.preventDefault();
    if (!projectId || !dragPhaseId || dragPhaseId === targetPhaseId) {
      dragPhaseId = null;
      dragOverPhaseId = null;
      return;
    }

    const target = sortedPhases.find(p => p.id === targetPhaseId);
    if (target) {
      await reorderPhase(dragPhaseId, target.order);
    }

    dragPhaseId = null;
    dragOverPhaseId = null;
  }

  function handleDragEnd() {
    dragPhaseId = null;
    dragOverPhaseId = null;
  }
</script>

<div class="roadmap-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">🗺️</span>
      Roadmap
      <span class="phase-count">({sortedPhases.length} phases)</span>
    </h2>
  </div>

  <div class="roadmap-container">
    {#if sortedPhases.length === 0}
      <div class="empty-roadmap">
        <span class="empty-icon">📍</span>
        <span>No roadmap phases defined yet</span>
      </div>
    {:else}
      <div class="phase-timeline">
        {#each sortedPhases as phase, index (phase.id)}
          {@const status = getPhaseStatus(phase, index)}
          {@const statusColor = getStatusColor(status)}
          
          <div
            class="phase-item"
            class:expanded={expandedPhase === phase.id}
            class:dragging={dragPhaseId === phase.id}
            class:drag-over={dragOverPhaseId === phase.id}
            draggable={!!projectId}
            ondragstart={(e) => handleDragStart(e, phase.id)}
            ondragover={(e) => handleDragOver(e, phase.id)}
            ondragleave={handleDragLeave}
            ondrop={(e) => handleDrop(e, phase.id)}
            ondragend={handleDragEnd}
          >
            {#if index > 0}
              <div class="phase-connector" style="--connector-color: {getStatusColor(getPhaseStatus(sortedPhases[index - 1], index - 1))}"></div>
            {/if}
            
            <div
              class="phase-node"
              style="--status-color: {statusColor}"
              role="button"
              tabindex="0"
              onclick={() => toggleExpanded(phase.id)}
              onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggleExpanded(phase.id); } }}
            >
              <div class="phase-icon">{getStatusIcon(status)}</div>
              <div class="phase-info">
                <div class="phase-header">
                  <!-- EDIT-06: Phase name inline edit -->
                  {#if editingField?.phaseId === phase.id && editingField.field === "name"}
                    <input
                      class="inline-edit-input"
                      bind:value={editValue}
                      onkeydown={handleEditKeydown}
                      onblur={saveEdit}
                      onclick={(e) => e.stopPropagation()}
                    />
                  {:else}
                    <span
                      class="phase-name"
                      class:editable={!!projectId}
                      ondblclick={(e) => { if (projectId) { e.stopPropagation(); startEdit(phase.id, "name", phase.name); } }}
                      title={projectId ? "Double-click to edit" : ""}
                    >{phase.name}</span>
                  {/if}
                  <span class="phase-number">#{phase.order}</span>
                </div>
                <!-- EDIT-06: Phase goal inline edit -->
                {#if editingField?.phaseId === phase.id && editingField.field === "goal"}
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
                    class="phase-goal"
                    class:editable={!!projectId}
                    ondblclick={(e) => { if (projectId) { e.stopPropagation(); startEdit(phase.id, "goal", phase.goal); } }}
                    title={projectId ? "Double-click to edit" : ""}
                  >{phase.goal}</div>
                {/if}
                <div class="phase-meta">
                  {#if phase.requirements.length > 0}
                    <span class="meta-item">
                      <span class="meta-label">Requirements:</span>
                      <span class="meta-value">{phase.requirements.length}</span>
                    </span>
                  {/if}
                  {#if phase.dependencies.length > 0}
                    <span class="meta-item">
                      <span class="meta-label">Dependencies:</span>
                      <span class="meta-value">{phase.dependencies.length}</span>
                    </span>
                  {/if}
                </div>
              </div>
              
              <div class="phase-actions">
                {#if projectId}
                  {#if projectId}
                    <span class="drag-handle" title="Drag to reorder">⋮⋮</span>
                  {/if}
                  <button
                    class="phase-delete-btn"
                    onclick={(e) => { e.stopPropagation(); handleDelete(phase.id, phase.name); }}
                    title="Delete phase"
                  >🗑</button>
                {/if}
                <div class="expand-arrow" class:rotated={expandedPhase === phase.id}>
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                    <path d="M6 12l4-4-4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                </div>
              </div>
            </div>
            
            {#if expandedPhase === phase.id}
              <div class="phase-details">
                {#if phase.requirements.length > 0}
                  <div class="detail-section">
                    <h4>Requirements</h4>
                    <ul class="requirement-list">
                      {#each phase.requirements as reqId (reqId)}
                        <li>{reqId}</li>
                      {/each}
                    </ul>
                  </div>
                {/if}
                
                {#if phase.dependencies.length > 0}
                  <div class="detail-section">
                    <h4>Dependencies</h4>
                    <ul class="dependency-list">
                      {#each phase.dependencies as depId (depId)}
                        <li>{depId}</li>
                      {/each}
                    </ul>
                  </div>
                {/if}

                <!-- EDIT-07: Notes/annotations per phase -->
                {#if projectId}
                  <div class="detail-section">
                    <AnnotationPanel
                      {projectId}
                      targetType="phase"
                      targetId={phase.id}
                      targetLabel={phase.name}
                    />
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .roadmap-section {
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

  .phase-count {
    color: var(--text-muted);
    font-weight: 400;
  }

  .roadmap-container {
    flex: 1;
    overflow-y: auto;
  }

  .empty-roadmap {
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

  .phase-timeline {
    position: relative;
    padding-left: var(--space-4);
  }

  .phase-item {
    position: relative;
    margin-bottom: var(--space-4);
    transition: opacity var(--transition-quick);
  }

  .phase-item.dragging {
    opacity: 0.4;
  }

  .phase-item.drag-over {
    border-top: 2px solid var(--accent);
    padding-top: 2px;
  }

  .phase-connector {
    position: absolute;
    left: -22px;
    top: -16px;
    width: 2px;
    height: 16px;
    background: var(--connector-color, var(--border));
    opacity: 0.6;
  }

  .phase-node {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    cursor: pointer;
    transition: all var(--transition-quick);
    position: relative;
  }

  .phase-node:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
  }

  .phase-item.expanded .phase-node {
    border-color: var(--status-color);
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }

  .phase-icon {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--status-color);
    color: white;
    border-radius: 50%;
    font-size: var(--text-sm);
    flex-shrink: 0;
    margin-left: -34px;
    position: relative;
    z-index: 1;
  }

  .phase-info {
    flex: 1;
    min-width: 0;
  }

  .phase-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .phase-name {
    font-weight: 600;
    color: var(--text);
    flex: 1;
    min-width: 0;
  }

  .phase-number {
    background: var(--surface);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
  }

  .phase-goal {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    line-height: 1.4;
    margin-bottom: var(--space-2);
  }

  .phase-meta {
    display: flex;
    gap: var(--space-3);
    font-size: var(--text-xs);
  }

  .meta-item {
    display: flex;
    gap: var(--space-1);
  }

  .meta-label {
    color: var(--text-muted);
    font-weight: 500;
  }

  .meta-value {
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .phase-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .drag-handle {
    color: var(--text-muted);
    cursor: grab;
    font-size: var(--text-sm);
    padding: 2px;
    opacity: 0;
    transition: opacity var(--transition-quick);
    user-select: none;
  }

  .phase-item:hover .drag-handle {
    opacity: 0.5;
  }

  .drag-handle:hover {
    opacity: 1 !important;
    color: var(--text);
  }

  .phase-delete-btn {
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

  .phase-item:hover .phase-delete-btn {
    opacity: 0.6;
  }

  .phase-delete-btn:hover {
    opacity: 1 !important;
    background: color-mix(in srgb, var(--status-error) 10%, transparent);
    border-color: var(--status-error);
    color: var(--status-error);
  }

  .expand-arrow {
    color: var(--text-muted);
    transition: transform var(--transition-quick), color var(--transition-quick);
    flex-shrink: 0;
  }

  .expand-arrow.rotated {
    transform: rotate(90deg);
    color: var(--accent);
  }

  /* Inline editing (EDIT-06) */
  .inline-edit-input {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: inherit;
    font-weight: 600;
    font-family: inherit;
  }

  .inline-edit-input:focus {
    outline: none;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 25%, transparent);
  }

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

  .phase-details {
    background: var(--bg);
    border: 1px solid var(--status-color);
    border-top: none;
    border-bottom-left-radius: var(--radius);
    border-bottom-right-radius: var(--radius);
    padding: var(--space-3);
    margin-left: -10px;
  }

  .detail-section {
    margin-bottom: var(--space-3);
  }

  .detail-section:last-child {
    margin-bottom: 0;
  }

  .detail-section h4 {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    margin: 0 0 var(--space-2) 0;
  }

  .requirement-list,
  .dependency-list {
    margin: 0;
    padding-left: var(--space-4);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .requirement-list li,
  .dependency-list li {
    margin-bottom: var(--space-1);
    font-family: var(--font-mono);
  }

  @media (max-width: 768px) {
    .phase-timeline {
      padding-left: var(--space-3);
    }

    .phase-icon {
      margin-left: -28px;
      width: 20px;
      height: 20px;
      font-size: var(--text-xs);
    }

    .phase-node {
      padding: var(--space-2);
      gap: var(--space-2);
    }

    .phase-header {
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-1);
    }

    .phase-meta {
      flex-direction: column;
      gap: var(--space-1);
    }

    .phase-details {
      margin-left: -4px;
    }
  }
</style>
