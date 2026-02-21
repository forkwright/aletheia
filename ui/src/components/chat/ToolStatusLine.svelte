<script lang="ts">
  import type { ToolCallState } from "../../lib/types";
  import Spinner from "../shared/Spinner.svelte";

  let { tools, onclick }: {
    tools: ToolCallState[];
    onclick?: () => void;
  } = $props();

  const TOOL_CATEGORIES: Record<string, { icon: string; label: string }> = {
    read: { icon: "\u{1F4C1}", label: "fs" },
    write: { icon: "\u{1F4C1}", label: "fs" },
    edit: { icon: "\u{1F4C1}", label: "fs" },
    ls: { icon: "\u{1F4C1}", label: "fs" },
    find: { icon: "\u{1F50D}", label: "search" },
    grep: { icon: "\u{1F50D}", label: "search" },
    web_search: { icon: "\u{1F50D}", label: "search" },
    mem0_search: { icon: "\u{1F50D}", label: "search" },
    exec: { icon: "\u26A1", label: "exec" },
    sessions_send: { icon: "\u{1F4AC}", label: "comms" },
    sessions_ask: { icon: "\u{1F4AC}", label: "comms" },
    sessions_spawn: { icon: "\u{1F4AC}", label: "comms" },
    message: { icon: "\u{1F4AC}", label: "comms" },
    blackboard: { icon: "\u{1F9E0}", label: "system" },
    note: { icon: "\u{1F9E0}", label: "system" },
    enable_tool: { icon: "\u{1F9E0}", label: "system" },
    web_fetch: { icon: "\u{1F310}", label: "web" },
  };

  function getCategory(name: string): string {
    return TOOL_CATEGORIES[name]?.label ?? "other";
  }

  let running = $derived(tools.filter(t => t.status === "running"));
  let completed = $derived(tools.filter(t => t.status !== "running").length);
  let errors = $derived(tools.filter(t => t.status === "error").length);
  let total = $derived(tools.length);

  let categoryBreakdown = $derived.by(() => {
    if (running.length > 0 || total < 2) return "";
    const counts = new Map<string, { icon: string; count: number }>();
    for (const t of tools) {
      const cat = getCategory(t.name);
      const entry = TOOL_CATEGORIES[t.name] ?? { icon: "\u2699", label: "other" };
      const existing = counts.get(cat);
      if (existing) existing.count++;
      else counts.set(cat, { icon: entry.icon, count: 1 });
    }
    return [...counts.values()].map(c => `${c.icon}${c.count}`).join(" ");
  });

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

  /** Get a short input summary for a running tool */
  function inputHint(tool: ToolCallState): string {
    if (!tool.input) return "";
    const inp = tool.input;
    switch (tool.name) {
      case "exec": {
        const cmd = String(inp.command ?? "");
        return cmd.length > 40 ? cmd.slice(0, 37) + "..." : cmd;
      }
      case "read":
      case "write":
      case "edit":
      case "ls": {
        const p = String(inp.path ?? inp.file ?? "");
        const parts = p.split("/");
        return parts.length > 1 ? parts.slice(-2).join("/") : p;
      }
      case "grep":
        return `/${inp.pattern ?? ""}/`;
      case "web_search":
      case "mem0_search":
        return String(inp.query ?? "").slice(0, 40);
      default:
        return "";
    }
  }

  let elapsed = $state(0);
  let runStart = $state(0);

  $effect(() => {
    if (running.length > 0) {
      if (!runStart) runStart = Date.now();
      elapsed = Math.floor((Date.now() - runStart) / 1000);
      const iv = setInterval(() => {
        elapsed = Math.floor((Date.now() - runStart) / 1000);
      }, 1000);
      return () => clearInterval(iv);
    } else {
      runStart = 0;
      elapsed = 0;
    }
  });

  function formatElapsed(s: number): string {
    if (s < 60) return `${s}s`;
    return `${Math.floor(s / 60)}m${s % 60}s`;
  }

  let statusText = $derived.by(() => {
    if (running.length > 0) {
      const current = running[running.length - 1]!;
      const icon = TOOL_CATEGORIES[current.name]?.icon ?? "\u2699";
      const hint = inputHint(current);
      const label = humanizeTool(current.name);
      const time = elapsed > 0 ? ` (${formatElapsed(elapsed)})` : "";
      return hint ? `${icon} ${label}: ${hint}${time}` : `${icon} ${label}${time}`;
    }
    if (errors > 0) {
      return `${total} tool${total === 1 ? '' : 's'} · ${errors} failed`;
    }
    if (categoryBreakdown) {
      return categoryBreakdown;
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
