# Spec: UI Interaction Quality — Thinking Persistence & Tool Detail

**Status:** Phase 1-3 done. Phase 4 next.
**Author:** Syn
**Date:** 2026-02-21

---

## Problem

Three interaction quality issues in the webchat UI make it harder than necessary for the user to understand what the agent did and why.

### 1. Thinking Pill/Panel Disappears After Turn Completes

During streaming, the thinking pill (`ThinkingStatusLine`) shows live thinking content — clickable to open the `ThinkingPanel` side panel. When the turn completes, the streaming state clears, the pill disappears from the streaming area, and the finalized message gets a new `ThinkingStatusLine` pill rendered from `message.thinking`.

But: the thinking panel itself closes. If the user had the panel open to watch the agent think, it snaps shut the instant the turn finishes. They have to click the pill on the completed message to reopen it. This is jarring and makes it feel like the thinking content was lost.

**Root cause:** In `ChatView.svelte`, the `ThinkingPanel` renders conditionally on `selectedThinking !== null`. During streaming, `thinkingIsLive = true` drives the panel from `getThinkingText(currentAgentId)`. On `turn_complete`, `chat.svelte.ts` clears `thinkingText = ""`, making the panel content empty. The next render cycle shows no panel because the live source is gone and `selectedThinking` was set to the live source (which is now empty).

The fix isn't just keeping the panel open — it's ensuring the transition from "live thinking" to "completed thinking" preserves the panel's content and open state without a flash.

### 2. Thinking Panel Formatting Is Raw

The thinking panel renders raw `<pre>` text with no formatting. Agent thinking often contains structured content — markdown headers, code blocks, bullet lists, numbered steps — that renders as a wall of monospace text. This makes it significantly harder to parse than it needs to be.

The thinking content isn't just stream-of-consciousness. It includes:
- Planning steps (numbered lists)
- Code analysis (code blocks with language hints)
- Decision trees (nested bullets)
- Key observations (bold/italic emphasis)
- Section breaks (headers)

All of this collapses into flat monospace text in the current `<pre>` rendering.

### 3. Tool Panel Lacks Actionable Detail

The `ToolPanel` currently shows:
- Tool name (humanized)
- Status icon (✓/✕/spinner)
- Duration
- Result preview (collapsed) / full result (expanded, with syntax highlighting)

What it doesn't show:
- **Tool inputs** — what command was run, what file was read, what pattern was searched. The user sees "Run command ✓ 1.2s" but has to expand and read the output to figure out *what* command. The most useful information is the input, not the output.
- **Categorization** — 28 tool calls in a turn are listed as a flat sequence. There's no grouping by type (filesystem ops, commands, memory searches) or by logical phase (investigation → implementation → verification).
- **Input summarization** — for `exec`, the command string IS the summary. For `read`, the file path. For `grep`, the pattern and path. These should be immediately visible without expanding.
- **Relationship context** — which tool calls were part of the same logical step? If the agent ran `grep` → `read` → `edit` → `read` on the same file, that's a single logical operation that should be visually grouped or at least connected.

---

## Design

### Phase 1: Thinking Panel Persistence ✅

**Goal:** Thinking panel stays open across the streaming→completed transition, content preserved seamlessly.

#### Changes to `ChatView.svelte`:

When `turn_complete` fires and `thinkingIsLive` is true (user is watching live thinking), capture the final thinking text before it's cleared:

```typescript
// In the stream event handler, on turn_complete:
case "turn_complete": {
  // If user was watching live thinking, transition to static
  if (thinkingIsLive) {
    selectedThinking = state.thinkingText; // Capture before clear
    thinkingIsLive = false;
    // Panel stays open with captured content
  }
  // ... rest of turn_complete handling
}
```

This requires the ChatView to subscribe to the stream events, or the chat store to expose a hook. The simplest approach: **don't clear `thinkingText` in the store on `turn_complete` — move it to the finalized message and let the panel read from the message.**

Actually, the better fix: the `turn_complete` handler in `chat.svelte.ts` already copies `state.thinkingText` into `assistantMsg.thinking`. The problem is that the panel is driven by `getThinkingText(agentId)` which reads from the transient store state, not from the message. Two options:

**Option A (minimal):** In `ChatView.svelte`, watch for `thinkingIsLive` transitioning from true to false (turn completed while panel was open). On that transition, set `selectedThinking` to the last known thinking text. Panel seamlessly shows static content.

