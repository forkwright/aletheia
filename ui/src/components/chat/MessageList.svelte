<script lang="ts">
  import type { ChatMessage, ToolCallState } from "../../lib/types";
  import Message from "./Message.svelte";
  import Markdown from "./Markdown.svelte";
  import StreamingIndicator from "./StreamingIndicator.svelte";
  import ToolStatusLine from "./ToolStatusLine.svelte";
  import ThinkingStatusLine from "./ThinkingStatusLine.svelte";

  let {
    messages,
    streamingText,
    thinkingText = "",
    activeToolCalls,
    isStreaming,
    agentName,
    agentEmoji,
    onToolClick,
    onThinkingClick,
  }: {
    messages: ChatMessage[];
    streamingText: string;
    thinkingText?: string;
    activeToolCalls: ToolCallState[];
    isStreaming: boolean;
    agentName?: string | null;
    agentEmoji?: string | null;
    onToolClick?: (tools: ToolCallState[]) => void;
    onThinkingClick?: (thinking?: string) => void;
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
    void thinkingText;
    void activeToolCalls.length;

    if (isNearBottom) {
      requestAnimationFrame(scrollToBottom);
    }
  });
</script>

<div class="message-list" bind:this={container} onscroll={checkScroll}>
  {#if messages.length === 0 && !isStreaming}
    <div class="empty-state">
      {#if agentEmoji}
        <div class="empty-emoji">{agentEmoji}</div>
      {:else}
        <div class="empty-initials">{initials}</div>
      {/if}
      <p class="empty-name">{agentName ?? "Agent"}</p>
      <p class="empty-hint">Ready when you are.</p>
    </div>
  {:else}
    {#each messages as message (message.id)}
      <Message {message} {agentName} {agentEmoji} {onToolClick} onThinkingClick={(thinking) => onThinkingClick?.(thinking)} />
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
          {#if thinkingText}
            <ThinkingStatusLine
              {thinkingText}
              isStreaming={true}
              onclick={() => onThinkingClick?.()}
            />
          {/if}
          {#if activeToolCalls.length > 0}
            <ToolStatusLine
              tools={activeToolCalls}
              onclick={() => onToolClick?.(activeToolCalls)}
            />
          {/if}
          {#if streamingText}
            <div class="chat-content">
              <Markdown content={streamingText} />
            </div>
          {:else if activeToolCalls.length === 0 && !thinkingText}
            <StreamingIndicator />
          {/if}
        </div>
      </div>
    {/if}
  {/if}

  {#if !isNearBottom && isStreaming}
    <button class="scroll-btn" onclick={scrollToBottom} aria-label="Scroll to newest messages">
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
    gap: 8px;
  }
  .empty-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-top: 4px;
  }
  .empty-hint {
    font-size: 13px;
    color: var(--text-muted);
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
    color: #0f1114;
    letter-spacing: 1px;
  }

  .scroll-btn {
    position: sticky;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--accent);
    color: #0f1114;
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

  @media (max-width: 768px) {
    .scroll-btn {
      bottom: 8px;
      padding: 8px 20px;
      font-size: 13px;
    }
    .empty-state {
      padding: 0 20px;
      text-align: center;
    }
  }
</style>
