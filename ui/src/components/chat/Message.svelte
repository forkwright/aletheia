<script lang="ts">
  import type { ChatMessage, ToolCallState } from "../../lib/types";
  import { formatTimestamp, formatDuration } from "../../lib/format";
  import Markdown from "./Markdown.svelte";
  import ToolStatusLine from "./ToolStatusLine.svelte";
  import ThinkingStatusLine from "./ThinkingStatusLine.svelte";

  let { message, agentName, agentEmoji, onToolClick, onThinkingClick }: {
    message: ChatMessage;
    agentName?: string | null;
    agentEmoji?: string | null;
    onToolClick?: (tools: ToolCallState[]) => void;
    onThinkingClick?: (thinking: string) => void;
  } = $props();

  let isUser = $derived(message.role === "user");
  let initials = $derived(agentName ? agentName.slice(0, 2).toUpperCase() : "AI");
  let hasMedia = $derived(message.media && message.media.length > 0);

  // Detect special message types for alternate rendering
  let isDistillationSummary = $derived(
    !isUser && message.content.startsWith("[Distillation #"),
  );
  let isTopicBoundary = $derived(
    message.content.startsWith("[TOPIC:") || message.content === "[TOPIC]",
  );
  let topicLabel = $derived(
    isTopicBoundary
      ? message.content.replace(/^\[TOPIC:\s*/, "").replace(/\]$/, "").trim() || "New topic"
      : "",
  );
  let distillationLabel = $derived(
    isDistillationSummary
      ? (message.content.match(/\[Distillation #(\d+)\]/)?.[0] ?? "Memory consolidated")
      : "",
  );

  let expandedImage = $state<string | null>(null);
  let summaryExpanded = $state(false);

  function openLightbox(src: string) {
    expandedImage = src;
  }

  function closeLightbox() {
    expandedImage = null;
  }
</script>

{#if isTopicBoundary}
  <div class="topic-boundary">
    <span class="topic-label">{topicLabel}</span>
  </div>
{:else if isDistillationSummary}
  <div class="segment-boundary">
    <div class="segment-line"></div>
    <button class="segment-label" onclick={() => (summaryExpanded = !summaryExpanded)}>
      {distillationLabel} — {summaryExpanded ? "hide" : "show"} summary
    </button>
    <div class="segment-line"></div>
  </div>
  {#if summaryExpanded}
    <div class="segment-summary">
      <Markdown content={message.content.replace(/^\[Distillation #\d+\]\n\n/, "")} />
    </div>
  {/if}
{:else}
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
      <ToolStatusLine
        tools={message.toolCalls}
        onclick={() => onToolClick?.(message.toolCalls!)}
      />
    {/if}
    {#if hasMedia}
      <div class="msg-media" class:single={message.media!.length === 1} class:grid={message.media!.length > 1}>
        {#each message.media! as item}
          {#if item.contentType.startsWith("image/")}
            <button class="media-thumb" onclick={() => openLightbox(`data:${item.contentType};base64,${item.data}`)}>
              <img
                src="data:{item.contentType};base64,{item.data}"
                alt={item.filename ?? "image"}
              />
            </button>
          {:else if item.contentType === "application/pdf"}
            <div class="file-attachment">
              <span class="file-att-icon">📄</span>
              <span class="file-att-name">{item.filename ?? "document.pdf"}</span>
            </div>
          {:else}
            <div class="file-attachment">
              <span class="file-att-icon">📝</span>
              <span class="file-att-name">{item.filename ?? "file"}</span>
            </div>
          {/if}
        {/each}
      </div>
    {/if}
    {#if message.thinking}
      <ThinkingStatusLine
        thinkingText={message.thinking}
        onclick={() => onThinkingClick?.(message.thinking!)}
      />
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

{#if expandedImage}
  <div class="lightbox" onclick={closeLightbox} onkeydown={(e) => e.key === "Escape" && closeLightbox()} role="dialog" tabindex="-1">
    <img src={expandedImage} alt="Expanded view" />
    <button class="lightbox-close" onclick={closeLightbox} aria-label="Close">×</button>
  </div>
{/if}
{/if}

<style>
  /* Segment boundary (distillation summary) */
  .segment-boundary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    margin: 4px 0;
  }
  .segment-line {
    flex: 1;
    height: 1px;
    background: var(--border);
    opacity: 0.5;
  }
  .segment-label {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-muted);
    background: none;
    border: none;
    padding: 2px 6px;
    cursor: pointer;
    border-radius: 3px;
    white-space: nowrap;
  }
  .segment-label:hover {
    color: var(--text-secondary);
    background: var(--surface);
  }
  .segment-summary {
    padding: 10px 16px;
    margin: 0 0 8px;
    border-left: 2px solid var(--border);
    font-size: 13px;
    color: var(--text-muted);
    background: var(--surface);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  /* Topic boundary */
  .topic-boundary {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 12px 0 8px;
    margin: 4px 0;
  }
  .topic-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--accent);
    background: var(--surface);
    padding: 3px 10px;
    border-radius: 12px;
    border: 1px solid var(--border);
  }

  .user-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .timestamp {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 4px;
  }

  /* Media in messages */
  .msg-media {
    margin-bottom: 8px;
  }
  .msg-media.single {
    max-width: 400px;
  }
  .msg-media.grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 6px;
    max-width: 500px;
  }
  .media-thumb {
    display: block;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 0;
    overflow: hidden;
    cursor: pointer;
    transition: border-color 0.15s;
  }
  .media-thumb:hover {
    border-color: var(--accent);
  }
  .media-thumb img {
    display: block;
    width: 100%;
    height: auto;
    max-height: 300px;
    object-fit: contain;
    background: var(--surface);
  }
  .msg-media.grid .media-thumb img {
    height: 150px;
    object-fit: cover;
  }

  /* File attachments */
  .file-attachment {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    color: var(--text-secondary);
  }
  .file-att-icon {
    font-size: 20px;
    flex-shrink: 0;
  }
  .file-att-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  /* Lightbox */
  .lightbox {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.85);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 32px;
  }
  .lightbox img {
    max-width: 90vw;
    max-height: 90vh;
    object-fit: contain;
    border-radius: var(--radius);
  }
  .lightbox-close {
    position: absolute;
    top: 16px;
    right: 16px;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.1);
    border: none;
    color: #fff;
    font-size: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.15s;
  }
  .lightbox-close:hover {
    background: rgba(255, 255, 255, 0.2);
  }
</style>
