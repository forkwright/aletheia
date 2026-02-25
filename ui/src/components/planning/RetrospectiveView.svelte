<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";
  import { authFetch } from "./api";

  interface PhaseRetro {
    name: string;
    goal: string;
    status: string;
    discussionCount: number;
    verificationStatus: string | null;
    gapCount: number;
    duration: string | null;
  }

  interface Pattern {
    type: "success" | "failure" | "antipattern" | "lesson";
    summary: string;
    context: string;
  }

  interface Retrospective {
    goal: string;
    outcome: "complete" | "abandoned" | "partial";
    phases: PhaseRetro[];
    patterns: Pattern[];
    generatedAt: string;
  }

  let { projectId }: { projectId: string } = $props();

  let retro = $state<Retrospective | null>(null);
  let reason = $state<string | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function loadRetro() {
    if (!projectId) return;
    try {
      loading = true;
      error = null;
      const res = await authFetch(`/api/planning/projects/${projectId}/retrospective`);
      if (!res.ok) {
        error = `Failed to load retrospective (${res.status})`;
        return;
      }
      const data = await res.json() as { retrospective: Retrospective | null; reason?: string };
      retro = data.retrospective;
      reason = data.reason ?? null;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => { loadRetro(); });

  function phaseIcon(status: string): string {
    switch (status) {
      case "complete": return "✅";
      case "failed": return "❌";
      case "skipped": return "⏭";
      default: return "⬜";
    }
  }

  function patternIcon(type: string): string {
    switch (type) {
      case "success": return "✅";
      case "failure": return "❌";
      case "antipattern": return "⚠️";
      case "lesson": return "💡";
      default: return "📝";
    }
  }

  function patternColor(type: string): string {
    switch (type) {
      case "success": return "var(--status-success)";
      case "failure": return "var(--status-error)";
      case "antipattern": return "var(--status-warning)";
      case "lesson": return "var(--accent)";
      default: return "var(--text-muted)";
    }
  }

  function outcomeLabel(outcome: string): string {
    switch (outcome) {
      case "complete": return "Completed Successfully";
      case "abandoned": return "Abandoned";
      case "partial": return "Partially Complete";
      default: return outcome;
    }
  }

  function outcomeColor(outcome: string): string {
    switch (outcome) {
      case "complete": return "var(--status-success)";
      case "abandoned": return "var(--status-error)";
      default: return "var(--status-warning)";
    }
  }
</script>

<div class="retro-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">📊</span>
      Retrospective
    </h2>
  </div>

  <div class="retro-content">
    {#if loading}
      <div class="center-state"><Spinner size={20} /><span>Loading retrospective...</span></div>
    {:else if error}
      <div class="center-state error">⚠️ {error}</div>
    {:else if !retro}
      <div class="center-state">
        <span class="empty-icon">📊</span>
        <span>{reason ?? "Retrospective not available yet"}</span>
      </div>
    {:else}
      <!-- Outcome Banner -->
      <div class="outcome-banner" style="--outcome-color: {outcomeColor(retro.outcome)}">
        <div class="outcome-info">
          <span class="outcome-label">{outcomeLabel(retro.outcome)}</span>
          <span class="outcome-goal">{retro.goal}</span>
        </div>
        <span class="outcome-time">{new Date(retro.generatedAt).toLocaleString()}</span>
      </div>

      <!-- Phase Summary -->
      <div class="phases-summary">
        <h3 class="sub-title">Phase Outcomes ({retro.phases.length})</h3>
        <div class="phases-grid">
          {#each retro.phases as phase}
            <div class="phase-card">
              <div class="phase-header">
                <span class="phase-icon">{phaseIcon(phase.status)}</span>
                <span class="phase-name">{phase.name}</span>
              </div>
              <div class="phase-stats">
                <span class="stat">{phase.status}</span>
                {#if phase.duration}
                  <span class="stat">⏱ {phase.duration}</span>
                {/if}
                {#if phase.discussionCount > 0}
                  <span class="stat">💬 {phase.discussionCount}</span>
                {/if}
                {#if phase.gapCount > 0}
                  <span class="stat gap">🔍 {phase.gapCount} gaps</span>
                {/if}
                {#if phase.verificationStatus}
                  <span class="stat">{phase.verificationStatus}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>

      <!-- Patterns -->
      {#if retro.patterns.length > 0}
        <div class="patterns-section">
          <h3 class="sub-title">Patterns & Lessons ({retro.patterns.length})</h3>
          <div class="patterns-list">
            {#each retro.patterns as pattern}
              <div class="pattern-card" style="--pattern-color: {patternColor(pattern.type)}">
                <div class="pattern-header">
                  <span class="pattern-icon">{patternIcon(pattern.type)}</span>
                  <span class="pattern-type">{pattern.type}</span>
                </div>
                <p class="pattern-summary">{pattern.summary}</p>
                <p class="pattern-context">{pattern.context}</p>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .retro-section { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .section-header { margin-bottom: var(--space-3); }
  .section-title { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-lg); font-weight: 600; color: var(--text); margin: 0; }
  .title-icon { font-size: var(--text-xl); }
  .retro-content { flex: 1; overflow-y: auto; }
  .center-state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-2); padding: var(--space-6); color: var(--text-muted); font-size: var(--text-sm); }
  .center-state.error { color: var(--status-error); }
  .empty-icon { font-size: var(--text-2xl); opacity: 0.5; }

  .outcome-banner { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-3); padding: var(--space-3); background: color-mix(in srgb, var(--outcome-color) 8%, var(--surface)); border: 1px solid color-mix(in srgb, var(--outcome-color) 25%, var(--border)); border-radius: var(--radius); margin-bottom: var(--space-4); }
  .outcome-info { display: flex; flex-direction: column; gap: var(--space-1); }
  .outcome-label { font-weight: 600; color: var(--outcome-color); font-size: var(--text-base); }
  .outcome-goal { color: var(--text-secondary); font-size: var(--text-sm); line-height: 1.4; }
  .outcome-time { font-size: var(--text-xs); color: var(--text-muted); white-space: nowrap; flex-shrink: 0; }

  .sub-title { font-size: var(--text-base); font-weight: 600; color: var(--text); margin: 0 0 var(--space-3) 0; }
  .phases-summary { margin-bottom: var(--space-4); }
  .phases-grid { display: flex; flex-direction: column; gap: var(--space-2); }
  .phase-card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: var(--space-3); }
  .phase-header { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-1); }
  .phase-icon { font-size: var(--text-base); }
  .phase-name { font-weight: 600; color: var(--text); font-size: var(--text-sm); }
  .phase-stats { display: flex; gap: var(--space-3); flex-wrap: wrap; }
  .stat { font-size: var(--text-xs); color: var(--text-muted); }
  .stat.gap { color: var(--status-warning); }

  .patterns-section { margin-bottom: var(--space-4); }
  .patterns-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .pattern-card { border: 1px solid var(--border); border-left: 3px solid var(--pattern-color); border-radius: var(--radius-sm); padding: var(--space-3); }
  .pattern-header { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); }
  .pattern-icon { font-size: var(--text-base); }
  .pattern-type { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--pattern-color); }
  .pattern-summary { font-weight: 600; color: var(--text); font-size: var(--text-sm); margin: 0 0 var(--space-1) 0; }
  .pattern-context { color: var(--text-secondary); font-size: var(--text-sm); line-height: 1.4; margin: 0; }

  @media (max-width: 768px) {
    .outcome-banner { flex-direction: column; }
    .phase-stats { flex-direction: column; gap: var(--space-1); }
  }
</style>
