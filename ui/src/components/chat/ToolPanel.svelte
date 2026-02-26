<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import type { ToolCallState } from "../../lib/types";
  import { formatDuration } from "../../lib/format";
  import { getToolCategory } from "../../lib/tools";
  import { highlightCode, inferLanguage } from "../../lib/markdown";
  import DOMPurify from "dompurify";
  import Spinner from "../shared/Spinner.svelte";

  let { tools, onClose }: {
    tools: ToolCallState[];
    onClose: () => void;
  } = $props();

  let expandedIds = new SvelteSet<string>();

  // Auto-expand errors — untrack expandedIds to avoid reactive cycle
  $effect(() => {
    const errorIds = tools.filter(t => t.status === "error").map(t => t.id);
    if (errorIds.length > 0) {
      expandedIds = new SvelteSet([...untrack(() => expandedIds), ...errorIds]);
    }
  });

  function toggleExpand(id: string) {
    const next = new SvelteSet(expandedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedIds = next;
  }

  function expandAll() {
    expandedIds = new SvelteSet(tools.map(t => t.id));
  }

  function collapseAll() {
    expandedIds = new SvelteSet();
  }

  let completed = $derived(tools.filter(t => t.status === "complete").length);
  let errors = $derived(tools.filter(t => t.status === "error").length);
  let running = $derived(tools.filter(t => t.status === "running").length);
  let totalDuration = $derived(
    tools.reduce((sum, t) => sum + (t.durationMs ?? 0), 0)
  );

  let categoryStats = $derived.by(() => {
    const counts = new SvelteMap<string, { icon: string; count: number }>();
    for (const t of tools) {
      const cat = getToolCategory(t.name);
      const existing = counts.get(cat.label);
      if (existing) existing.count++;
      else counts.set(cat.label, { icon: cat.icon, count: 1 });
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
  let collapsedIds = new SvelteSet<string>();

  function toggleCollapse(id: string) {
    const next = new SvelteSet(collapsedIds);
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

  // Cache highlighted output per tool — avoids re-running hljs/DOMPurify on every tools update
  const resultCache = new SvelteMap<string, { raw: string; html: string }>();

  function highlightResult(tool: ToolCallState): string {
    if (!tool.result) return "";
    const cached = resultCache.get(tool.id);
    if (cached && cached.raw === tool.result) return cached.html;
    let html: string;
    if (isDiffResult(tool)) {
      html = DOMPurify.sanitize(renderDiff(tool.result), { ADD_ATTR: ["class"] });
    } else {
      const lang = inferLanguage(tool.name, tool.result);
      if (lang) {
        html = DOMPurify.sanitize(highlightCode(tool.result, lang), { ADD_ATTR: ["class"] });
      } else {
        html = tool.result
          .replace(/&/g, "&amp;")
          .replace(/</g, "&lt;")
          .replace(/>/g, "&gt;");
      }
    }
    resultCache.set(tool.id, { raw: tool.result, html });
    return html;
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
      <button class="toggle-btn" onclick={expandAll} title="Expand all" aria-label="Expand all tool calls">⊞</button>
      <button class="toggle-btn" onclick={collapseAll} title="Collapse all" aria-label="Collapse all tool calls">⊟</button>
    </div>
    {#if categoryStats.length > 1}
      <div class="header-categories">
        {#each categoryStats as cat (cat.icon)}
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
              <!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized via highlightResult() which calls DOMPurify.sanitize() -->
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
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
  }
  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-xl);
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
    font-size: var(--text-xs);
    color: var(--text-secondary);
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .stat.ok { color: var(--status-success); }
  .stat.err { color: var(--status-error); }
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
    font-size: var(--text-xs);
    color: var(--text-muted);
    letter-spacing: -0.5px;
  }
  .toggle-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    padding: 2px 4px;
    border-radius: var(--radius-sm);
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
    background: var(--status-error-bg);
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
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
    transition: background var(--transition-quick);
  }
  .tool-row:hover {
    background: var(--surface-hover);
  }
  .tool-idx {
    color: var(--text-muted);
    font-size: var(--text-2xs);
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
    font-size: var(--text-2xs);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tool-status-icon.complete { color: var(--status-success); }
  .tool-status-icon.error { color: var(--status-error); }
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
    font-size: var(--text-2xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tool-input-summary {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }
  .tool-time {
    color: var(--text-muted);
    font-size: var(--text-2xs);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }
  .tool-tokens {
    color: var(--text-muted);
    font-size: var(--text-2xs);
    font-family: var(--font-mono);
    flex-shrink: 0;
    opacity: 0.7;
  }
  .tool-chevron {
    color: var(--text-muted);
    font-size: var(--text-sm);
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }

  /* Preview — collapsed one-liner */
  .tool-preview {
    padding: 0 14px 4px 52px;
    font-size: var(--text-xs);
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
    font-size: var(--text-2xs);
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
    font-size: var(--text-xs);
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
    font-size: var(--text-xs);
    padding: 2px 0;
    cursor: pointer;
    margin-top: 2px;
  }
  .collapse-toggle:hover {
    text-decoration: underline;
  }
  /* Diff rendering */
  .tool-result :global(.diff-add) {
    color: var(--syntax-inserted);
    background: var(--diff-add-bg);
    display: inline-block;
    width: 100%;
  }
  .tool-result :global(.diff-del) {
    color: var(--syntax-deleted);
    background: var(--diff-del-bg);
    display: inline-block;
    width: 100%;
  }
  .tool-result :global(.diff-hunk) {
    color: var(--status-info);
    font-weight: 500;
  }

  /* hljs tokens in tool results */
  .tool-result :global(.hljs-keyword) { color: var(--syntax-keyword); }
  .tool-result :global(.hljs-string),
  .tool-result :global(.hljs-regexp) { color: var(--syntax-string); }
  .tool-result :global(.hljs-number) { color: var(--syntax-number); }
  .tool-result :global(.hljs-comment) { color: var(--syntax-comment); }
  .tool-result :global(.hljs-built_in) { color: var(--syntax-builtin); }
  .tool-result :global(.hljs-function),
  .tool-result :global(.hljs-title) { color: var(--syntax-function); }
  .tool-result :global(.hljs-property) { color: var(--syntax-number); }
  .tool-result :global(.hljs-tag) { color: var(--syntax-tag); }
  .tool-result :global(.hljs-name) { color: var(--syntax-tag); }
  .tool-result :global(.hljs-attr) { color: var(--syntax-number); }
  .tool-result :global(.hljs-addition) { color: var(--syntax-inserted); background: var(--diff-add-bg); }
  .tool-result :global(.hljs-deletion) { color: var(--syntax-deleted); background: var(--diff-del-bg); }

  @media (max-width: 768px) {
    .tool-panel {
      width: 100%;
      max-width: 100%;
      position: absolute;
      right: 0;
      top: 0;
      bottom: 0;
      z-index: 50;
      border-left: none;
    }
    .panel-header {
      padding: 12px 14px 8px;
      padding-top: calc(12px);
    }
    .tool-row {
      padding: 8px 14px;
    }
    .tool-detail {
      padding: 4px 10px 8px 36px;
    }
    .tool-preview {
      padding: 0 10px 4px 36px;
    }
    .tool-result {
      font-size: var(--text-2xs);
    }
    .tool-input-json {
      font-size: var(--text-2xs);
    }
  }
</style>
