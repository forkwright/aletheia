<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";

  interface VerificationGap {
    criterion?: string;
    status: "met" | "partially-met" | "not-met";
    detail?: string;
    proposedFix?: string;
  }

  interface VerificationData {
    status: string;
    summary: string;
    gaps: VerificationGap[];
    verifiedAt?: string;
    overridden?: boolean;
    overrideNote?: string;
  }

  let { projectId, phaseId, phaseName }: {
    projectId: string;
    phaseId: string;
    phaseName?: string;
  } = $props();

  let verification = $state<VerificationData | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function loadVerification() {
    if (!projectId || !phaseId) return;
    try {
      loading = true;
      error = null;
      const res = await fetch(`/api/planning/projects/${projectId}/phases/${phaseId}/verification`);
      if (!res.ok) {
        error = `Failed to load verification (${res.status})`;
        return;
      }
      const data = await res.json() as { verification: VerificationData | null };
      verification = data.verification;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    loadVerification();
  });

  function statusIcon(status: string): string {
    switch (status) {
      case "met": return "✅";
      case "partially-met": return "⚠️";
      case "not-met": return "❌";
      default: return "❓";
    }
  }

  function statusColor(status: string): string {
    switch (status) {
      case "met": return "var(--status-success)";
      case "partially-met": return "var(--status-warning)";
      case "not-met": return "var(--status-error)";
      default: return "var(--text-muted)";
    }
  }

  let gapsByStatus = $derived.by(() => {
    if (!verification?.gaps) return { critical: [], partial: [], met: [] };
    return {
      critical: verification.gaps.filter(g => g.status === "not-met"),
      partial: verification.gaps.filter(g => g.status === "partially-met"),
      met: verification.gaps.filter(g => g.status === "met"),
    };
  });
</script>

