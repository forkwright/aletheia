<script lang="ts">
  import type { ToolCallState } from "../../lib/types";
  import { formatDuration } from "../../lib/format";
  import { highlightCode, inferLanguage } from "../../lib/markdown";
  import DOMPurify from "dompurify";
  import Spinner from "../shared/Spinner.svelte";

  let { tools, onClose }: {
    tools: ToolCallState[];
    onClose: () => void;
  } = $props();

  let expandedIds = $state<Set<string>>(new Set());

  // Auto-expand errors
  $effect(() => {
    const errorIds = tools.filter(t => t.status === "error").map(t => t.id);
    if (errorIds.length > 0) {
      expandedIds = new Set([...expandedIds, ...errorIds]);
    }
  });

  function toggleExpand(id: string) {
    const next = new Set(expandedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedIds = next;
  }

  function expandAll() {
    expandedIds = new Set(tools.map(t => t.id));
  }

  function collapseAll() {
    expandedIds = new Set();
  }

  let completed = $derived(tools.filter(t => t.status === "complete").length);
  let errors = $derived(tools.filter(t => t.status === "error").length);
  let running = $derived(tools.filter(t => t.status === "running").length);
  let totalDuration = $derived(
    tools.reduce((sum, t) => sum + (t.durationMs ?? 0), 0)
  );

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

  let categoryStats = $derived.by(() => {
    const counts = new Map<string, { icon: string; count: number }>();
    for (const t of tools) {
      const entry = TOOL_CATEGORIES[t.name] ?? { icon: "\u2699", label: "other" };
      const existing = counts.get(entry.label);
      if (existing) existing.count++;
      else counts.set(entry.label, { icon: entry.icon, count: 1 });
    }
    return [...counts.values()];
  });

  /** Humanize a tool name */
  function humanize(name: string): string {
    switch (name) {
      case "exec": return "Run command";
      case "read": return "Read file";
      case "write": return "Write file";
      case "edit": return "Edit file";
      case "grep": return "Search files";
      case "find": return "Find files";
      case "ls": return "List directory";
      case "web_search": return "Search web";
      case "web_fetch": return "Fetch page";
      case "mem0_search": return "Search memory";
      case "blackboard": return "Blackboard";
      case "sessions_send": return "Message agent";
      case "sessions_ask": return "Ask agent";
      case "sessions_spawn": return "Spawn worker";
      case "message": return "Send message";
      case "enable_tool": return "Enable tool";
      default: return name;
    }
  }

  /** Summarize tool input for inline display */
  function getInputSummary(tool: ToolCallState): string {
    if (!tool.input) return "";
    const inp = tool.input;
    switch (tool.name) {
      case "exec": {
        const cmd = String(inp.command ?? "");
        return cmd.length > 60 ? cmd.slice(0, 57) + "..." : cmd;
      }
      case "read":
      case "write":
      case "edit":
      case "ls":
        return String(inp.path ?? inp.file ?? "");
      case "grep": {
        const pattern = String(inp.pattern ?? "");
        const path = String(inp.path ?? "");
        return path ? `/${pattern}/ in ${path}` : `/${pattern}/`;
      }
      case "find":
        return `${inp.pattern ?? ""} in ${inp.path ?? ""}`;
      case "web_search":
      case "mem0_search":
        return String(inp.query ?? "");
      case "web_fetch":
        return String(inp.url ?? "");
      case "sessions_send":
      case "sessions_ask":
        return `\u2192 ${inp.agentId ?? inp.targetAgent ?? ""}`;
      case "blackboard":
        return `${inp.action ?? ""} ${inp.key ?? ""}`;
      default:
        return "";
    }
  }

  function statusIcon(status: string): string {
    switch (status) {
      case "running": return "";
      case "error": return "✕";
      default: return "✓";
    }
  }

  /** Truncate tool result for preview */
  function previewResult(result: string | undefined): string {
    if (!result) return "";
    const trimmed = result.trim();
    if (trimmed.length <= 120) return trimmed;
    return trimmed.slice(0, 120) + "…";
  }

  const COLLAPSE_THRESHOLD = 20;
  let collapsedIds = $state<Set<string>>(new Set());

  function toggleCollapse(id: string) {
    const next = new Set(collapsedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    collapsedIds = next;
  }

  function resultLineCount(result: string | undefined): number {
    if (!result) return 0;
    return result.split("\n").length;
  }

  function isCollapsible(tool: ToolCallState): boolean {
    return resultLineCount(tool.result) > COLLAPSE_THRESHOLD;
  }

  function isCollapsed(tool: ToolCallState): boolean {
    return isCollapsible(tool) && !collapsedIds.has(tool.id);
  }

  function isDiffResult(tool: ToolCallState): boolean {
    if (tool.name === "edit") return true;
    if (!tool.result) return false;
    // Detect unified diff markers
    const r = tool.result;
    return (r.startsWith("---") && r.includes("+++")) || r.includes("@@ ");
  }

  function renderDiff(result: string): string {
    const lines = result.split("\n");
    return lines.map(line => {
      const escaped = line
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
      if (line.startsWith("+") && !line.startsWith("+++")) {
        return `<span class="diff-add">${escaped}</span>`;
      }
      if (line.startsWith("-") && !line.startsWith("---")) {
        return `<span class="diff-del">${escaped}</span>`;
      }
      if (line.startsWith("@@")) {
        return `<span class="diff-hunk">${escaped}</span>`;
      }
      return escaped;
    }).join("\n");
  }

  function highlightResult(tool: ToolCallState): string {
    if (!tool.result) return "";
    // Render diffs with diff coloring
    if (isDiffResult(tool)) {
      return DOMPurify.sanitize(renderDiff(tool.result), { ADD_ATTR: ["class"] });
    }
    const lang = inferLanguage(tool.name, tool.result);
    if (lang) {
      return DOMPurify.sanitize(highlightCode(tool.result, lang), { ADD_ATTR: ["class"] });
    }
    // Escape HTML for non-highlighted results
    return tool.result
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }
</script>

<div class="tool-panel">
  <div class="panel-header">
    <div class="header-top">
      <span class="panel-title">Tool Activity</span>
      <button class="close-btn" onclick={onClose} aria-label="Close panel">×</button>
    </div>
    <div class="header-stats">
      {#if running > 0}
        <span class="stat running"><Spinner size={10} /> {running} running</span>
      {/if}
      {#if completed > 0}
        <span class="stat ok">✓ {completed}</span>
      {/if}
      {#if errors > 0}
        <span class="stat err">✕ {errors}</span>
      {/if}
      {#if totalDuration > 0}
        <span class="stat time">{formatDuration(totalDuration)}</span>
      {/if}
      <span class="stat-spacer"></span>
      <button class="toggle-btn" onclick={expandAll} title="Expand all">⊞</button>
      <button class="toggle-btn" onclick={collapseAll} title="Collapse all">⊟</button>
    </div>
    {#if categoryStats.length > 1}
      <div class="header-categories">
        {#each categoryStats as cat}
          <span class="cat-badge">{cat.icon}{cat.count}</span>
        {/each}
      </div>
    {/if}
  </div>
  <div class="panel-body">
    {#each tools as tool, i (tool.id)}
      <div
        class="tool-item"
        class:running={tool.status === "running"}
        class:error={tool.status === "error"}
        class:expanded={expandedIds.has(tool.id)}
      >
        <button class="tool-row" onclick={() => toggleExpand(tool.id)}>
          <span class="tool-idx">{i + 1}</span>
          <span class="tool-status-icon" class:running={tool.status === "running"} class:complete={tool.status === "complete"} class:error={tool.status === "error"}>
            {#if tool.status === "running"}
              <Spinner size={11} />
            {:else}
              {statusIcon(tool.status)}
            {/if}
          </span>
          <span class="tool-label">
            <span class="tool-name">{humanize(tool.name)}</span>
            {#if getInputSummary(tool)}
              <span class="tool-input-summary">{getInputSummary(tool)}</span>
            {:else if tool.name !== humanize(tool.name)}
              <span class="tool-raw">{tool.name}</span>
            {/if}
          </span>
          {#if tool.durationMs != null}
            <span class="tool-time">{formatDuration(tool.durationMs)}</span>
          {/if}
          {#if tool.tokenEstimate}
            <span class="tool-tokens">~{tool.tokenEstimate > 999 ? `${(tool.tokenEstimate / 1000).toFixed(1)}k` : tool.tokenEstimate} tok</span>
          {/if}
          <span class="tool-chevron">{expandedIds.has(tool.id) ? "−" : "+"}</span>
        </button>
        {#if expandedIds.has(tool.id) && (tool.result || tool.input)}
          <div class="tool-detail">
            {#if tool.input}
              <pre class="tool-input-json">{JSON.stringify(tool.input, null, 2)}</pre>
            {/if}
            {#if tool.result}
              <pre class="tool-result" class:collapsed={isCollapsed(tool)}>{@html highlightResult(tool)}</pre>
            {/if}
            {#if isCollapsible(tool)}
              <button class="collapse-toggle" onclick={() => toggleCollapse(tool.id)}>
                {isCollapsed(tool) ? `Show all ${resultLineCount(tool.result)} lines` : "Show less"}
              </button>
            {/if}
          </div>
        {:else if !expandedIds.has(tool.id) && tool.result}
          <div class="tool-preview">{previewResult(tool.result)}</div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .tool-panel {
    width: 380px;
    max-width: 45vw;
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
    padding: 12px 14px 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .header-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  .panel-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    line-height: 1;
    cursor: pointer;
  }
  .close-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .header-stats {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text-secondary);
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .stat.ok { color: var(--green); }
  .stat.err { color: var(--red); }
  .stat.running { color: var(--accent); }
  .stat.time { color: var(--text-muted); font-family: var(--font-mono); }
  .stat-spacer { flex: 1; }
  .header-categories {
    display: flex;
    gap: 6px;
    padding: 4px 0 0;
    flex-wrap: wrap;
  }
  .cat-badge {
    font-size: 11px;
    color: var(--text-muted);
    letter-spacing: -0.5px;
  }
  .toggle-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    padding: 2px 4px;
    border-radius: 3px;
    cursor: pointer;
    line-height: 1;
  }
  .toggle-btn:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .panel-body {
    flex: 1;
    overflow-y: auto;
    padding: 6px 0;
  }

  /* Individual tool items */
  .tool-item {
    border-bottom: 1px solid rgba(48, 54, 61, 0.5);
  }
  .tool-item:last-child {
    border-bottom: none;
  }
  .tool-item.error {
    background: rgba(248, 81, 73, 0.04);
  }
  .tool-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 14px;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: background 0.1s;
  }
  .tool-row:hover {
    background: var(--surface-hover);
  }
  .tool-idx {
    color: var(--text-muted);
    font-size: 10px;
    font-family: var(--font-mono);
    width: 18px;
    text-align: right;
    flex-shrink: 0;
    opacity: 0.6;
  }
  .tool-status-icon {
    width: 14px;
    text-align: center;
    flex-shrink: 0;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tool-status-icon.complete { color: var(--green); }
  .tool-status-icon.error { color: var(--red); }
  .tool-status-icon.running { color: var(--accent); }
  .tool-label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 6px;
    overflow: hidden;
  }
  .tool-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tool-raw {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tool-input-summary {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }
  .tool-time {
    color: var(--text-muted);
    font-size: 10px;
    font-family: var(--font-mono);
    flex-shrink: 0;
  }
  .tool-tokens {
    color: var(--text-muted);
    font-size: 9px;
    font-family: var(--font-mono);
    flex-shrink: 0;
    opacity: 0.7;
  }
  .tool-chevron {
    color: var(--text-muted);
    font-size: 13px;
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }

  /* Preview — collapsed one-liner */
  .tool-preview {
    padding: 0 14px 4px 52px;
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.4;
  }

  /* Expanded detail */
  .tool-detail {
    padding: 4px 14px 8px 52px;
    max-height: 300px;
    overflow: auto;
  }
  .tool-input-json {
    margin: 0 0 6px;
    font-family: var(--font-mono);
    font-size: 10px;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text-muted);
    background: var(--surface);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    border-left: 2px solid var(--accent);
    opacity: 0.8;
  }
  .tool-result {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text-secondary);
    background: var(--surface);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
  }
  .tool-result.collapsed {
    max-height: calc(1.5em * 10 + 16px);
    overflow: hidden;
    -webkit-mask-image: linear-gradient(to bottom, black 60%, transparent 100%);
    mask-image: linear-gradient(to bottom, black 60%, transparent 100%);
  }
  .collapse-toggle {
    display: block;
    background: none;
    border: none;
    color: var(--accent);
    font-size: 11px;
    padding: 2px 0;
    cursor: pointer;
    margin-top: 2px;
  }
  .collapse-toggle:hover {
    text-decoration: underline;
  }
  /* Diff rendering */
  .tool-result :global(.diff-add) {
    color: #aff5b4;
    background: rgba(63, 185, 80, 0.15);
    display: inline-block;
    width: 100%;
  }
  .tool-result :global(.diff-del) {
    color: #ffdcd7;
    background: rgba(248, 81, 73, 0.15);
    display: inline-block;
    width: 100%;
  }
  .tool-result :global(.diff-hunk) {
    color: var(--purple);
    font-weight: 500;
  }

  /* hljs tokens in tool results */
  .tool-result :global(.hljs-keyword) { color: #ff7b72; }
  .tool-result :global(.hljs-string),
  .tool-result :global(.hljs-regexp) { color: #a5d6ff; }
  .tool-result :global(.hljs-number) { color: #79c0ff; }
  .tool-result :global(.hljs-comment) { color: #8b949e; }
  .tool-result :global(.hljs-built_in) { color: #ffa657; }
  .tool-result :global(.hljs-function),
  .tool-result :global(.hljs-title) { color: #d2a8ff; }
  .tool-result :global(.hljs-property) { color: #79c0ff; }
  .tool-result :global(.hljs-tag) { color: #7ee787; }
  .tool-result :global(.hljs-name) { color: #7ee787; }
  .tool-result :global(.hljs-attr) { color: #79c0ff; }
  .tool-result :global(.hljs-addition) { color: #aff5b4; background: rgba(63, 185, 80, 0.15); }
  .tool-result :global(.hljs-deletion) { color: #ffdcd7; background: rgba(248, 81, 73, 0.15); }

  @media (max-width: 768px) {
    .tool-panel {
      width: 100%;
      max-width: 100%;
      position: absolute;
      right: 0;
      top: 0;
      bottom: 0;
      z-index: 50;
    }
  }
</style>
