<script lang="ts">
  import type { ToolCallState } from "../../lib/types";
  import Spinner from "../shared/Spinner.svelte";

  let { tools, onclick }: {
    tools: ToolCallState[];
    onclick?: () => void;
  } = $props();

  let running = $derived(tools.filter(t => t.status === "running"));
  let completed = $derived(tools.filter(t => t.status !== "running").length);
  let errors = $derived(tools.filter(t => t.status === "error").length);
  let total = $derived(tools.length);

  /** Humanize a tool name into a readable activity label */
  function humanizeTool(name: string): string {
    switch (name) {
      case "exec": return "Running command";
      case "read": return "Reading file";
      case "write": return "Writing file";
      case "edit": return "Editing file";
      case "grep": return "Searching files";
      case "find": return "Finding files";
      case "ls": return "Listing directory";
      case "web_search": return "Searching the web";
      case "web_fetch": return "Fetching page";
      case "mem0_search": return "Searching memory";
      case "blackboard": return "Checking blackboard";
      case "sessions_send": return "Messaging agent";
      case "sessions_ask": return "Asking agent";
      case "sessions_spawn": return "Spawning worker";
      case "message": return "Sending message";
      case "enable_tool": return "Enabling tool";
      case "voice_reply": return "Sending voice message";
      default: return name.replace(/_/g, " ").replace(/\b\w/g, c => c.toUpperCase());
    }
  }

  let statusText = $derived.by(() => {
    if (running.length > 0) {
      return humanizeTool(running[running.length - 1]!.name);
    }
    if (errors > 0) {
      return `${total} tool${total === 1 ? '' : 's'} · ${errors} failed`;
    }
    return `${total} tool${total === 1 ? '' : 's'} completed`;
  });

  let isActive = $derived(running.length > 0);
</script>

<button
  class="tool-status-line"
  class:active={isActive}
  class:has-errors={errors > 0 && !isActive}
  onclick={() => onclick?.()}
  title="Click to view tool details"
>
  <span class="status-indicator">
    {#if isActive}
      <Spinner size={12} />
    {:else if errors > 0}
      <span class="icon-error">!</span>
    {:else}
      <span class="icon-done">✓</span>
    {/if}
  </span>
  <span class="status-text">{statusText}</span>
  {#if total > 1}
    <span class="status-count">{completed}/{total}</span>
  {/if}
  <span class="chevron">›</span>
</button>

<style>
  .tool-status-line {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    margin-bottom: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
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
  .tool-status-line:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
    color: var(--text);
  }
  .tool-status-line.active {
    border-color: rgba(88, 166, 255, 0.3);
    color: var(--text);
  }
  .tool-status-line.has-errors {
    border-color: rgba(248, 81, 73, 0.3);
    color: var(--red);
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
    color: var(--green);
    font-size: 11px;
    font-weight: 700;
  }
  .icon-error {
    color: var(--red);
    font-size: 11px;
    font-weight: 700;
  }
  .status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .status-count {
    color: var(--text-muted);
    font-size: 11px;
    font-family: var(--font-mono);
    flex-shrink: 0;
  }
  .chevron {
    color: var(--text-muted);
    font-size: 14px;
    flex-shrink: 0;
    transition: transform 0.15s;
  }
  .tool-status-line:hover .chevron {
    transform: translateX(1px);
    color: var(--accent);
  }
</style>
