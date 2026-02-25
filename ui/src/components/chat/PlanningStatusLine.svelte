<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";

  let { projectId, state, activeWave, onclick }: {
    projectId: string;
    state: string;
    activeWave: number | null;
    onclick: () => void;
  } = $props();

  const ACTIVE_STATES = new Set([
    "executing",
    "verifying",
    "phase-planning",
    "researching",
    "roadmap",
    "requirements",
    "questioning",
  ]);

  let isActive = $derived(ACTIVE_STATES.has(state));

  let statusText = $derived.by(() => {
    if (state === "executing" && activeWave !== null) return `Wave ${activeWave + 1} running`;
    if (state === "verifying") return "Verifying phase";
    if (state === "complete") return "Planning complete";
    if (state === "blocked") return "Blocked \u2014 needs input";
    if (state === "phase-planning") return "Planning phases";
    return state.charAt(0).toUpperCase() + state.slice(1);
  });
</script>

<button
  class="planning-status-line"
  class:active={isActive}
  class:complete={state === "complete"}
  class:blocked={state === "blocked" || state === "abandoned"}
  onclick={onclick}
  title="Click to view planning execution"
>
  <span class="status-indicator">
    {#if isActive}
      <Spinner size={12} />
    {:else if state === "complete"}
      <span class="icon-done">&#x2713;</span>
    {:else if state === "blocked" || state === "abandoned"}
      <span class="icon-blocked">!</span>
    {:else}
      <span class="icon-done">&#x2713;</span>
    {/if}
  </span>
  <span class="status-text">{statusText}</span>
  <span class="chevron">&rsaquo;</span>
</button>

<style>
  .planning-status-line {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    margin-bottom: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--border);
    border-radius: var(--radius-pill);
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background var(--transition-quick), border-color var(--transition-quick), color var(--transition-quick);
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
  }
  .planning-status-line:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .planning-status-line.active {
    border-left-color: var(--status-active);
    border-color: var(--status-active);
    color: var(--text);
  }
  .planning-status-line.complete {
    border-left-color: var(--status-success);
    border-color: var(--status-success-border);
    color: var(--text-secondary);
  }
  .planning-status-line.blocked {
    border-left-color: var(--status-error);
    border-color: var(--status-error-border);
    color: var(--status-error);
  }
  .status-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .icon-done {
    color: var(--status-success);
    font-size: var(--text-xs);
    font-weight: 700;
  }
  .icon-blocked {
    color: var(--status-error);
    font-size: var(--text-xs);
    font-weight: 700;
  }
  .status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .chevron {
    color: var(--text-muted);
    font-size: var(--text-base);
    flex-shrink: 0;
    transition: transform var(--transition-quick);
  }
  .planning-status-line:hover .chevron {
    transform: translateX(1px);
    color: var(--accent);
  }

  @media (max-width: 768px) {
    .planning-status-line {
      font-size: var(--text-xs);
      padding: 4px 8px;
      max-width: calc(100vw - 80px);
    }
  }
</style>
