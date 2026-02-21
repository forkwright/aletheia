<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";

  let { thinkingText, isStreaming = false, onclick }: {
    thinkingText: string;
    isStreaming?: boolean;
    onclick?: () => void;
  } = $props();

  function extractLiveSummary(text: string): string {
    if (!text || text.length < 10) return "Thinking...";

    const tail = text.slice(-300);

    // Try to find last complete sentence
    const sentenceMatch = tail.match(/[.!?]\s+([^.!?]+[.!?])\s*$/);
    if (sentenceMatch) return sentenceMatch[1]!.trim().slice(0, 80);

    // Fallback: last non-empty line
    const lines = tail.split("\n").filter(Boolean);
    const last = lines[lines.length - 1];
    if (last && last.length > 10) {
      const trimmed = last.trim();
      return trimmed.length > 80 ? trimmed.slice(0, 77) + "..." : trimmed;
    }

    return "Thinking...";
  }

  function generateCompletedSummary(text: string): string {
    if (!text || text.length < 10) return "Thought process";

    const sentences = text.match(/[^.!?\n]+[.!?]+/g) ?? [];
    if (sentences.length === 0) {
      const firstLine = text.split("\n").filter(Boolean)[0];
      return firstLine ? firstLine.trim().slice(0, 80) : "Thought process";
    }
    if (sentences.length === 1) return sentences[0]!.trim().slice(0, 80);

    const first = sentences[0]!.trim().slice(0, 40);
    const last = sentences[sentences.length - 1]!.trim().slice(0, 40);
    return `${first}${first.length >= 40 ? "..." : ""} \u2192 ${last}${last.length >= 40 ? "..." : ""}`;
  }

  let summary = $derived(isStreaming ? extractLiveSummary(thinkingText) : generateCompletedSummary(thinkingText));
</script>

<button
  class="thinking-status-line"
  class:active={isStreaming}
  onclick={() => onclick?.()}
  title="Click to view full thinking"
>
  <span class="status-indicator">
    {#if isStreaming}
      <Spinner size={12} />
    {:else}
      <span class="icon-done">&#x2713;</span>
    {/if}
  </span>
  <span class="status-text">{summary}</span>
  <span class="chevron">&rsaquo;</span>
</button>

<style>
  .thinking-status-line {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    margin-bottom: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--amber);
    border-radius: 14px;
    color: var(--text-secondary);
    font-size: 12px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s, color 0.15s;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
  }
  .thinking-status-line:hover {
    background: var(--surface-hover);
    border-color: var(--amber);
    color: var(--text);
  }
  .thinking-status-line.active {
    border-color: var(--amber);
    color: var(--text);
  }
  .status-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .icon-done {
    color: var(--amber);
    font-size: 11px;
    font-weight: 700;
  }
  .status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .chevron {
    color: var(--text-muted);
    font-size: 14px;
    flex-shrink: 0;
    transition: transform 0.15s;
  }
  .thinking-status-line:hover .chevron {
    transform: translateX(1px);
    color: var(--amber);
  }

  @media (max-width: 768px) {
    .thinking-status-line {
      font-size: 11px;
      padding: 4px 8px;
      max-width: calc(100vw - 80px);
    }
  }
</style>
