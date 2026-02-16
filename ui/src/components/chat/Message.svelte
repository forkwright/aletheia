<script lang="ts">
  import type { ChatMessage, ToolCallState } from "../../lib/types";
  import { formatTimestamp } from "../../lib/format";
  import Markdown from "./Markdown.svelte";

  let { message, agentName, agentEmoji, onToolClick }: {
    message: ChatMessage;
    agentName?: string | null;
    agentEmoji?: string | null;
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

<div class="chat-msg" class:user={isUser} class:assistant={!isUser}>
  <div class="chat-avatar" class:user={isUser} class:agent={!isUser}>
    {#if isUser}
      <span class="chat-avatar-text">You</span>
    {:else if agentEmoji}
      <span class="chat-avatar-emoji">{agentEmoji}</span>
    {:else}
      <span class="chat-avatar-text">{initials}</span>
    {/if}
  </div>
  <div class="chat-body">
    {#if message.toolCalls && message.toolCalls.length > 0}
      <button
        class="chat-tool-pill"
        class:has-error={message.toolCalls.some((t) => t.status === "error")}
        onclick={() => onToolClick?.(message.toolCalls!)}
      >
        <span class="chat-tool-icon">⚙</span>
        {toolSummary(message.toolCalls)}
      </button>
    {/if}
    {#if message.content}
      <div class="chat-content">
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
  .user-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .timestamp {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
