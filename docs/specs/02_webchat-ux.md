# Spec: Webchat UX — Notifications, Resilience, and File Editor

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problems

### 1. No cross-agent notifications

When chatting with Demiurge and Syn sends a message (or vice versa), there's no notification. You have to manually switch agents to check. In a multi-agent workspace, this means missing time-sensitive messages or constantly tab-switching to poll.

### 2. Refresh kills work, staleness requires refresh

The webchat has a broken relationship with browser refresh:

**Staleness:** Messages sent from Signal or another transport don't appear in the webchat until you refresh. The global SSE event stream (`/api/events`) is referenced in client code but **has no server endpoint** — the `events.ts` client connects to a URL that doesn't exist. The `turn:after` event that triggers history reload never fires because the EventSource never connects. The only way to see new messages is a full page refresh.

**Refresh kills work:** When you refresh the page, the browser aborts the active SSE stream (`/api/sessions/stream`). The server detects the abort and cancels the in-flight turn (via `abortSignal`). This means refreshing while an agent is mid-tool-loop kills their work. The agent's partial state is lost — they have to start over.

These two problems compound: the UI goes stale → you refresh to fix it → the refresh kills active work.

### 3. Tool output line splitting

The first few lines of streaming text that follow tool calls get split strangely. This is because the `needsTextSeparator` logic in `chat.svelte.ts` inserts `\n\n` between tool results and subsequent text, but the streaming text arrives character-by-character. The separator fires on the first `text_delta` after a `tool_result`, splitting mid-word or mid-line. The visual result is a broken first line.

### 4. File explorer is a dead-end tab

The files tab replaces the chat view entirely. You can't see files and chat simultaneously. The tree is always visible (wastes space when you're working in a file). There's no editing — it's read-only. For a system where the agents *and* the human both work with files, this is a critical gap. You should be able to:

- See files alongside the chat
- Edit files directly (markdown, YAML, JSON, TypeScript, etc.)
- Render markdown in formatted view
- Save changes (with git status awareness)

---

## Design

### Part 1: Cross-Agent Notifications

#### Server: Global SSE endpoint

**File:** `infrastructure/runtime/src/pylon/server.ts`

Add `GET /api/events` — a persistent SSE connection that broadcasts system-wide events:

```typescript
app.get("/api/events", (c) => {
  const encoder = new TextEncoder();
  let closed = false;

  const stream = new ReadableStream({
    start(controller) {
      // Send init with active turns
      const activeTurns = manager.getActiveTurns();
      const initPayload = `event: init\ndata: ${JSON.stringify({ activeTurns })}\n\n`;
      controller.enqueue(encoder.encode(initPayload));

      // Subscribe to event bus
      const handlers = {
        "turn:before": (data: unknown) => {
          if (!closed) controller.enqueue(encoder.encode(`event: turn:before\ndata: ${JSON.stringify(data)}\n\n`));
        },
        "turn:after": (data: unknown) => {
          if (!closed) controller.enqueue(encoder.encode(`event: turn:after\ndata: ${JSON.stringify(data)}\n\n`));
        },
        "cross-agent:message": (data: unknown) => {
          if (!closed) controller.enqueue(encoder.encode(`event: cross-agent:message\ndata: ${JSON.stringify(data)}\n\n`));
        },
      };

      for (const [event, handler] of Object.entries(handlers)) {
        eventBus.on(event, handler);
      }

      // Keepalive ping every 15s
      const pingInterval = setInterval(() => {
        if (!closed) {
          controller.enqueue(encoder.encode(`: ping\n\n`));
        }
      }, 15_000);

      // Cleanup on disconnect
      c.req.raw.signal.addEventListener("abort", () => {
        closed = true;
        clearInterval(pingInterval);
        for (const [event, handler] of Object.entries(handlers)) {
          eventBus.off(event, handler);
        }
        controller.close();
      });
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Connection": "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
});
```

This endpoint is the backbone. It fixes staleness (client gets real-time `turn:after` events) and enables notifications (client gets `cross-agent:message` events).

