<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";
  import { authFetch } from "./api";

  interface Checkpoint {
    id: string;
    type: string;
    question: string;
    decision: string | null;
    context: Record<string, unknown>;
    createdAt: string;
  }

  let { projectId }: { projectId: string } = $props();

  let checkpoints = $state<Checkpoint[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let submitting = $state<Record<string, boolean>>({});

  async function loadCheckpoints() {
    if (!projectId) return;
    try {
      loading = true;
      error = null;
      const res = await authFetch(`/api/planning/projects/${projectId}/checkpoints`);
      if (!res.ok) {
        error = `Failed to load checkpoints (${res.status})`;
        return;
      }
      const data = await res.json() as { checkpoints: Checkpoint[] };
      checkpoints = data.checkpoints ?? [];
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => { loadCheckpoints(); });

  // Poll for new checkpoints when project is in active states
  $effect(() => {
    const iv = setInterval(loadCheckpoints, 10000);
    return () => clearInterval(iv);
  });

  let pendingCheckpoints = $derived(checkpoints.filter(cp => !cp.decision));
  let resolvedCheckpoints = $derived(checkpoints.filter(cp => cp.decision));
  let showResolved = $state(false);

  async function resolveCheckpoint(checkpointId: string, action: "approve" | "skip", note?: string) {
    if (submitting[checkpointId]) return;
    submitting[checkpointId] = true;
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/checkpoints/${checkpointId}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, note: note?.trim() || undefined }),
      });
      if (!res.ok) throw new Error("Failed to resolve checkpoint");
      // Optimistic update
      checkpoints = checkpoints.map(cp =>
        cp.id === checkpointId ? { ...cp, decision: action === "approve" ? "approved" : "skipped" } : cp
      );
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      submitting[checkpointId] = false;
    }
  }

  function riskColor(type: string): string {
    if (type.includes("irreversible") || type.includes("deletion")) return "var(--status-error)";
    if (type.includes("high")) return "var(--status-warning)";
    return "var(--text-muted)";
  }
</script>

<div class="checkpoint-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">🚧</span>
      Checkpoints
      {#if pendingCheckpoints.length > 0}
        <span class="pending-badge">{pendingCheckpoints.length} pending</span>
      {/if}
    </h2>
    {#if resolvedCheckpoints.length > 0}
      <button class="toggle-resolved" onclick={() => showResolved = !showResolved}>
        {showResolved ? "Hide" : "Show"} Resolved ({resolvedCheckpoints.length})
      </button>
    {/if}
  </div>

  <div class="checkpoint-content">
    {#if loading}
      <div class="center-state"><Spinner size={20} /><span>Loading checkpoints...</span></div>
    {:else if error}
      <div class="center-state error">⚠️ {error}</div>
    {:else if checkpoints.length === 0}
      <div class="center-state"><span class="empty-icon">✅</span><span>No checkpoints — running freely</span></div>
    {:else}
      {#if pendingCheckpoints.length > 0}
        <div class="checkpoint-group">
          <h3 class="group-title">⏳ Awaiting Decision</h3>
          {#each pendingCheckpoints as cp (cp.id)}
            <div class="checkpoint-card pending" style="--risk-color: {riskColor(cp.type)}">
              <div class="cp-header">
                <span class="cp-type">{cp.type}</span>
                <span class="cp-time">{new Date(cp.createdAt).toLocaleString()}</span>
              </div>
              <p class="cp-question">{cp.question}</p>
              {#if Object.keys(cp.context).length > 0}
                <details class="cp-context">
                  <summary>Context</summary>
                  <pre>{JSON.stringify(cp.context, null, 2)}</pre>
                </details>
              {/if}
              <div class="cp-actions">
                <button
                  class="btn-approve"
                  onclick={() => resolveCheckpoint(cp.id, "approve")}
                  disabled={submitting[cp.id]}
                >
                  {#if submitting[cp.id]}<Spinner size={12} />{:else}✓ Approve{/if}
                </button>
                <button
                  class="btn-skip"
                  onclick={() => resolveCheckpoint(cp.id, "skip")}
                  disabled={submitting[cp.id]}
                >
                  {#if submitting[cp.id]}<Spinner size={12} />{:else}⏭ Skip{/if}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if showResolved && resolvedCheckpoints.length > 0}
        <div class="checkpoint-group">
          <h3 class="group-title">✓ Resolved</h3>
          {#each resolvedCheckpoints as cp (cp.id)}
            <div class="checkpoint-card resolved">
              <div class="cp-header">
                <span class="cp-type">{cp.type}</span>
                <span class="cp-decision">{cp.decision}</span>
              </div>
              <p class="cp-question">{cp.question}</p>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .checkpoint-section { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .section-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-3); }
  .section-title { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-lg); font-weight: 600; color: var(--text); margin: 0; }
  .title-icon { font-size: var(--text-xl); }
  .pending-badge { background: var(--status-warning); color: white; font-size: var(--text-xs); font-weight: 600; padding: var(--space-1) var(--space-2); border-radius: var(--radius-pill); }
  .toggle-resolved { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-sm); color: var(--text-secondary); font-size: var(--text-xs); padding: var(--space-1) var(--space-2); cursor: pointer; }
  .toggle-resolved:hover { background: var(--surface-hover); color: var(--text); }
  .checkpoint-content { flex: 1; overflow-y: auto; }
  .center-state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-2); padding: var(--space-6); color: var(--text-muted); font-size: var(--text-sm); }
  .center-state.error { color: var(--status-error); }
  .empty-icon { font-size: var(--text-2xl); opacity: 0.5; }
  .checkpoint-group { margin-bottom: var(--space-4); }
  .group-title { font-size: var(--text-base); font-weight: 600; color: var(--text); margin: 0 0 var(--space-3) 0; border-bottom: 1px solid var(--border); padding-bottom: var(--space-1); }
  .checkpoint-card { border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-3); margin-bottom: var(--space-2); }
  .checkpoint-card.pending { border-left: 3px solid var(--risk-color, var(--status-warning)); background: color-mix(in srgb, var(--risk-color, var(--status-warning)) 5%, transparent); }
  .checkpoint-card.resolved { opacity: 0.7; }
  .cp-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-2); }
  .cp-type { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-muted); }
  .cp-time { font-size: var(--text-xs); color: var(--text-muted); }
  .cp-decision { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; color: var(--status-success); }
  .cp-question { color: var(--text); font-size: var(--text-sm); line-height: 1.5; margin: 0 0 var(--space-2) 0; }
  .cp-context { margin-bottom: var(--space-2); }
  .cp-context summary { font-size: var(--text-xs); font-weight: 600; color: var(--text-secondary); cursor: pointer; padding: var(--space-1); }
  .cp-context pre { font-size: var(--text-xs); background: var(--surface); padding: var(--space-2); border-radius: var(--radius-sm); overflow-x: auto; color: var(--text-secondary); margin: var(--space-1) 0 0 0; }
  .cp-actions { display: flex; gap: var(--space-2); }
  .btn-approve, .btn-skip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-2) var(--space-3); border: 1px solid transparent; border-radius: var(--radius-sm); font-size: var(--text-sm); font-weight: 600; cursor: pointer; transition: all var(--transition-quick); }
  .btn-approve { background: var(--status-success); color: white; }
  .btn-approve:hover:not(:disabled) { opacity: 0.9; }
  .btn-skip { background: var(--surface); color: var(--text-secondary); border-color: var(--border); }
  .btn-skip:hover:not(:disabled) { background: var(--surface-hover); color: var(--text); }
  .btn-approve:disabled, .btn-skip:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
