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
    onToolClick,
  }: {
    messages: ChatMessage[];
    streamingText: string;
    activeToolCalls: ToolCallState[];
    isStreaming: boolean;
    agentName?: string | null;
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

  // Auto-scroll when new content arrives and user is near bottom
  $effect(() => {
    void messages.length;
    void streamingText;
    void activeToolCalls.length;

    if (isNearBottom) {
      requestAnimationFrame(scrollToBottom);
    }
  });

  function streamingToolSummary(tools: ToolCallState[]): string {
    const running = tools.filter((t) => t.status === "running").length;
    if (running > 0) return `${tools.length} tools (${running} running...)`;
    return `${tools.length} tool${tools.length === 1 ? "" : "s"} used`;
  }
</script>

<div class="message-list" bind:this={container} onscroll={checkScroll}>
  {#if messages.length === 0 && !isStreaming}
    <div class="empty-state">
      <div class="empty-initials">{initials}</div>
      <p>Send a message to start a conversation.</p>
    </div>
  {:else}
    {#each messages as message (message.id)}
      <Message {message} {agentName} {onToolClick} />
    {/each}

    {#if isStreaming}
      <div class="message assistant streaming">
        <div class="avatar agent-avatar">
          <span class="avatar-text">{initials}</span>
        </div>
        <div class="body">
          {#if activeToolCalls.length > 0}
            <button
              class="tool-pill"
              class:has-error={activeToolCalls.some((t) => t.status === "error")}
              onclick={() => onToolClick?.(activeToolCalls)}
            >
              <span class="tool-icon">&#9881;</span>
              {streamingToolSummary(activeToolCalls)}
            </button>
          {/if}
          {#if streamingText}
            <div class="content">
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
  .message {
    display: flex;
    gap: 12px;
    padding: 12px 16px;
  }
  .message.assistant {
    background: rgba(255, 255, 255, 0.01);
  }
  .message.streaming {
    animation: fade-in 0.2s ease;
  }
  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
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
  .agent-avatar {
    background: var(--accent);
    border-color: var(--accent);
  }
  .avatar-text {
    font-size: 10px;
    font-weight: 700;
    color: #fff;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .content {
    margin-top: 2px;
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