#### Client: Notification system

**File:** `ui/src/stores/notifications.svelte.ts`

```typescript
interface Notification {
  id: string;
  agentId: string;
  agentName: string;
  preview: string;
  timestamp: string;
  read: boolean;
}

let notifications = $state<Notification[]>([]);
let unreadCount = $state(0);
```

**When a `turn:after` event fires for an agent that isn't currently active:** create a notification with a preview of the response text. Show it as:

1. **Badge on the agent's sidebar entry** — a small unread count dot/number, like messaging apps
2. **Toast notification** — brief popup in the bottom-right: "🌀 Syn: *reviewing PR #35...*" that auto-dismisses after 5 seconds
3. **Browser notification** (optional, with permission) — for when the tab is in the background

Clicking the toast or the sidebar badge switches to that agent and marks the notification as read.

**File:** `ui/src/components/layout/Sidebar.svelte` — Add unread indicator per agent:

```svelte
{#if getUnreadCount(agent.id) > 0}
  <span class="unread-badge">{getUnreadCount(agent.id)}</span>
{/if}
```

### Part 2: Refresh Resilience

#### Problem: Refresh aborts work

The `/api/sessions/stream` endpoint receives messages and streams responses. When the browser disconnects (refresh/close), the server abort signal fires, canceling the turn. This is correct behavior for user-initiated stops but wrong for accidental refreshes.

**Fix: Decouple message submission from stream consumption.**

Split into two operations:

1. **Submit message** — `POST /api/sessions/send` (already exists) — fire-and-forget. The server processes the turn regardless of whether the client is connected.
2. **Attach to stream** — `GET /api/sessions/:id/stream` — connect to an active turn's output stream. If the turn is already in progress, catch up from the beginning. If the client disconnects and reconnects, it resumes from where it left off.

The key insight: **the turn's lifecycle is server-owned, not client-owned.** The client submitting a message starts the turn. The client watching the stream is just observation. Closing the observation stream should not cancel the turn.

**Implementation:**

```typescript
// POST /api/sessions/send — submit message, return immediately
app.post("/api/sessions/send", async (c) => {
  const { agentId, message, sessionKey, media } = await c.req.json();
  // Validate, resolve session, start turn in background
  const turnId = manager.startTurn({ text: message, nousId: agentId, sessionKey, media });
  return c.json({ turnId, sessionId: resolvedSession.id });
});

// GET /api/sessions/:sessionId/stream?turnId=X — attach to turn output
app.get("/api/sessions/:sessionId/stream", (c) => {
  const turnId = c.req.query("turnId");
  // If turn is active, stream events from buffer + live
  // If turn completed, stream from buffer only (catch-up)
  // If turn not found, 404
});
```

**Turn buffer:** The manager buffers all events for active turns (bounded, e.g. last 1000 events). When a client attaches, it replays the buffer then switches to live streaming. When the turn completes, the buffer is retained for 60 seconds for late-arriving clients, then discarded.

This means:
- Refreshing during a turn → page reloads → client reattaches to the same turn → sees all buffered events → resumes live streaming. **No work lost.**
- Closing the tab → turn continues server-side → next time you open webchat, the completed response is in history.

**Existing `/api/sessions/stream` (POST)** can be kept as a convenience endpoint that combines send + attach in one request, but with the abort signal only detaching from the stream, not canceling the turn. Add an explicit cancel via `POST /api/sessions/:turnId/cancel`.

#### Problem: Staleness

Fixed by Part 1's global SSE endpoint. When `turn:after` fires, the client reloads history for the affected agent. This already works in the client code (`ChatView.svelte` lines 56-80) — it just needs the server endpoint to exist.

**Additional fix: Periodic history poll as fallback.** If the SSE connection drops (network blip, server restart), the client polls history every 30 seconds until SSE reconnects. This ensures messages always appear within 30 seconds even if the real-time channel is down.

