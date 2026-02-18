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

  // Persist collapse state (desktop only — mobile uses the Layout-controlled collapsed prop)
  const COLLAPSE_KEY = "aletheia_sidebar_agents_collapsed";
  let agentsCollapsed = $state(localStorage.getItem(COLLAPSE_KEY) === "true");

  function toggleAgents() {
    agentsCollapsed = !agentsCollapsed;
    localStorage.setItem(COLLAPSE_KEY, String(agentsCollapsed));
  }

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
    <button class="section-header" onclick={toggleAgents}>
      <span class="chevron" class:open={!agentsCollapsed}>›</span>
      <span class="section-title">Agents</span>
      <span class="agent-count">{getAgents().length}</span>
    </button>
    {#if !agentsCollapsed}
      <div class="section-list">
        {#each getAgents() as agent}
          <AgentCard
            {agent}
            isActive={agent.id === getActiveAgentId()}
            onclick={() => handleAgentClick(agent.id)}
          />
        {/each}
      </div>
    {/if}
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
    gap: 6px;
    width: 100%;
    padding: 4px 12px 8px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    text-align: left;
    transition: color 0.15s;
  }
  .section-header:hover {
    color: var(--text-secondary);
  }
  .chevron {
    font-size: 12px;
    transition: transform 0.15s ease;
    flex-shrink: 0;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .section-title {
    flex: 1;
  }
  .agent-count {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--surface);
    padding: 1px 6px;
    border-radius: 8px;
  }
  .section-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    animation: section-open 0.15s ease;
  }
  @keyframes section-open {
    from { opacity: 0; transform: translateY(-4px); }
    to { opacity: 1; transform: translateY(0); }
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
