<script lang="ts">
  import { getToasts, dismissToast } from "../../stores/toast.svelte";
  import { setActiveAgent } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";

  function handleNavigate(agentId: string, toastId: string) {
    setActiveAgent(agentId);
    loadSessions(agentId);
    dismissToast(toastId);
  }
</script>

{#if getToasts().length > 0}
  <div class="toast-container">
    {#each getToasts() as toast (toast.id)}
      <div class="toast" role="status" aria-live="polite">
        <span class="toast-header">
          {#if toast.emoji}<span class="toast-emoji">{toast.emoji}</span>{/if}
          <span class="toast-agent">{toast.agentName}</span>
          <button
            class="toast-dismiss"
            onclick={(e: MouseEvent) => { e.stopPropagation(); dismissToast(toast.id); }}
            aria-label="Dismiss notification"
          >×</button>
        </span>
        <span class="toast-preview">{toast.preview}</span>
        {#if toast.action}
          <button
            class="toast-action"
            onclick={() => toast.action!.callback()}
          >{toast.action.label}</button>
        {:else if toast.agentId}
          <button
            class="toast-action"
            onclick={() => handleNavigate(toast.agentId!, toast.id)}
          >View</button>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    bottom: 80px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1000;
    max-width: 340px;
    pointer-events: none;
  }
  .toast {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 14px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    box-shadow: var(--shadow-md);
    animation: slide-in 0.2s ease-out;
    width: 100%;
    pointer-events: auto;
  }
  .toast-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .toast-emoji {
    font-size: var(--text-base);
  }
  .toast-agent {
    font-weight: 600;
    flex: 1;
  }
  .toast-dismiss {
    color: var(--text-muted);
    font-size: var(--text-lg);
    padding: 0 4px;
    line-height: 1;
    cursor: pointer;
    background: none;
    border: none;
  }
  .toast-dismiss:hover {
    color: var(--text);
  }
  .toast-preview {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toast-action {
    align-self: flex-end;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--accent);
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 3px 10px;
    cursor: pointer;
    margin-top: 2px;
    transition: background var(--transition-quick), border-color var(--transition-quick);
  }
  .toast-action:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
  }
  @keyframes slide-in {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }

  @media (max-width: 768px) {
    .toast-container {
      bottom: 100px;
      right: 8px;
      left: 8px;
      max-width: none;
    }
  }
</style>
