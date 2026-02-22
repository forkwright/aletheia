<script lang="ts">
  import type { Agent } from "../../lib/types";
  import { formatTimeSince } from "../../lib/format";

  let { agent, isActive = false, lastActivity, onclick, unreadCount = 0 }: {
    agent: Agent;
    isActive?: boolean;
    lastActivity?: string | null;
    onclick: () => void;
    unreadCount?: number;
  } = $props();
</script>

<button class="agent-card" class:active={isActive} {onclick}>
  <span class="avatar">
    {#if agent.emoji}
      <span class="emoji">{agent.emoji}</span>
    {:else}
      <span class="initials">{agent.name.slice(0, 2).toUpperCase()}</span>
    {/if}
  </span>
  <span class="info">
    <span class="name">{agent.name}</span>
    {#if lastActivity}
      <span class="activity">{formatTimeSince(lastActivity)}</span>
    {/if}
  </span>
  {#if unreadCount > 0}
    <span class="unread-badge">{unreadCount > 9 ? "9+" : unreadCount}</span>
  {/if}
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
  .agent-card:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .agent-card.active {
    background: var(--surface);
    border-color: var(--border);
  }
  .avatar {
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
  }
  .emoji {
    font-size: 20px;
    line-height: 1;
  }
  .initials {
    font-size: 11px;
    font-weight: 700;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: #0f1114;
    letter-spacing: 0.5px;
  }
  .info {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .activity {
    font-size: 11px;
    color: var(--text-muted);
  }
  .unread-badge {
    flex-shrink: 0;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--accent);
    color: #0f1114;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }

  @media (max-width: 768px) {
    .agent-card {
      padding: 10px 14px;
      min-height: 44px; /* Apple HIG minimum touch target */
    }
  }
</style>
