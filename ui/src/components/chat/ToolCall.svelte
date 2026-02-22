<script lang="ts">
  import type { ToolCallState } from "../../lib/types";
  import { formatDuration } from "../../lib/format";
  import Spinner from "../shared/Spinner.svelte";

  let { tool }: { tool: ToolCallState } = $props();
  let expanded = $state(false);
  $effect(() => { if (tool.status === "error") expanded = true; });
</script>

<div class="tool-call" class:running={tool.status === "running"} class:error={tool.status === "error"}>
  <button class="tool-header" onclick={() => expanded = !expanded}>
    <span class="tool-icon">
      {#if tool.status === "running"}
        <Spinner size={14} />
      {:else if tool.status === "error"}
        <span class="icon-error">✕</span>
      {:else}
        <span class="icon-ok">✓</span>
      {/if}
    </span>
    <span class="tool-name">{tool.name}</span>
    {#if tool.durationMs != null}
      <span class="tool-duration">{formatDuration(tool.durationMs)}</span>
    {/if}
    <span class="expand-icon">{expanded ? "−" : "+"}</span>
  </button>
  {#if expanded && tool.result}
    <div class="tool-output">
      <pre>{tool.result}</pre>
    </div>
  {/if}
</div>

<style>
  .tool-call {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    margin: 6px 0;
    overflow: hidden;
    background: var(--surface);
  }
  .tool-call.running {
    border-color: var(--accent);
    animation: pulse-border 2s ease infinite;
  }
  .tool-call.error {
    border-color: var(--status-error);
  }
  @keyframes pulse-border {
    0%, 100% { border-color: var(--accent); }
    50% { border-color: var(--border); }
  }
  .tool-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
  }
  .tool-header:hover {
    background: var(--surface-hover);
  }
  .tool-header:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .tool-icon {
    display: flex;
    align-items: center;
    width: 20px;
    justify-content: center;
    flex-shrink: 0;
  }
  .icon-ok {
    color: var(--status-success);
    font-size: var(--text-sm);
    font-weight: 700;
  }
  .icon-error {
    color: var(--status-error);
    font-weight: 700;
    font-size: var(--text-sm);
  }
  .tool-name {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    flex: 1;
  }
  .tool-duration {
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
  }
  .expand-icon {
    color: var(--text-muted);
    font-size: var(--text-base);
    width: 16px;
    text-align: center;
  }
  .tool-output {
    border-top: 1px solid var(--border);
    padding: 8px 10px;
    max-height: 300px;
    overflow: auto;
  }
  .tool-output pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text-secondary);
  }
</style>
