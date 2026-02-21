<script lang="ts">
  import AgentCard from "../agents/AgentCard.svelte";
  import {
    getAgents,
    getActiveAgentId,
    setActiveAgent,
  } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";
  import { getUnreadCount, markRead } from "../../stores/notifications.svelte";

  let { collapsed = false, onAgentSelect }: {
    collapsed?: boolean;
    onAgentSelect?: () => void;
  } = $props();

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
    transition: width 0.2s ease, opacity 0.2s ease;
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

  @media (max-width: 768px) {
    .sidebar {
      position: fixed;
      top: calc(var(--topbar-height) + var(--safe-top));
      left: 0;
      bottom: 0;
      z-index: 100;
      width: var(--sidebar-width);
      box-shadow: 4px 0 16px rgba(0, 0, 0, 0.3);
      transition: transform 0.2s ease, opacity 0.2s ease;
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