```typescript
let previousThinkingLive = false;
$effect(() => {
  const isLive = thinkingIsLive && currentAgentId ? getIsStreaming(currentAgentId) : false;
  if (previousThinkingLive && !isLive && currentAgentId) {
    // Turn completed while panel was open — transition to static
    const lastThinking = getThinkingText(currentAgentId);
    if (lastThinking) {
      selectedThinking = lastThinking;
    } else {
      // Thinking already cleared; find it from the latest message
      const msgs = getMessages(currentAgentId);
      const lastMsg = msgs[msgs.length - 1];
      if (lastMsg?.thinking) {
        selectedThinking = lastMsg.thinking;
      }
    }
    thinkingIsLive = false;
  }
  previousThinkingLive = isLive;
});
```

**Option B (cleaner but more invasive):** Add a `lastThinkingText` field to `AgentChatState` that persists after `turn_complete`. The panel reads from this field when not live streaming. Cleared on next `turn_start` or when the user explicitly closes the panel.

**Recommendation:** Option A. It's a 15-line change in `ChatView.svelte` with no store changes. Option B is cleaner architecturally but touches the store contract.

### Phase 2: Thinking Panel Rendering ✅

**Goal:** Thinking content renders with Markdown formatting instead of raw `<pre>`.

#### Changes to `ThinkingPanel.svelte`:

Replace the `<pre class="thinking-text">` with the existing `Markdown` component, wrapped in a thinking-specific style container:

```svelte
{#if thinkingText}
  <div class="thinking-content">
    <Markdown content={thinkingText} />
  </div>
{:else}
  <div class="empty-thinking">No thinking content yet.</div>
{/if}
```

The `Markdown` component already handles code blocks, lists, headers, emphasis, etc. The thinking panel just needs appropriate styling:

```css
.thinking-content {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.6;
}

/* Thinking-specific overrides */
.thinking-content :global(h1),
.thinking-content :global(h2),
.thinking-content :global(h3) {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  margin: 12px 0 4px;
  border-bottom: none;
}

.thinking-content :global(pre) {
  font-size: 11.5px;
  background: var(--surface);
  border-radius: var(--radius-sm);
  padding: 8px;
  margin: 6px 0;
}

.thinking-content :global(ul),
.thinking-content :global(ol) {
  padding-left: 18px;
  margin: 4px 0;
}

.thinking-content :global(li) {
  margin: 2px 0;
}

.thinking-content :global(code) {
  font-size: 11.5px;
  background: var(--surface);
  padding: 1px 4px;
  border-radius: 3px;
}
```

**Streaming consideration:** During live streaming, the Markdown parser will re-parse on every chunk. This is fine — the existing `Markdown` component is already used for streaming text in the message area. The thinking text arrives as `thinking_delta` chunks and renders incrementally.

**Fallback:** If the thinking content is genuinely unstructured (no markdown markers), the Markdown component renders it as plain paragraphs — still better than `<pre>` because it wraps properly and respects paragraph breaks.

### Phase 3: Tool Input Display ✅

**Goal:** Show what each tool was called with, not just what it returned. The input IS the context.

#### 3a: Carry tool inputs through the event stream

Expand `tool_start` to include the tool's input parameters:

```typescript
// infrastructure/runtime/src/nous/pipeline/types.ts
| { type: "tool_start"; toolName: string; toolId: string; input?: Record<string, unknown> }

// ui/src/lib/types.ts
| { type: "tool_start"; toolName: string; toolId: string; input?: Record<string, unknown> }
```

In `execute.ts`, the tool input is available at the point where `tool_start` is yielded. Include it:

```typescript
yield { type: "tool_start", toolName: streamEvent.name, toolId: streamEvent.id, input: streamEvent.input };
```

#### 3b: Store tool input in `ToolCallState`

```typescript
export interface ToolCallState {
  id: string;
  name: string;
  status: "running" | "complete" | "error";
  input?: Record<string, unknown>;  // NEW
  result?: string;
  durationMs?: number;
}
```

In `chat.svelte.ts`, on `tool_start`:
```typescript
case "tool_start":
  state.activeToolCalls = [
    ...state.activeToolCalls,
    { id: event.toolId, name: event.toolName, status: "running", input: event.input },
  ];
  break;
```

In `historyToMessages`, parse input from the stored `tool_use` content blocks:
```typescript
currentToolCalls = toolBlocks.map((b: { id: string; name: string; input?: Record<string, unknown> }) => ({
  id: b.id,
  name: b.name,
  status: "complete" as const,
  input: b.input,
}));
```

#### 3c: Input summary in `ToolStatusLine`

