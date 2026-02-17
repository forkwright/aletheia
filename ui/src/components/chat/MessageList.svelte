<script lang="ts">
  import type { ChatMessage, ToolCallState } from "../../lib/types";
  import Message from "./Message.svelte";
  import Markdown from "./Markdown.svelte";
  import StreamingIndicator from "./StreamingIndicator.svelte";

  let {
    messages,
    streamingText,
    activeToolCalls,
    isStreaming,
    agentName,
    agentEmoji,
    onToolClick,
  }: {
    messages: ChatMessage[];
    streamingText: string;
    activeToolCalls: ToolCallState[];
    isStreaming: boolean;
    agentName?: string | null;
    agentEmoji?: string | null;
    onToolClick?: (tools: ToolCallState[]) => void;
  } = $props();

  let initials = $derived(agentName ? agentName.slice(0, 2).toUpperCase() : "AI");

  let container = $state<HTMLDivElement | null>(null);
  let isNearBottom = $state(true);

  function checkScroll() {
    if (!container) return;
    const threshold = 100;
    isNearBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight < threshold;
  }

  function scrollToBottom() {
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  }

  $effect(() => {
    void messages.length;
    void streamingText;
    void activeToolCalls.length;

    if (isNearBottom) {
      requestAnimationFrame(scrollToBottom);
    }
  });

  /** Humanize a tool name into a readable activity label */
  function humanizeTool(tc: ToolCallState): string {
    const name = tc.name;
    // Common tool names → human-readable
    switch (name) {
      case "exec": return "Running command";
      case "read": return "Reading file";
      case "write": return "Writing file";
      case "edit": return "Editing file";
      case "grep": return "Searching files";
      case "find": return "Finding files";
      case "ls": return "Listing directory";
      case "web_search": return "Searching web";
      case "web_fetch": return "Fetching page";
      case "mem0_search": return "Searching memory";
      case "blackboard": return "Checking blackboard";
      case "sessions_send": return "Messaging agent";
      case "sessions_ask": return "Asking agent";
      case "sessions_spawn": return "Spawning worker";
      case "message": return "Sending message";
      case "enable_tool": return "Enabling tool";
      default: return name.replace(/_/g, " ");
    }
  }

  function toolStatusIcon(status: string): string {
    switch (status) {
      case "running": return "◌";
      case "complete": return "✓";
      case "error": return "✕";
      default: return "·";
    }
  }

  function formatMs(ms?: number): string {
    if (ms == null) return "";
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  }
</script>

<div class="message-list" bind:this={container} onscroll={checkScroll}>
  {#if messages.length === 0 && !isStreaming}
    <div class="empty-state">
      {#if agentEmoji}
        <div class="empty-emoji">{agentEmoji}</div>
      {:else}
        <div class="empty-initials">{initials}</div>
      {/if}
      <p>Send a message to start a conversation.</p>
    </div>
  {:else}
    {#each messages as message (message.id)}
      <Message {message} {agentName} {agentEmoji} {onToolClick} />
    {/each}

    {#if isStreaming}
      <div class="chat-msg assistant streaming">
        <div class="chat-avatar agent">
          {#if agentEmoji}
            <span class="chat-avatar-emoji">{agentEmoji}</span>
          {:else}
            <span class="chat-avatar-text">{initials}</span>
          {/if}
        </div>
        <div class="chat-body">
          {#if activeToolCalls.length > 0}
            <div class="activity-feed">
              {#each activeToolCalls as tc (tc.id)}
                <div class="activity-item" class:running={tc.status === "running"} class:error={tc.status === "error"}>
                  <span class="activity-status" class:running={tc.status === "running"} class:complete={tc.status === "complete"} class:error={tc.status === "error"}>
                    {toolStatusIcon(tc.status)}
                  </span>
                  <span class="activity-label">{humanizeTool(tc)}</span>
                  {#if tc.durationMs != null}
                    <span class="activity-duration">{formatMs(tc.durationMs)}</span>
                  {/if}
                  {#if tc.status === "running"}
                    <span class="activity-spinner"></span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
          {#if streamingText}
            <div class="chat-content">
              <Markdown content={streamingText} />
            </div>
          {:else if activeToolCalls.length === 0}
            <StreamingIndicator />
          {/if}
        </div>
      </div>
    {/if}
  {/if}

  {#if !isNearBottom && isStreaming}
    <button class="scroll-btn" onclick={scrollToBottom}>
      New messages below
    </button>
  {/if}
</div>

<style>
  .message-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    position: relative;
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 14px;
    gap: 12px;
  }
  .empty-emoji {
    font-size: 48px;
    line-height: 1;
    width: 64px;
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .empty-initials {
    font-size: 28px;
    font-weight: 700;
    width: 64px;
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: var(--accent);
    color: #fff;
    letter-spacing: 1px;
  }

  /* Live activity feed during streaming */
  .activity-feed {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
    font-size: 12px;
    font-family: var(--font-mono);
  }
  .activity-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    color: var(--text-secondary);
    transition: opacity 0.15s;
  }
  .activity-item.running {
    color: var(--text);
  }
  .activity-item.error {
    color: var(--red);
  }
  .activity-status {
    width: 14px;
    text-align: center;
    flex-shrink: 0;
    font-size: 11px;
  }
  .activity-status.running {
    color: var(--accent);
  }
  .activity-status.complete {
    color: var(--green);
  }
  .activity-status.error {
    color: var(--red);
  }
  .activity-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .activity-duration {
    color: var(--text-muted);
    font-size: 11px;
    flex-shrink: 0;
  }
  .activity-spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .scroll-btn {
    position: sticky;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--accent);
    color: #fff;
    border: none;
    padding: 6px 16px;
    border-radius: 16px;
    font-size: 12px;
    font-weight: 500;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
    z-index: 10;
  }
  .scroll-btn:hover {
    background: var(--accent-hover);
  }
</style>
