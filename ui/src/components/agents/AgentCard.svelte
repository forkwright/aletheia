<script lang="ts">
  import type { Agent } from "../../lib/types";

  let { agent, isActive = false, onclick }: {
    agent: Agent;
    isActive?: boolean;
    onclick: () => void;
  } = $props();
</script>

<button class="agent-card" class:active={isActive} {onclick}>
  <span class="initials">{agent.name.slice(0, 2).toUpperCase()}</span>
  <span class="name">{agent.name}</span>
  <span class="indicator" class:online={true}></span>
</button>

<style>
  .agent-card {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 14px;
    text-align: left;
    transition: background 0.15s, border-color 0.15s;
  }
  .agent-card:hover {
    background: var(--surface-hover);
  }
  .agent-card.active {
    background: var(--surface);
    border-color: var(--border);
  }
  .initials {
    font-size: 11px;
    font-weight: 700;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: #fff;
    flex-shrink: 0;
    letter-spacing: 0.5px;
  }
  .name {
    flex: 1;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
    flex-shrink: 0;
  }
  .indicator.online {
    background: var(--green);
  }
</style>