Show the most useful input parameter as a subtitle under the humanized name. Each tool has a "primary input" that gives immediate context:

```typescript
function getInputSummary(name: string, input?: Record<string, unknown>): string {
  if (!input) return "";
  switch (name) {
    case "exec":
      return truncate(String(input.command ?? ""), 60);
    case "read":
      return String(input.path ?? "");
    case "write":
      return String(input.path ?? "");
    case "edit":
      return String(input.path ?? "");
    case "grep":
      return `/${input.pattern ?? ""}/ ${input.path ? `in ${input.path}` : ""}`.trim();
    case "find":
      return `${input.pattern ?? ""} ${input.path ? `in ${input.path}` : ""}`.trim();
    case "ls":
      return String(input.path ?? ".");
    case "web_search":
      return truncate(String(input.query ?? ""), 60);
    case "web_fetch":
      return truncate(String(input.url ?? ""), 60);
    case "mem0_search":
      return truncate(String(input.query ?? ""), 60);
    case "sessions_send":
    case "sessions_ask":
      return `→ ${input.agentId ?? "?"}`;
    case "sessions_spawn":
      return `${input.role ?? "worker"}`;
    case "message":
      return `→ ${input.to ?? "?"}`;
    case "blackboard":
      return `${input.action ?? "?"} ${input.key ? `[${input.key}]` : ""}`.trim();
    case "note":
      return `${input.action ?? "?"}`;
    case "enable_tool":
      return String(input.name ?? "");
    default:
      return "";
  }
}
```

#### 3d: Input summary in `ToolPanel` tool rows

Show the input summary inline with the tool name in the expanded panel:

```svelte
<span class="tool-label">
  <span class="tool-name">{humanize(tool.name)}</span>
  {#if getInputSummary(tool.name, tool.input)}
    <span class="tool-input-summary">{getInputSummary(tool.name, tool.input)}</span>
  {:else if tool.name !== humanize(tool.name)}
    <span class="tool-raw">{tool.name}</span>
  {/if}
</span>
```

Style the input summary distinctly — it's the most useful piece of information:

```css
.tool-input-summary {
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
```

#### 3e: Full input display in expanded tool detail

When a tool row is expanded, show the full input before the result:

```svelte
{#if expandedIds.has(tool.id)}
  <div class="tool-detail">
    {#if tool.input && Object.keys(tool.input).length > 0}
      <div class="tool-input-block">
        <span class="detail-label">Input</span>
        <pre class="tool-input">{@html highlightCode(JSON.stringify(tool.input, null, 2), "json")}</pre>
      </div>
    {/if}
    {#if tool.result}
      <div class="tool-result-block">
        <span class="detail-label">Output</span>
        <pre class="tool-result" class:collapsed={isCollapsed(tool)}>{@html highlightResult(tool)}</pre>
        {#if isCollapsible(tool)}
          <button class="collapse-toggle" onclick={() => toggleCollapse(tool.id)}>
            {isCollapsed(tool) ? `Show all ${resultLineCount(tool.result)} lines` : "Show less"}
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}
```

```css
.detail-label {
  display: block;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  margin-bottom: 3px;
}

.tool-input-block,
.tool-result-block {
  margin-bottom: 8px;
}

.tool-input {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-secondary);
  background: var(--surface);
  border-radius: var(--radius-sm);
  padding: 6px 8px;
  border-left: 2px solid var(--accent);
}
```

### Phase 4: Tool Categorization & Grouping

**Goal:** Group tool calls by logical category so 28 tools aren't an undifferentiated list.

#### Category definitions

```typescript
type ToolCategory = "filesystem" | "search" | "execute" | "memory" | "communication" | "system";

const TOOL_CATEGORIES: Record<string, ToolCategory> = {
  read: "filesystem",
  write: "filesystem",
  edit: "filesystem",
  ls: "filesystem",
  find: "search",
  grep: "search",
  web_search: "search",
  web_fetch: "search",
  exec: "execute",
  mem0_search: "memory",
  blackboard: "memory",
  note: "memory",
  sessions_send: "communication",
  sessions_ask: "communication",
  sessions_spawn: "communication",
  message: "communication",
  voice_reply: "communication",
  enable_tool: "system",
};

const CATEGORY_LABELS: Record<ToolCategory, string> = {
  filesystem: "Files",
  search: "Search",
  execute: "Commands",
  memory: "Memory",
  communication: "Agents",
  system: "System",
};

const CATEGORY_ICONS: Record<ToolCategory, string> = {
  filesystem: "📁",
  search: "🔍",
  execute: "⚡",
  memory: "🧠",
  communication: "💬",
  system: "⚙️",
};
```

