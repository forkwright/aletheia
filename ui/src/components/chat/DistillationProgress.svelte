<script lang="ts">
  import { onGlobalEvent } from "../../lib/events";
  import { onMount, onDestroy } from "svelte";
  import Spinner from "../shared/Spinner.svelte";

  const STAGE_LABELS: Record<string, string> = {
    sanitize: "Sanitizing messages",
    extract: "Extracting facts",
    summarize: "Summarizing context",
    flush: "Flushing to memory",
    verify: "Verifying integrity",
  };

  let active = $state(false);
  let stage = $state("");
  let progress = $state(0);
  let total = $state(6);
  let nousId = $state("");
  let cleanup: (() => void) | undefined;
  let dismissTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    cleanup = onGlobalEvent((event, data) => {
      const d = data as Record<string, unknown>;
      if (event === "distill:before") {
        active = true;
        stage = "preparing";
        progress = 0;
        nousId = (d.nousId as string) ?? "";
        if (dismissTimer) clearTimeout(dismissTimer);
      } else if (event === "distill:stage") {
        active = true;
        stage = (d.stage as string) ?? "";
        progress = (d.progress as number) ?? 0;
        total = (d.total as number) ?? 6;
      } else if (event === "distill:after") {
        stage = "complete";
        progress = total;
        dismissTimer = setTimeout(() => { active = false; }, 3000);
      }
    });
  });

  onDestroy(() => {
    cleanup?.();
    if (dismissTimer) clearTimeout(dismissTimer);
  });

  let pct = $derived(total > 0 ? Math.round((progress / total) * 100) : 0);
  let label = $derived(STAGE_LABELS[stage] ?? stage);
</script>

{#if active}
  <div class="distill-bar" class:complete={stage === "complete"}>
    <div class="distill-icon">
      {#if stage === "complete"}
        <span class="done-icon">✓</span>
      {:else}
        <Spinner size={12} />
      {/if}
    </div>
    <div class="distill-info">
      <span class="distill-label">
        {stage === "complete" ? "Context compressed" : label}
        {#if nousId}
          <span class="distill-agent">({nousId})</span>
        {/if}
      </span>
      <div class="distill-track">
        <div class="distill-fill" style="width: {pct}%"></div>
      </div>
    </div>
  </div>
{/if}

<style>
  .distill-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    margin: 4px 0;
    background: rgba(154, 123, 79, 0.06);
    border: 1px solid rgba(154, 123, 79, 0.2);
    border-radius: 8px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
    animation: fade-in 0.2s ease;
  }
  .distill-bar.complete {
    border-color: rgba(63, 185, 80, 0.3);
    background: rgba(63, 185, 80, 0.06);
  }
  @keyframes fade-in {
    from { opacity: 0; transform: translateY(-4px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .distill-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }
  .done-icon {
    color: var(--status-success);
    font-size: var(--text-sm);
    font-weight: 700;
  }
  .distill-info {
    flex: 1;
    min-width: 0;
  }
  .distill-label {
    display: block;
    margin-bottom: 3px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .distill-agent {
    color: var(--text-muted);
    font-size: var(--text-2xs);
  }
  .distill-track {
    height: 3px;
    background: var(--surface);
    border-radius: 2px;
    overflow: hidden;
  }
  .distill-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.4s ease;
  }
  .distill-bar.complete .distill-fill {
    background: var(--status-success);
  }
</style>
