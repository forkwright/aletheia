<script lang="ts">
  import type { Session } from "../../lib/types";
  import { formatTimeSince } from "../../lib/format";

  let { session, isActive = false, onclick }: {
    session: Session;
    isActive?: boolean;
    onclick: () => void;
  } = $props();
</script>

<button class="session-item" class:active={isActive} {onclick}>
  <span class="key">{session.sessionKey}</span>
  <span class="meta">
    {session.messageCount} msgs
    <span class="dot">·</span>
    {formatTimeSince(session.lastActivity ?? session.updatedAt)}
  </span>
</button>

<style>
  .session-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    padding: 6px 12px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    text-align: left;
    transition: background 0.15s;
  }
  .session-item:hover {
    background: var(--surface-hover);
  }
  .session-item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .session-item.active {
    background: var(--surface);
    border-color: var(--border);
  }
  .key {
    font-family: var(--font-mono);
    color: var(--accent);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .meta {
    color: var(--text-muted);
    font-size: 11px;
  }
  .dot {
    margin: 0 3px;
  }
</style>
