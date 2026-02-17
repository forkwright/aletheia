<script lang="ts">
  import AgentCard from "../agents/AgentCard.svelte";
  import {
    getAgents,
    getActiveAgentId,
    setActiveAgent,
  } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";

  let { collapsed = false, onAgentSelect }: {
    collapsed?: boolean;
    onAgentSelect?: () => void;
  } = $props();

  function handleAgentClick(id: string) {
    setActiveAgent(id);
    loadSessions(id);
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
  <div class="section">
    <div class="section-header">Agents</div>
    <div class="section-list">
      {#each getAgents() as agent}
        <AgentCard
          {agent}
          isActive={agent.id === getActiveAgentId()}
          onclick={() => handleAgentClick(agent.id)}
        />
      {/each}
    </div>
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
    overflow: hidden;
    flex-shrink: 0;
    transition: transform 0.2s ease, opacity 0.2s ease;
  }
  .section {
    padding: 8px;
  }
  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 12px 8px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  .section-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  @media (max-width: 768px) {
    .sidebar {
      position: fixed;
      top: var(--topbar-height);
      left: 0;
      bottom: 0;
      z-index: 100;
      box-shadow: 4px 0 16px rgba(0, 0, 0, 0.3);
    }
    .sidebar.collapsed {
      transform: translateX(-100%);
      opacity: 0;
      pointer-events: none;
    }
  }
</style>