```typescript
// In ChatView.svelte onMount
let pollInterval: number | null = null;

onGlobalEvent((event) => {
  if (event === "connection") {
    const { status } = data as { status: string };
    if (status === "disconnected" && !pollInterval) {
      // Fallback: poll history every 30s while SSE is down
      pollInterval = setInterval(() => {
        const agentId = getActiveAgentId();
        const sessionId = getActiveSessionId();
        if (agentId && sessionId) loadHistory(agentId, sessionId);
      }, 30_000);
    } else if (status === "connected" && pollInterval) {
      clearInterval(pollInterval);
      pollInterval = null;
      // Immediate reload on reconnect
      const agentId = getActiveAgentId();
      const sessionId = getActiveSessionId();
      if (agentId && sessionId) loadHistory(agentId, sessionId);
    }
  }
});
```

### Part 3: Tool Output Line Splitting Fix

**File:** `ui/src/stores/chat.svelte.ts`

The current code:

```typescript
case "text_delta":
  if (needsTextSeparator && state.streamingText) {
    state.streamingText += "\n\n";
    needsTextSeparator = false;
  }
  state.streamingText += event.text;
  break;
```

The problem: `needsTextSeparator` is set to true after every `tool_result`. The next `text_delta` inserts `\n\n` — but if the LLM's first text after tools starts with a newline, you get triple-newline. And if the text is a continuation of a previous block, the separator splits a logical paragraph.

**Fix:** Don't insert a separator on every tool→text transition. Instead, only separate when the streaming text *already has content* and the new text block represents a genuinely new response segment. The server already sends `text_delta` events that naturally break at block boundaries. Let the markdown renderer handle paragraph spacing.

```typescript
case "text_delta":
  state.streamingText += event.text;
  break;

// Remove needsTextSeparator entirely
```

If there's a genuine need to visually separate post-tool text from pre-tool text, handle it in the renderer: detect tool call markers in the accumulated text and insert spacing at render time, not at accumulation time.

### Part 4: File Editor with Side-by-Side Layout

#### Layout: Split pane, not tab replacement

The files view opens as a **side panel alongside the chat**, not as a tab that replaces it. The layout becomes:

```
┌──────────┬────────────────────────┬──────────────────┐
│ Sidebar  │     Chat View          │   File Panel     │
│ (agents) │                        │                  │
│          │                        │ [tree] [editor]  │
│          │                        │                  │
└──────────┴────────────────────────┴──────────────────┘
                    ← drag handle →
```

- The file panel slides in from the right when activated (keyboard shortcut: `Cmd+B` or toolbar button)
- A draggable divider between chat and file panel lets you control the split ratio
- The panel remembers its width in localStorage
- Default split: 60% chat / 40% files
- The file tree inside the panel is collapsible (toggle button) so you can maximize editor space
- Mobile: file panel goes full-screen as an overlay (same as current tab behavior)

**Implementation:**

**File:** `ui/src/components/layout/Layout.svelte`

Replace the tab-based view switching with a persistent split:

```svelte
<div class="main">
  <Sidebar collapsed={sidebarCollapsed} onAgentSelect={closeSidebar} />
  <div class="content-area" style="--file-panel-width: {filePanelOpen ? filePanelWidth : 0}px">
    <div class="chat-pane">
      <ChatView />
    </div>
    {#if filePanelOpen}
      <div
        class="resize-handle"
        onmousedown={startResize}
        role="separator"
        aria-orientation="vertical"
      ></div>
      <div class="file-pane" style="width: {filePanelWidth}px">
        <FileEditor />
      </div>
    {/if}
  </div>
</div>
```

The resize handle uses a mousedown → mousemove → mouseup pattern (standard split pane behavior). The width is persisted to localStorage.

#### File tree: Collapsible sidebar within the panel

The file tree becomes a collapsible panel inside the file pane:

```
┌──────────────────────┐      ┌──────────────────────┐
│ [≡] Files    [×]     │      │ [≡]  MEMORY.md  [×]  │
│ ├── memory/          │  →   │                       │
│ │   ├── 2026-02-19   │      │  # Memory             │
│ │   └── ref-*.md     │      │  ...content...        │
│ ├── MEMORY.md ←      │      │                       │
│ └── AGENTS.md        │      │                       │
│ ────────────────     │      │                       │
│ [editor area]        │      │                       │
└──────────────────────┘      └──────────────────────┘
  Tree expanded                  Tree collapsed
```

The `[≡]` button toggles the tree. When collapsed, the file name shows in the header bar so you know what's open.

#### Editor: Native editing with language support

**New component:** `ui/src/components/files/FileEditor.svelte`

The editor replaces the read-only `<pre>` preview with an actual code editor. Two options for the implementation:

**Option A: CodeMirror 6** — Full-featured, extensible, excellent TypeScript/markdown support, ~150KB gzipped. Used by Replit, Gitea, Observable. Supports:
- Syntax highlighting for all our file types (TS, JS, Svelte, JSON, YAML, Markdown, Python, SQL, Shell)
- Line numbers, bracket matching, auto-indent
- Search/replace (Cmd+F)
- Multiple cursors
- Vim/Emacs keybindings (optional)
- Custom themes (matches our dark UI)
- Markdown preview mode via plugin

**Option B: Monaco Editor** — VSCode's editor. More powerful but much heavier (~2MB). Overkill for our use case.

**Recommendation: CodeMirror 6.** It's the right balance of capability and size. We need good editing, not a full IDE.

```svelte
<script lang="ts">
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { markdown } from "@codemirror/lang-markdown";
  import { javascript } from "@codemirror/lang-javascript";
  // ... other language imports

  let editorContainer: HTMLDivElement;
  let view: EditorView;
  let isDirty = $state(false);
  let isMarkdownPreview = $state(false);

  function initEditor(content: string, path: string) {
    const lang = resolveLanguage(path);
    const state = EditorState.create({
      doc: content,
      extensions: [
        basicSetup,
        lang,
        darkTheme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) isDirty = true;
        }),
        // Cmd+S to save
        keymap.of([{
          key: "Mod-s",
          run: () => { save(); return true; },
        }]),
      ],
    });
    view = new EditorView({ state, parent: editorContainer });
  }

  async function save() {
    const content = view.state.doc.toString();
    await saveWorkspaceFile(selectedPath, content, activeAgentId);
    isDirty = false;
  }
</script>

<div class="editor-header">
  <span class="file-path">{selectedPath}</span>
  {#if isDirty}
    <span class="dirty-indicator">●</span>
  {/if}
  {#if isMarkdownFile(selectedPath)}
    <button class="preview-toggle" onclick={() => isMarkdownPreview = !isMarkdownPreview}>
      {isMarkdownPreview ? "Edit" : "Preview"}
    </button>
  {/if}
  <button class="save-btn" onclick={save} disabled={!isDirty}>
    Save {#if isDirty}(Cmd+S){/if}
  </button>
</div>

{#if isMarkdownPreview}
  <div class="markdown-preview">
    {@html renderMarkdown(view.state.doc.toString())}
  </div>
{:else}
  <div class="editor-container" bind:this={editorContainer}></div>
{/if}
```

#### Markdown preview

For `.md` files, a toggle button switches between edit mode (CodeMirror) and preview mode (rendered markdown using the same `Markdown.svelte` component the chat already uses). The preview is live — it re-renders from the editor content, not from the saved file, so you see your changes as you make them.

#### Server: File save endpoint

**File:** `infrastructure/runtime/src/pylon/server.ts`

```typescript
app.put("/api/workspace/file", async (c) => {
  const { path, content, agentId } = await c.req.json();
  if (!path || typeof content !== "string") {
    return c.json({ error: "path and content required" }, 400);
  }

  const workspace = resolveAgentWorkspace(agentId ?? undefined);
  if (!workspace) return c.json({ error: "No workspace configured" }, 400);

  const resolved = safeWorkspacePath(workspace, path);
  if (!resolved) return c.json({ error: "Invalid path" }, 400);

  try {
    writeFileSync(resolved, content, "utf-8");
    return c.json({ ok: true, path, size: Buffer.byteLength(content) });
  } catch (err) {
    return c.json({ error: err instanceof Error ? err.message : "Write failed" }, 500);
  }
});
```