<div class="verification-panel">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">🔍</span>
      Verification
      {#if phaseName}
        <span class="phase-name">— {phaseName}</span>
      {/if}
    </h2>
  </div>

  <div class="verification-content">
    {#if loading}
      <div class="loading-state">
        <Spinner size={20} />
        <span>Loading verification results...</span>
      </div>
    {:else if error}
      <div class="error-state">
        <span>⚠️ {error}</span>
        <button class="retry-btn" onclick={loadVerification}>Retry</button>
      </div>
    {:else if !verification}
      <div class="empty-state">
        <span class="empty-icon">🔍</span>
        <span>No verification results yet</span>
        <small>Verification runs after phase execution completes</small>
      </div>
    {:else}
      <div class="overall-status" style="--status-color: {statusColor(verification.status)}">
        <div class="status-icon">{statusIcon(verification.status)}</div>
        <div class="status-info">
          <span class="status-label">{verification.status.replace("-", " ")}</span>
          <span class="status-summary">{verification.summary}</span>
        </div>
        {#if verification.verifiedAt}
          <span class="verified-time">
            {new Date(verification.verifiedAt).toLocaleString()}
          </span>
        {/if}
      </div>

      {#if verification.overridden}
        <div class="override-banner">
          <span>🔓 Overridden by user</span>
          {#if verification.overrideNote}
            <span class="override-note">: {verification.overrideNote}</span>
          {/if}
        </div>
      {/if}

      {#if verification.gaps.length > 0}
        <div class="gaps-section">
          <h3 class="gaps-title">
            Gaps ({verification.gaps.length})
            {#if gapsByStatus.critical.length > 0}
              <span class="gap-count critical">{gapsByStatus.critical.length} critical</span>
            {/if}
            {#if gapsByStatus.partial.length > 0}
              <span class="gap-count partial">{gapsByStatus.partial.length} partial</span>
            {/if}
          </h3>
          <div class="gaps-list">
            {#each verification.gaps as gap, i}
              <div class="gap-item" style="--gap-color: {statusColor(gap.status)}">
                <div class="gap-header">
                  <span class="gap-icon">{statusIcon(gap.status)}</span>
                  <span class="gap-criterion">{gap.criterion ?? `Gap ${i + 1}`}</span>
                  <span class="gap-badge">{gap.status.replace("-", " ")}</span>
                </div>
                {#if gap.detail}
                  <div class="gap-detail">{gap.detail}</div>
                {/if}
                {#if gap.proposedFix}
                  <div class="gap-fix">
                    <strong>Fix:</strong> {gap.proposedFix}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="no-gaps">✅ All criteria verified — no gaps found</div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .verification-panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .section-header { margin-bottom: var(--space-3); }
  .section-title { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-lg); font-weight: 600; color: var(--text); margin: 0; }
  .title-icon { font-size: var(--text-xl); }
  .phase-name { color: var(--text-muted); font-weight: 400; }
  .verification-content { flex: 1; overflow-y: auto; }
  .loading-state, .error-state, .empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-2); padding: var(--space-6); color: var(--text-muted); font-size: var(--text-sm); }
  .empty-icon { font-size: var(--text-2xl); opacity: 0.5; }
  .empty-state small { font-size: var(--text-xs); opacity: 0.8; }
  .retry-btn { background: var(--accent); color: white; border: none; border-radius: var(--radius-sm); padding: var(--space-1) var(--space-3); font-size: var(--text-xs); cursor: pointer; }
  .overall-status { display: flex; align-items: flex-start; gap: var(--space-3); padding: var(--space-3); background: color-mix(in srgb, var(--status-color) 8%, var(--surface)); border: 1px solid color-mix(in srgb, var(--status-color) 25%, var(--border)); border-radius: var(--radius); margin-bottom: var(--space-3); }
  .status-icon { font-size: var(--text-2xl); flex-shrink: 0; line-height: 1; }
  .status-info { flex: 1; display: flex; flex-direction: column; gap: var(--space-1); }
  .status-label { font-weight: 600; color: var(--text); text-transform: capitalize; font-size: var(--text-base); }
  .status-summary { color: var(--text-secondary); font-size: var(--text-sm); line-height: 1.4; }
  .verified-time { font-size: var(--text-xs); color: var(--text-muted); white-space: nowrap; flex-shrink: 0; }
  .override-banner { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: color-mix(in srgb, var(--status-warning) 10%, transparent); border: 1px solid color-mix(in srgb, var(--status-warning) 25%, transparent); border-radius: var(--radius-sm); margin-bottom: var(--space-3); font-size: var(--text-sm); color: var(--text-secondary); }
  .override-note { font-style: italic; }
  .gaps-section { margin-top: var(--space-2); }
  .gaps-title { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-base); font-weight: 600; color: var(--text); margin: 0 0 var(--space-3) 0; }
  .gap-count { font-size: var(--text-xs); font-weight: 600; padding: var(--space-1) var(--space-2); border-radius: var(--radius-pill); }
  .gap-count.critical { background: color-mix(in srgb, var(--status-error) 15%, transparent); color: var(--status-error); }
  .gap-count.partial { background: color-mix(in srgb, var(--status-warning) 15%, transparent); color: var(--status-warning); }
  .gaps-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .gap-item { border: 1px solid var(--border); border-left: 3px solid var(--gap-color, var(--border)); border-radius: var(--radius-sm); padding: var(--space-3); }
  .gap-header { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); }
  .gap-criterion { font-weight: 600; color: var(--text); flex: 1; font-size: var(--text-sm); }
  .gap-badge { font-size: var(--text-2xs); font-weight: 600; padding: 2px var(--space-1); border-radius: var(--radius-pill); text-transform: uppercase; letter-spacing: 0.05em; background: color-mix(in srgb, var(--gap-color) 15%, transparent); color: var(--gap-color); }
  .gap-detail { color: var(--text-secondary); font-size: var(--text-sm); line-height: 1.4; margin-bottom: var(--space-2); }
  .gap-fix { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: var(--space-2); font-size: var(--text-sm); color: var(--text-secondary); }
  .gap-fix strong { color: var(--text); }
  .no-gaps { padding: var(--space-4); text-align: center; color: var(--status-success); font-size: var(--text-sm); font-weight: 500; }
</style>
