<script lang="ts">
  import { getToasts, dismissToast } from "../../stores/toast.svelte";
  import { setActiveAgent } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";

  function handleClick(agentId?: string, toastId?: string) {
    if (agentId) {
      setActiveAgent(agentId);
      loadSessions(agentId);
    }
    if (toastId) dismissToast(toastId);
  }
</script>

{#if getToasts().length > 0}
  <div class="toast-container">
    {#each getToasts() as toast (toast.id)}
      <div
        class="toast"
        role="button"
        tabindex="0"
        onclick={() => handleClick(toast.agentId, toast.id)}
        onkeydown={(e: KeyboardEvent) => { if (e.key === "Enter") handleClick(toast.agentId, toast.id); }}
      >
        <span class="toast-header">
          {#if toast.emoji}<span class="toast-emoji">{toast.emoji}</span>{/if}
          <span class="toast-agent">{toast.agentName}</span>
          <span
            class="toast-dismiss"
            role="button"
            tabindex="0"
            onclick={(e: MouseEvent) => { e.stopPropagation(); dismissToast(toast.id); }}
            onkeydown={(e: KeyboardEvent) => { if (e.key === "Enter") { e.stopPropagation(); dismissToast(toast.id); } }}
          >×</span>
        </span>
        <span class="toast-preview">{toast.preview}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    bottom: 16px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1000;
    max-width: 340px;
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
    font-size: 13px;
    text-align: left;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    animation: slide-in 0.2s ease-out;
    cursor: pointer;
    width: 100%;
  }
  .toast:hover {
    border-color: var(--accent);
  }
  .toast-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .toast-emoji {
    font-size: 14px;
  }
  .toast-agent {
    font-weight: 600;
    flex: 1;
  }
  .toast-dismiss {
    color: var(--text-muted);
    font-size: 16px;
    padding: 0 2px;
    line-height: 1;
    cursor: pointer;
  }
  .toast-dismiss:hover {
    color: var(--text);
  }
  .toast-preview {
    color: var(--text-secondary);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @keyframes slide-in {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
</style>
