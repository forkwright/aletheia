<script lang="ts">
  import AgentCard from "../agents/AgentCard.svelte";
  import {
    getAgents,
    getActiveAgentId,
    setActiveAgent,
  } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";

  function handleAgentClick(id: string) {
    setActiveAgent(id);
    loadSessions(id);
  }

  $effect(() => {
    const agentId = getActiveAgentId();
    if (agentId) {
      loadSessions(agentId);
    }
  });
</script>

<aside class="sidebar">
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
</style>