**Security:** Uses the same `safeWorkspacePath` as the read endpoint — prevents path traversal. Respects `allowedRoots` from the agent config for broader access.

#### Unsaved changes guard

If you have unsaved changes and try to close the file panel, switch files, or navigate away, show a confirmation: "You have unsaved changes. Save before closing?"

Use `beforeunload` for page-level navigation and an in-app guard for file switching:

```typescript
window.addEventListener("beforeunload", (e) => {
  if (isDirty) {
    e.preventDefault();
    e.returnValue = "";
  }
});
```

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1a** | Global SSE endpoint (`/api/events`) | Medium | Fixes staleness, enables all real-time features |
| **1b** | Cross-agent notification badges + toasts | Small | Immediately useful for multi-agent work |
| **1c** | History poll fallback (SSE disconnect) | Small | Resilience for network blips |
| **2a** | Decouple send from stream (turn buffer) | Medium | Fixes refresh-kills-work |
| **2b** | Client-side stream reattach on refresh | Small | Depends on 2a |
| **3** | Tool output line split fix | Tiny | Pure client-side fix |
| **4a** | Split pane layout with draggable divider | Medium | Layout restructuring |
| **4b** | Collapsible file tree | Small | Space efficiency |
| **4c** | CodeMirror editor integration | Medium | Core editing capability |
| **4d** | File save endpoint + Cmd+S | Small | Completes edit cycle |
| **4e** | Markdown preview toggle | Small | Quality-of-life |

**Recommended:** 1a → 3 → 1b → 1c → 2a → 2b → 4a → 4c → 4d → 4b → 4e

Phase 1a is the keystone — the global SSE endpoint fixes staleness and unblocks notifications. Phase 3 is a one-line fix. Phase 2a is the biggest architectural change but has the highest impact on daily usability.

---

## Testing

### Notifications
- Send message via Signal to agent A. Verify webchat (viewing agent B) shows notification badge on A's sidebar entry and toast popup.
- Click notification → switches to agent A, marks as read.
- Multiple unread messages → badge shows count.

### Refresh resilience
- Start a long tool loop (e.g. "review all PRs"). Mid-loop, refresh the page. Verify the agent continues working. Verify the refreshed page shows the in-progress turn and streams remaining output.
- Close the tab entirely during a tool loop. Reopen webchat. Verify the completed response appears in history.

### Staleness
- Send a message via Signal. Without refreshing webchat, verify the message appears within 5 seconds (via SSE `turn:after` → history reload).
- Kill the SSE connection (network tab). Verify fallback polling kicks in within 30 seconds.

### Tool output
- Trigger a multi-tool turn (e.g. "check git status and list files"). Verify the text after tool calls renders cleanly without split first lines.

### File editor
- Open file panel alongside chat. Drag divider to resize. Verify both panes are functional.
- Open a TypeScript file. Verify syntax highlighting. Edit content, verify dirty indicator appears. Cmd+S to save. Verify file is saved on disk.
- Open a markdown file. Toggle to preview mode. Verify rendered markdown. Edit in edit mode, switch to preview, verify changes are reflected.
- Open a file with unsaved changes, try to switch to another file. Verify confirmation dialog.
- Collapse the file tree. Verify editor gets full width. Expand tree. Verify tree reappears.

---

## Success Criteria

- **Notifications:** Never miss a cross-agent message while viewing a different agent. Badge clears on view.
- **No staleness:** Messages from any transport appear in webchat within 5 seconds without manual refresh.
- **Refresh-safe:** Refreshing the page during a tool loop doesn't cancel the agent's work. The response appears after refresh.
- **Clean tool output:** No split first lines after tool calls.
- **File editing:** Can open, edit, save, and preview files alongside the chat. Markdown renders. Unsaved changes are guarded.
