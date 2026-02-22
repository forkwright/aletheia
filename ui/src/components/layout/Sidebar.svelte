<script lang="ts">
  import AgentCard from "../agents/AgentCard.svelte";
  import {
    getAgents,
    getActiveAgentId,
    loadAgents,
    setActiveAgent,
  } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";
  import { getUnreadCount, markRead } from "../../stores/notifications.svelte";
  import { createAgent } from "../../lib/api";

  let { collapsed = false, onAgentSelect }: {
    collapsed?: boolean;
    onAgentSelect?: () => void;
  } = $props();

  let showForm = $state(false);
  let formName = $state("");
  let formId = $state("");
  let formEmoji = $state("");
  let formError = $state("");
  let creating = $state(false);

  function deriveId(name: string): string {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 30);
  }

  function handleNameInput() {
    formId = deriveId(formName);
  }

  async function handleCreate() {
    if (!formName.trim() || !formId.trim()) return;
    creating = true;
    formError = "";
    try {
      await createAgent(formId, formName.trim(), formEmoji.trim() || undefined);
      await loadAgents();
      setActiveAgent(formId);
      loadSessions(formId);
      showForm = false;
      formName = "";
      formId = "";
      formEmoji = "";
      onAgentSelect?.();
    } catch (err) {
      formError = err instanceof Error ? err.message : String(err);
    } finally {
      creating = false;
    }
  }

  function handleAgentClick(id: string) {
    setActiveAgent(id);
    loadSessions(id);
    markRead(id);
    onAgentSelect?.();
  }

  $effect(() => {
    const agentId = getActiveAgentId();
    if (agentId) {
      loadSessions(agentId);
    }
  });
</script>

<aside class="sidebar" class:collapsed>
  <div class="agent-list">
    {#each getAgents() as agent}
      <AgentCard
        {agent}
        isActive={agent.id === getActiveAgentId()}
        unreadCount={getUnreadCount(agent.id)}
        onclick={() => handleAgentClick(agent.id)}
      />
    {/each}
  </div>

  {#if showForm}
    <div class="create-form">
      <input
        type="text"
        placeholder="Agent name"
        bind:value={formName}
        oninput={handleNameInput}
        disabled={creating}
      />
      <input
        type="text"
        placeholder="ID (auto)"
        bind:value={formId}
        disabled={creating}
      />
      <input
        type="text"
        placeholder="Emoji (optional)"
        bind:value={formEmoji}
        disabled={creating}
      />
      {#if formError}
        <div class="form-error">{formError}</div>
      {/if}
      <div class="form-actions">
        <button class="btn-create" onclick={handleCreate} disabled={creating || !formName.trim()}>
          {creating ? "Creating..." : "Create"}
        </button>
        <button class="btn-cancel" onclick={() => { showForm = false; formError = ""; }}>
          Cancel
        </button>
      </div>
    </div>
  {:else}
    <button class="add-agent" onclick={() => { showForm = true; }} title="Create new agent">
      +
    </button>
  {/if}
</aside>

<style>
  .sidebar {
    width: var(--sidebar-width);
    height: 100%;
    background: var(--bg-elevated);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
    flex-shrink: 0;
    transition: width var(--transition-quick), opacity var(--transition-quick);
  }
  .sidebar.collapsed {
    width: 0;
    opacity: 0;
    border-right: none;
    pointer-events: none;
  }
  .agent-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
  }
  .add-agent {
    margin: 4px 8px 8px;
    padding: 6px;
    border: 1px dashed var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 18px;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .add-agent:hover {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--bg-secondary);
  }
  .create-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    margin: 4px 8px 8px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--border);
  }
  .create-form input {
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 13px;
  }
  .create-form input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .form-error {
    color: var(--error);
    font-size: 12px;
    padding: 0 2px;
  }
  .form-actions {
    display: flex;
    gap: 6px;
  }
  .btn-create {
    flex: 1;
    padding: 6px;
    border: none;
    border-radius: 4px;
    background: var(--accent);
    color: var(--bg-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-create:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-cancel {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
  }

  @media (max-width: 768px) {
    .sidebar {
      position: fixed;
      top: calc(var(--topbar-height) + var(--safe-top));
      left: 0;
      bottom: 0;
      z-index: 100;
      width: var(--sidebar-width);
      box-shadow: 4px 0 16px rgba(0, 0, 0, 0.3);
      transition: transform var(--transition-measured), opacity var(--transition-measured);
      padding-bottom: var(--safe-bottom);
    }
    .sidebar.collapsed {
      width: var(--sidebar-width);
      transform: translateX(-100%);
      opacity: 0;
      border-right: 1px solid var(--border);
      pointer-events: none;
    }
  }
</style>
