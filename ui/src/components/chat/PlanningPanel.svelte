<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";

  interface PlanEntry {
    phaseId: string;
    name: string;
    status: string;
    waveNumber: number | null;
    startedAt: string | null;
    completedAt: string | null;
    error: string | null;
  }

  interface ExecutionSnapshot {
    projectId: string;
    state: string;
    activeWave: number | null;
    plans: PlanEntry[];
    activePlanIds: string[];
    startedAt: string | null;
    completedAt: string | null;
  }

  let { projectId, onClose }: {
    projectId: string;
    onClose: () => void;
  } = $props();

  let snapshot = $state<ExecutionSnapshot | null>(null);
  let fetchError = $state(false);

  async function fetchSnapshot(): Promise<void> {
    try {
      const res = await fetch(`/api/planning/projects/${projectId}/execution`, {
        headers: { "Content-Type": "application/json" },
      });
      if (!res.ok) {
        fetchError = true;
        return;
      }
      snapshot = (await res.json()) as ExecutionSnapshot;
      fetchError = false;
    } catch {
      fetchError = true;
    }
  }

  $effect(() => {
    if (!projectId) return;
    fetchSnapshot();
    const iv = setInterval(fetchSnapshot, 2500);
    return () => clearInterval(iv);
  });

  function statusLabel(status: string): string {
    switch (status) {
      case "pending": return "Pending";
      case "running": return "Running";
      case "done": return "Done";
      case "failed": return "Failed";
      case "skipped": return "Skipped";
      case "zombie": return "Zombie";
      default: return status.charAt(0).toUpperCase() + status.slice(1);
    }
  }

  function stateLabel(state: string): string {
    if (state === "executing") return "Executing";
    if (state === "verifying") return "Verifying";
    if (state === "complete") return "Complete";
    if (state === "blocked") return "Blocked";
    if (state === "phase-planning") return "Planning phases";
    if (state === "questioning") return "Questioning";
    if (state === "researching") return "Researching";
    return state.charAt(0).toUpperCase() + state.slice(1);
  }
</script>

<div class="planning-panel">
  <div class="panel-header">
    <div class="header-top">
      <span class="panel-title">Planning Execution</span>
      <button class="close-btn" onclick={onClose} aria-label="Close">&times;</button>
    </div>
    {#if snapshot}
      <span class="state-subtitle">{stateLabel(snapshot.state)}</span>
    {/if}
  </div>
  <div class="panel-body">
    {#if fetchError}
      <div class="status-message error">Unable to load status.</div>
    {:else if snapshot === null}
      <div class="status-message loading">
        <Spinner size={14} />
        <span>Loading&hellip;</span>
      </div>
    {:else if snapshot.plans.length === 0}
      <div class="status-message empty">No plans yet.</div>
    {:else}
      {#each snapshot.plans as plan (plan.phaseId + "-" + plan.name)}
        <div class="plan-item">
          <span class="plan-name">{plan.name}</span>
          <span class="plan-badge status-{plan.status}">{statusLabel(plan.status)}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .planning-panel {
    width: 380px;
    max-width: 45vw;
    flex-shrink: 0;
    border-left: 1px solid var(--border);
    background: var(--bg-elevated);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: slide-in 0.15s ease;
  }
  @keyframes slide-in {
    from { transform: translateX(20px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .panel-header {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .header-top {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 2px;
  }
  .panel-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    flex: 1;
  }
  .state-subtitle {
    font-size: var(--text-xs);
    color: var(--text-muted);
    display: block;
    margin-top: 2px;
  }
  .close-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-lg);
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: background var(--transition-quick), color var(--transition-quick);
    flex-shrink: 0;
  }
  .close-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .panel-body {
    flex: 1;
    overflow-y: auto;
    padding: 6px 0;
    min-height: 0;
  }
  .status-message {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 16px 14px;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }
  .status-message.error {
    color: var(--status-error);
  }
  .plan-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 14px;
    border-bottom: 1px solid rgba(48, 54, 61, 0.5);
    font-size: var(--text-sm);
  }
  .plan-item:last-child {
    border-bottom: none;
  }
  .plan-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .plan-badge {
    font-size: var(--text-2xs);
    font-weight: 600;
    padding: 2px 7px;
    border-radius: var(--radius-pill);
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .plan-badge.status-pending {
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .plan-badge.status-running {
    color: var(--accent);
    background: rgba(154, 123, 79, 0.1);
    border: 1px solid rgba(154, 123, 79, 0.3);
  }
  .plan-badge.status-done {
    color: var(--status-success);
    background: var(--status-success-bg);
    border: 1px solid var(--status-success-border);
  }
  .plan-badge.status-failed {
    color: var(--status-error);
    background: var(--status-error-bg);
    border: 1px solid var(--status-error-border);
  }
  .plan-badge.status-skipped {
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border);
    opacity: 0.7;
  }
  .plan-badge.status-zombie {
    color: #e87c3e;
    background: rgba(232, 124, 62, 0.1);
    border: 1px solid rgba(232, 124, 62, 0.3);
  }

  @media (max-width: 768px) {
    .planning-panel {
      width: 100%;
      max-width: 100%;
      position: absolute;
      right: 0;
      top: 0;
      bottom: 0;
      z-index: 50;
      border-left: none;
    }
  }
</style>