#### Tool panel header stats by category

Replace or augment the flat "✓ 12 ✕ 1" stats with category breakdown:

```svelte
<div class="header-stats">
  {#each categoryGroups as [category, count]}
    <span class="stat cat" title={CATEGORY_LABELS[category]}>
      {CATEGORY_ICONS[category]} {count}
    </span>
  {/each}
  {#if errors > 0}
    <span class="stat err">✕ {errors}</span>
  {/if}
  <span class="stat time">{formatDuration(totalDuration)}</span>
</div>
```

#### Optional: grouped view toggle

Add a toggle in the panel header: **Sequential** (current) vs **Grouped** (by category). In grouped view, tool calls are organized under category headers:

```
📁 Files (8)
  1. Read file    /mnt/ssd/aletheia/ui/src/...   ✓  0.1s
  2. Edit file    /mnt/ssd/aletheia/ui/src/...   ✓  0.2s
  ...

⚡ Commands (4)
  9. Run command  npm run build                   ✓  3.2s
  ...

🔍 Search (3)
  13. Search files  /tool_start/ in *.ts          ✓  0.8s
  ...
```

This is a view mode, not a data transformation — the underlying data stays sequential. The grouped view just rearranges presentation.

### Phase 5: Tool Status Line Enhancement

**Goal:** The collapsed `ToolStatusLine` pill in the message shows more at-a-glance value.

Current: `✓ Running command   3/28 ›`
Better:  `⚡ git status        📁 8 🔍 3 ⚡ 4   3/28 ›`

When actively running, show the current tool with its input summary. When complete, show the category breakdown instead of just a count.

```typescript
let statusText = $derived.by(() => {
  if (running.length > 0) {
    const current = running[running.length - 1]!;
    const summary = getInputSummary(current.name, current.input);
    const label = humanizeTool(current.name);
    return summary ? `${label}: ${summary}` : label;
  }
  // When complete, show category summary
  return categoryGroups.map(([cat, count]) => `${CATEGORY_ICONS[cat]}${count}`).join(" ");
});
```

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** ✅ | Thinking panel persistence across turn completion | Small | High — eliminates jarring panel close |
| **2** ✅ | Thinking panel Markdown rendering | Small | Medium — makes thinking content actually readable |
| **3** ✅ | Tool input display (event stream → UI) | Medium | High — the single biggest tool panel improvement |
| **4** | Tool categorization & grouping | Medium | Medium — scales tool-heavy turns |
| **5** | Tool status line enhancement | Small | Medium — better at-a-glance value |

---

## Testing

- **Thinking persistence:** Open thinking panel during streaming. Wait for turn to complete. Verify panel stays open with final thinking content (no flash, no close).
- **Thinking persistence (click after completion):** Complete a turn with thinking. Click the thinking pill on the finalized message. Verify panel opens with full thinking content.
- **Thinking Markdown:** Send a message that triggers thinking with markdown content (headers, lists, code blocks). Verify the thinking panel renders formatted output, not raw `<pre>`.
- **Thinking Markdown streaming:** During live streaming, verify incremental Markdown rendering doesn't cause layout thrashing or parsing errors on incomplete markdown.
- **Tool input in stream:** Trigger a tool call. Verify the `tool_start` SSE event includes the `input` field.
- **Tool input in panel:** Open the tool panel. Verify each tool row shows an input summary (command for exec, path for read, pattern for grep, etc.).
- **Tool input expanded:** Expand a tool row in the panel. Verify the full input JSON is displayed above the output, with syntax highlighting.
- **Tool input from history:** Reload a session with tool calls from history. Verify input data is parsed from the stored `tool_use` content blocks and displayed correctly.
- **Tool categories:** Trigger 10+ tool calls of different types. Open the tool panel. Verify category stats in the header (📁 5 🔍 3 ⚡ 2).
- **Grouped view:** Toggle to grouped view. Verify tools are organized under category headers with correct counts.
- **Status line with input:** During streaming, verify the ToolStatusLine shows the current tool's input summary (e.g., "Running command: `git status`").

---

## Success Criteria

- Thinking panel never closes unexpectedly during a turn transition
- Thinking content is formatted (headers, lists, code blocks render properly)
- Every tool call shows its primary input without expanding (command, path, pattern, target agent)
- Tool-heavy turns (20+ calls) are scannable via category grouping
- A user can understand what the agent did from the tool panel alone, without reading the agent's text output
