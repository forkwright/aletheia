<script lang="ts">
  import type { ChatMessage, ToolCallState } from "../../lib/types";
  import { formatTimestamp } from "../../lib/format";
  import Markdown from "./Markdown.svelte";

  let { message, agentName, onToolClick }: {
    message: ChatMessage;
    agentName?: string | null;
    onToolClick?: (tools: ToolCallState[]) => void;
  } = $props();

  let isUser = $derived(message.role === "user");
  let initials = $derived(agentName ? agentName.slice(0, 2).toUpperCase() : "AI");

  function toolSummary(tools: ToolCallState[]): string {
    const running = tools.filter((t) => t.status === "running").length;
    const errors = tools.filter((t) => t.status === "error").length;
    if (running > 0) return `${tools.length} tools running...`;
    if (errors > 0) return `${tools.length} tools (${errors} failed)`;
    return `${tools.length} tool${tools.length === 1 ? "" : "s"} used`;
  }
</script>

<div class="message" class:user={isUser} class:assistant={!isUser}>
  <div class="avatar" class:user-avatar={isUser} class:agent-avatar={!isUser}>
    {#if isUser}
      <span class="avatar-text">You</span>
    {:else}
      <span class="avatar-text">{initials}</span>
    {/if}
  </div>
  <div class="body">
    {#if message.toolCalls && message.toolCalls.length > 0}
      <button
        class="tool-pill"
        class:has-error={message.toolCalls.some((t) => t.status === "error")}
        onclick={() => onToolClick?.(message.toolCalls!)}
      >
        <span class="tool-icon">&#9881;</span>
        {toolSummary(message.toolCalls)}
      </button>
    {/if}
    {#if message.content}
      <div class="content">
        {#if isUser}
          <div class="user-text">{message.content}</div>
        {:else}
          <Markdown content={message.content} />
        {/if}
      </div>
    {/if}
    <div class="timestamp">{formatTimestamp(message.timestamp)}</div>
  </div>
</div>

<style>
  .message {
    display: flex;
    gap: 12px;
    padding: 12px 16px;
    transition: background 0.15s;
  }
  .message:hover {
    background: rgba(255, 255, 255, 0.02);
  }
  .message.assistant {
    background: rgba(255, 255, 255, 0.01);
  }
  .avatar {
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
  }
  .user-avatar {
    background: var(--surface);
  }
  .agent-avatar {
    background: var(--accent);
    border-color: var(--accent);
  }
  .avatar-text {
    font-size: 10px;
    font-weight: 700;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }
  .agent-avatar .avatar-text {
    color: #fff;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .content {
    margin-top: 2px;
  }
  .user-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .tool-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 10px;
    margin-bottom: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }
  .tool-pill:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
    color: var(--text);
  }
  .tool-pill.has-error {
    border-color: var(--red);
    color: var(--red);
  }
  .tool-icon {
    font-size: 12px;
    opacity: 0.7;
  }
  .timestamp {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
