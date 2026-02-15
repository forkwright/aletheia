<script lang="ts">
  import type { ToolCallState } from "../../lib/types";
  import ToolCall from "./ToolCall.svelte";

  let { tools, onClose }: {
    tools: ToolCallState[];
    onClose: () => void;
  } = $props();
</script>

<div class="tool-panel">
  <div class="panel-header">
    <span class="panel-title">Tool Calls ({tools.length})</span>
    <button class="close-btn" onclick={onClose}>x</button>
  </div>
  <div class="panel-body">
    {#each tools as tool}
      <ToolCall {tool} />
    {/each}
  </div>
</div>

<style>
  .tool-panel {
    width: 340px;
    flex-shrink: 0;
    border-left: 1px solid var(--border);
    background: var(--bg-elevated);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: slide-in 0.15s ease;
  }
  @keyframes slide-in {
    from { transform: translateX(20px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .panel-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    line-height: 1;
  }
  .close-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .panel-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
  }
</style>
