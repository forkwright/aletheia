<script lang="ts">
  import Markdown from "./Markdown.svelte";

  let { thinkingText, isStreaming = false, onClose }: {
    thinkingText: string;
    isStreaming?: boolean;
    onClose: () => void;
  } = $props();

  let scrollContainer = $state<HTMLDivElement | null>(null);
  let autoScroll = $state(true);

  function checkScroll() {
    if (!scrollContainer) return;
    const threshold = 40;
    autoScroll =
      scrollContainer.scrollHeight - scrollContainer.scrollTop - scrollContainer.clientHeight < threshold;
  }

  $effect(() => {
    void thinkingText;
    if (autoScroll && scrollContainer) {
      requestAnimationFrame(() => {
        if (scrollContainer) scrollContainer.scrollTop = scrollContainer.scrollHeight;
      });
    }
  });
</script>

<div class="thinking-panel">
  <div class="panel-header">
    <div class="header-top">
      <span class="panel-title">Thinking</span>
      {#if isStreaming}
        <span class="live-badge">Live</span>
      {/if}
      <button class="close-btn" onclick={onClose} aria-label="Close">&times;</button>
    </div>
  </div>
  <div
    class="panel-body"
    bind:this={scrollContainer}
    onscroll={checkScroll}
  >
    {#if thinkingText}
      <div class="thinking-content">
        <Markdown content={thinkingText} />
      </div>
    {:else}
      <div class="empty-thinking">No thinking content yet.</div>
    {/if}
  </div>
</div>

<style>
  .thinking-panel {
    width: 380px;
    max-width: 100%;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border-left: 1px solid var(--border);
    animation: slide-in 0.15s ease;
    overflow: hidden;
  }
  @keyframes slide-in {
    from { transform: translateX(16px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .panel-header {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .header-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .panel-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    flex: 1;
  }
  .live-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--amber);
    background: rgba(232, 169, 42, 0.1);
    padding: 2px 6px;
    border-radius: 8px;
    border: 1px solid rgba(232, 169, 42, 0.2);
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.7; }
    50% { opacity: 1; }
  }
  .close-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: background 0.15s, color 0.15s;
    flex-shrink: 0;
  }
  .close-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .panel-body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 12px;
    min-height: 0;
  }
  .thinking-content {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.5;
  }
  .thinking-content :global(.markdown-body) {
    font-size: 13px;
  }
  .thinking-content :global(p) {
    margin: 0 0 6px;
  }
  .thinking-content :global(pre) {
    font-size: 12px;
  }
  .empty-thinking {
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
    padding: 24px;
  }

  @media (max-width: 768px) {
    .thinking-panel {
      width: 100%;
      position: absolute;
      inset: 0;
      z-index: 20;
    }
  }
</style>
