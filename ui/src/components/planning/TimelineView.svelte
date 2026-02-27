<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";
  import { authFetch } from "./api";

  interface Milestone {
    id: string;
    name: string;
    type: "builtin" | "phase";
    status: "pending" | "active" | "complete" | "failed";
    order: number;
    goal?: string;
    requirements?: string[];
    requirementCount?: number;
  }

  interface RequirementsSummary {
    v1: number;
    v2: number;
    outOfScope: number;
  }

  let { projectId }: { projectId: string } = $props();

  let milestones = $state<Milestone[]>([]);
  let reqSummary = $state<RequirementsSummary>({ v1: 0, v2: 0, outOfScope: 0 });
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function loadTimeline() {
    if (!projectId) return;
    try {
      loading = true;
      error = null;
      const res = await authFetch(`/api/planning/projects/${projectId}/timeline`);
      if (!res.ok) {
        error = `Failed to load timeline (${res.status})`;
        return;
      }
      const data = await res.json() as {
        milestones: Milestone[];
        goal: string;
        state: string;
        requirementsSummary: RequirementsSummary;
      };
      milestones = data.milestones ?? [];
      reqSummary = data.requirementsSummary ?? { v1: 0, v2: 0, outOfScope: 0 };
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => { loadTimeline(); });

  function statusIcon(status: string): string {
    switch (status) {
      case "complete": return "✓";
      case "active": return "●";
      case "failed": return "✗";
      default: return "○";
    }
  }

  function statusColor(status: string): string {
    switch (status) {
      case "complete": return "var(--status-success)";
      case "active": return "var(--accent)";
      case "failed": return "var(--status-error)";
      default: return "var(--text-muted)";
    }
  }

  let progress = $derived.by(() => {
    if (milestones.length === 0) return 0;
    const done = milestones.filter(m => m.status === "complete").length;
    return Math.round((done / milestones.length) * 100);
  });
</script>

<div class="timeline-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">📍</span>
      Timeline
    </h2>
    {#if reqSummary.v1 + reqSummary.v2 > 0}
      <div class="req-summary">
        <span class="req-badge v1">V1: {reqSummary.v1}</span>
        <span class="req-badge v2">V2: {reqSummary.v2}</span>
      </div>
    {/if}
  </div>

  <div class="timeline-content">
    {#if loading}
      <div class="center-state"><Spinner size={20} /><span>Loading timeline...</span></div>
    {:else if error}
      <div class="center-state error">⚠️ {error}</div>
    {:else if milestones.length === 0}
      <div class="center-state"><span class="empty-icon">📍</span><span>No milestones defined yet</span></div>
    {:else}
      <!-- Progress bar -->
      <div class="progress-row">
        <div class="progress-bar">
          <div class="progress-fill" style="width: {progress}%"></div>
        </div>
        <span class="progress-label">{progress}%</span>
      </div>

      <!-- Milestone track -->
      <div class="milestone-track">
        {#each milestones as milestone, i (milestone.id)}
          <div class="milestone-item" class:active={milestone.status === "active"}>
            <!-- Connector line -->
            {#if i > 0}
              <div
                class="connector"
                class:complete={milestones[i - 1]?.status === "complete"}
              ></div>
            {/if}

            <!-- Node -->
            <div class="milestone-node" style="--node-color: {statusColor(milestone.status)}">
              <span class="node-icon">{statusIcon(milestone.status)}</span>
            </div>

            <!-- Label -->
            <div class="milestone-info">
              <span class="milestone-name">{milestone.name}</span>
              <span class="milestone-type">{milestone.type === "builtin" ? "System" : "Phase"}</span>
              {#if milestone.goal}
                <span class="milestone-goal">{milestone.goal}</span>
              {/if}
              {#if milestone.requirementCount && milestone.requirementCount > 0}
                <span class="milestone-reqs">{milestone.requirementCount} requirements</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .timeline-section { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .section-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-3); }
  .section-title { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-lg); font-weight: 600; color: var(--text); margin: 0; }
  .title-icon { font-size: var(--text-xl); }
  .req-summary { display: flex; gap: var(--space-2); }
  .req-badge { font-size: var(--text-xs); font-weight: 600; padding: var(--space-1) var(--space-2); border-radius: var(--radius-pill); }
  .req-badge.v1 { background: color-mix(in srgb, var(--status-success) 15%, transparent); color: var(--status-success); }
  .req-badge.v2 { background: color-mix(in srgb, var(--status-warning) 15%, transparent); color: var(--status-warning); }
  .timeline-content { flex: 1; overflow-y: auto; }
  .center-state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-2); padding: var(--space-6); color: var(--text-muted); font-size: var(--text-sm); }
  .center-state.error { color: var(--status-error); }
  .empty-icon { font-size: var(--text-2xl); opacity: 0.5; }

  .progress-row { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-4); }
  .progress-bar { flex: 1; height: 6px; background: var(--surface); border-radius: 3px; overflow: hidden; }
  .progress-fill { height: 100%; background: linear-gradient(90deg, var(--status-success), var(--accent)); transition: width 0.3s ease; border-radius: 3px; }
  .progress-label { font-size: var(--text-xs); font-weight: 600; color: var(--text-muted); font-family: var(--font-mono); min-width: 32px; text-align: right; }

  .milestone-track { position: relative; padding-left: 20px; }
  .milestone-item { position: relative; display: flex; align-items: flex-start; gap: var(--space-3); padding-bottom: var(--space-4); }
  .milestone-item:last-child { padding-bottom: 0; }
  .milestone-item.active .milestone-name { color: var(--accent); }

  .connector { position: absolute; left: 11px; top: -16px; width: 2px; height: 20px; }
  .connector { background: var(--border); }
  .connector.complete { background: var(--status-success); }

  .milestone-node { width: 24px; height: 24px; border-radius: 50%; border: 2px solid var(--node-color); display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: var(--bg); position: relative; z-index: 1; }
  .node-icon { font-size: 10px; font-weight: 700; color: var(--node-color); line-height: 1; }

  .milestone-info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .milestone-name { font-weight: 600; color: var(--text); font-size: var(--text-sm); }
  .milestone-type { font-size: var(--text-xs); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
  .milestone-goal { font-size: var(--text-xs); color: var(--text-secondary); line-height: 1.3; }
  .milestone-reqs { font-size: var(--text-xs); color: var(--text-muted); }

  @media (max-width: 768px) {
    .milestone-track { padding-left: 16px; }
    .section-header { flex-direction: column; align-items: flex-start; gap: var(--space-2); }
  }
</style>
