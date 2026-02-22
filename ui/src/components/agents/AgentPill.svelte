<script lang="ts">
  import type { Agent } from "../../lib/types";

  let { agent, isActive = false, unreadCount = 0, activeTurns = 0, statusLabel = "", onclick }: {
    agent: Agent;
    isActive?: boolean;
    unreadCount?: number;
    activeTurns?: number;
    statusLabel?: string;
    onclick: () => void;
  } = $props();

  let statusText = $derived(
    unreadCount > 0
      ? `${unreadCount > 9 ? "9+" : unreadCount} new`
      : activeTurns > 0
        ? (statusLabel || "Working")
        : "Idle",
  );

  let statusClass = $derived(
    activeTurns > 0 ? "working" : unreadCount > 0 ? "unread" : "idle",
  );
</script>

<button class="agent-pill" class:active={isActive} {onclick} title={agent.name}>
  <span class="pill-dot {statusClass}"></span>
  {#if agent.emoji}
    <span class="pill-emoji">{agent.emoji}</span>
  {/if}
  <span class="pill-name">{agent.name}</span>
  <span class="pill-status {statusClass}">{statusText}</span>
</button>

<style>
  .agent-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    color: var(--text-secondary);
    font-size: var(--text-sm);
    white-space: nowrap;
    transition: all var(--transition-quick);
    flex-shrink: 0;
  }
  .agent-pill:hover {
    background: var(--surface);
    color: var(--text);
  }
  .agent-pill.active {
    background: var(--surface);
    border-color: var(--accent-border);
    color: var(--text);
  }
  .pill-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .pill-dot.idle {
    background: var(--text-muted);
  }
  .pill-dot.working {
    background: var(--status-active);
    animation: pulse-dot 1.5s ease infinite;
  }
  .pill-dot.unread {
    background: var(--accent);
  }
  @keyframes pulse-dot {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .pill-emoji {
    font-size: var(--text-base);
    line-height: 1;
  }
  .pill-name {
    font-weight: 500;
  }
  .pill-status {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }
  .pill-status.working {
    color: var(--status-active);
  }
  .pill-status.unread {
    color: var(--accent);
    font-weight: 600;
  }

  @media (max-width: 768px) {
    .agent-pill {
      padding: 6px 10px;
      min-height: 36px;
    }
    .pill-status {
      display: none;
    }
  }
</style>
