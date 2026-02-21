# Spec 18: Integrated IDE

Lightweight file editor embedded in the Aletheia web UI, enabling humans and agents to work on the same files without context-switching to an external IDE.

## Motivation

Spec 17 identified "IDE integration" as a gap across all eight compared systems. Most solve this with LSP, ACP, or VS Code extensions — bolted-on bridges to external tooling. Aletheia already has the scaffolding for a native solution: CodeMirror 6 with full language support, a file tree with git status, workspace API endpoints for read/write, and a togglable editor panel in the layout. This spec extends those pieces into a usable embedded editor and wires it into the agent workflow so users see agent file changes in real-time.

**Design constraint:** This is not VS Code. No LSP, no debugger, no terminal emulator. The goal is "good enough to not tab away" for reviewing agent work, making quick edits, and watching agents modify files live.

## Current State

| Component | Status | Location |
|-----------|--------|----------|
| CodeMirror 6 editor | **Working** — writable, save, dirty tracking, Cmd+S | `ui/src/components/files/FileEditor.svelte` |
| Language support | js/ts/tsx/jsx/py/json/yaml/md/css/html/svelte | `ui/package.json` (@codemirror/lang-*) |
| File tree (explorer) | **Working** — read-only preview, git status dots, filter | `ui/src/components/files/FileExplorer.svelte` |
| File tree (editor) | **Working** — integrated tree in editor panel | `FileEditor.svelte:206-234` |
| Workspace API: tree | GET `/api/workspace/tree` (depth-limited, sorted) | `server.ts:1350` |
| Workspace API: read | GET `/api/workspace/file` (1MB limit) | `server.ts:1365` |
| Workspace API: write | PUT `/api/workspace/file` (creates parent dirs) | `server.ts:1389` |
| Workspace API: git | GET `/api/workspace/git-status` (porcelain) | `server.ts:1421` |
| SSE stream | tool_start, tool_result, text_delta, etc. | `ui/src/lib/stream.ts` |
| Layout toggle | File panel with resize handle, localStorage width | `ui/src/components/layout/Layout.svelte:57-114` |
| File store | Svelte 5 runes, lazy tree, git status cache | `ui/src/stores/files.svelte.ts` |
| Markdown preview | Basic inline preview toggle | `FileEditor.svelte:111-132` |
| Path traversal protection | `safeWorkspacePath()` rejects `..` escapes | `server.ts:1296` |
| Git auto-commit | Agent writes/edits trigger `commitWorkspaceChange()` | `organon/built-in/workspace-git.ts` |

### What's Missing

1. **Multi-tab** — currently single-file; opening a new file replaces the old one
2. **Agent edit notifications** — no toast when an agent modifies a file via `write`/`edit` tool
3. **File diffing** — no way to see what an agent changed vs. your local state
4. **File creation/deletion/rename** — no UI for these operations (API write exists, delete/rename don't)
5. **Clickable file paths in chat** — agent mentions `src/foo.ts` in response but it's plain text
6. **Workspace search** — no cross-file content search from UI
7. **Large file warning** — editor loads anything under 1MB without warning

## Architecture

### Layout States

```
┌─────────────────────────────────────────────────┐
│ TopBar                              [Files] [≡] │
├───────┬─────────────────────────────────────────┤
│       │                                         │
│  S    │              Chat                       │
│  i    │                                         │
│  d    │                                         │
│  e    │                                         │
│  b    │                                         │
│  a    │                                         │
│  r    │                                         │
│       │                                         │
└───────┴─────────────────────────────────────────┘
        State 1: Chat only (default — current behavior)
```

```
┌─────────────────────────────────────────────────┐
│ TopBar                              [Files] [≡] │
├───────┬──────────────────┬──┬───────────────────┤
│       │ [tree] │ tabs ▾  │  │                   │
│  S    │ ───────┤─────────│◂▸│     Chat          │
│  i    │ src/   │ editor  │  │                   │
│  d    │  foo.ts│         │  │                   │
│  e    │  bar.ts│         │  │                   │
│  b    │ docs/  │         │  │                   │
│  a    │        │         │  │                   │
│  r    │        │         │  │                   │
│       │        │         │  │                   │
└───────┴────────┴─────────┴──┴───────────────────┘
        State 2: Chat + Editor (toggle on)
```

**Toggle:** `[Files]` button in TopBar or `Ctrl+E` / `Cmd+E`. State persisted in localStorage. This is the existing behavior — just needs the tab bar and agent integration wired in.

### Data Flow: Agent File Edits

```
Agent calls write/edit tool
  → server executes + auto-commits
  → SSE tool_result event streamed to UI
  → UI filters for toolName: "write" | "edit"
  → Toast: "Syn edited config.ts" [Open]
  → If file already in a tab → mark tab stale + show refresh prompt
  → User clicks Open/Refresh → loads updated content from API
```

## Implementation

### Phase 1: Multi-Tab Editor

**Goal:** Support multiple open files with a tab bar.

#### 1.1 Tab state in file store

File: `ui/src/stores/files.svelte.ts`

```typescript
interface EditorTab {
  path: string;
  name: string;
  dirty: boolean;
  stale: boolean;  // true when agent modified the file externally
}

let openTabs = $state<EditorTab[]>([]);
let activeTabPath = $state<string | null>(null);

export function getOpenTabs(): EditorTab[] { return openTabs; }
export function getActiveTabPath(): string | null { return activeTabPath; }

export function openTab(path: string): void {
  if (!openTabs.find(t => t.path === path)) {
    const name = path.split("/").pop() ?? path;
    openTabs = [...openTabs, { path, name, dirty: false, stale: false }];
  }
  activeTabPath = path;
}

export function closeTab(path: string): void {
  openTabs = openTabs.filter(t => t.path !== path);
  if (activeTabPath === path) {
    activeTabPath = openTabs.at(-1)?.path ?? null;
  }
}

export function markTabDirty(path: string, dirty: boolean): void {
  openTabs = openTabs.map(t => t.path === path ? { ...t, dirty } : t);
}

export function markTabStale(path: string): void {
  openTabs = openTabs.map(t => t.path === path ? { ...t, stale: true } : t);
}
```

~40 LOC added to existing store.

#### 1.2 Tab bar component

New file: `ui/src/components/files/EditorTabs.svelte`

Horizontal tab bar rendered above the CodeMirror editor area. Each tab shows:
- File name (truncated if needed)
- Yellow dot if dirty (unsaved local changes)
- Blue dot if stale (agent edited externally)
- Close button (×) — confirms if dirty

Middle-click or close button closes tab. Click activates tab. Active tab has accent-colored bottom border. Overflow scrolls horizontally.

~80 LOC.

#### 1.3 Wire tabs into FileEditor

File: `ui/src/components/files/FileEditor.svelte`

- Replace single `currentPath` with `activeTabPath` from store
- `openFile()` calls `openTab()` instead of replacing `currentPath`
- Tab switch preserves editor state per tab (content cached in a `Map<string, string>`)
- Save updates `markTabDirty(path, false)`
- `beforeunload` handler checks any tab dirty

~30 LOC modified.

### Phase 2: Agent Edit Notifications

**Goal:** Users see when agents modify files, with one-click access.

#### 2.1 Filter SSE tool_result events

File: `ui/src/lib/stream.ts` or consuming component

The existing `tool_result` event includes `toolName` and `result`. When `toolName` is `"write"` or `"edit"`, the `tool_start` event's `input` field contains the file path. Capture `tool_start` events with matching `toolId` to extract the path.

Track pending tool calls:
```typescript
// In ChatView or a dedicated hook
if (event.type === "tool_start" && (event.toolName === "write" || event.toolName === "edit")) {
  pendingFileTools.set(event.toolId, event.input?.path as string);
}
if (event.type === "tool_result" && pendingFileTools.has(event.toolId)) {
  const path = pendingFileTools.get(event.toolId)!;
  pendingFileTools.delete(event.toolId);
  if (!event.isError) {
    notifyFileEdit(agentName, path);
  }
}
```

~20 LOC.

#### 2.2 Toast notification

File: `ui/src/components/shared/Toast.svelte` (exists)

Show toast: **"Syn edited `config.ts`"** with an **[Open]** action button.

Clicking Open:
1. Opens the file in a new tab (or activates existing tab)
2. Toggles editor panel open if hidden
3. If tab already open, marks it stale

Toast auto-dismisses after 5s. Multiple edits in quick succession batch into one toast ("Syn edited 3 files" with [Open All]).

~40 LOC.

#### 2.3 Stale tab refresh

When a tab is marked stale (blue dot), clicking the dot or a toolbar button reloads the file from the API. The editor content is replaced with the server version.

If the tab is also dirty (local unsaved changes + agent external edit), show a conflict prompt: "Agent modified this file. Reload server version or keep yours?" Two buttons: [Reload] [Keep Mine].

~30 LOC.

### Phase 3: File Operations

**Goal:** Create, delete, and rename files from the UI.

#### 3.1 Context menu

Right-click on file tree items shows a context menu:
- **New File** (on directory or empty space)
- **New Folder** (on directory or empty space)
- **Rename** (on file or directory)
- **Delete** (on file or directory, with confirmation)

New component: `ui/src/components/files/TreeContextMenu.svelte` — ~60 LOC.

#### 3.2 New API endpoints

File: `infrastructure/runtime/src/pylon/server.ts`

**DELETE** `/api/workspace/file`
```
Query: path, agentId
Validates with safeWorkspacePath(). Calls unlinkSync().
Returns { ok: true } or 404/400.
```

**POST** `/api/workspace/file/move`
```
Body: { from, to, agentId }
Validates both paths with safeWorkspacePath().
Calls renameSync(). Creates parent dirs if needed.
Returns { ok: true, from, to }.
```

Both endpoints follow existing patterns. ~35 LOC total in server.ts.

#### 3.3 Inline rename

When user clicks Rename, the tree item switches to an inline `<input>` with the current name selected. Enter confirms (calls move API), Escape cancels. ~30 LOC in FileEditor.svelte tree rendering.

#### 3.4 API client additions

File: `ui/src/lib/api.ts`

```typescript
export async function deleteWorkspaceFile(path: string, agentId?: string): Promise<void>
export async function moveWorkspaceFile(from: string, to: string, agentId?: string): Promise<{ok: boolean}>
```

~20 LOC.

### Phase 4: Clickable File Paths in Chat

**Goal:** When an agent mentions a file path in its response, make it clickable.

File: `ui/src/components/chat/MessageBubble.svelte` (or wherever messages are rendered)

Post-process rendered message HTML to detect file path patterns:
- Paths starting with known prefixes (e.g., `src/`, `docs/`, `infrastructure/`)
- Paths with recognized extensions (`.ts`, `.js`, `.py`, `.json`, `.md`, `.svelte`, etc.)
- Backtick-wrapped paths (`` `src/foo.ts` ``)

Replace with clickable `<a>` elements that call `openTab(path)` and toggle the editor panel open.

Regex pattern (approximate):
```
(?:`([a-zA-Z0-9_\-./]+\.[a-z]{1,5})`)|(?:(?:^|\s)((?:src|docs|infrastructure|ui|shared)/[a-zA-Z0-9_\-./]+\.[a-z]{1,5}))
```

~40 LOC.

### Phase 5: Workspace Search (stretch)

**Goal:** Find text across files without leaving the UI.

#### 5.1 Search API endpoint

File: `infrastructure/runtime/src/pylon/server.ts`

**GET** `/api/workspace/search`
```
Query: q (search term), agentId, glob (optional file pattern), maxResults (default 50)
Uses child_process to run grep -rn or ripgrep if available.
Returns array of { path, line, lineNumber, matchStart, matchEnd }.
```

Falls back to recursive `readFileSync` + string match if no grep. Respects `safeWorkspacePath()`. Skips binary files and `node_modules`/`.git`.

~60 LOC server-side.

#### 5.2 Search UI

Search input in the file tree sidebar header. Results replace the tree temporarily, showing file paths with line previews. Click a result opens the file at that line.

~60 LOC.

## New API Endpoints Summary

| Method | Path | Purpose | Phase |
|--------|------|---------|-------|
| GET | `/api/workspace/tree` | List files (exists) | — |
| GET | `/api/workspace/file` | Read file (exists) | — |
| PUT | `/api/workspace/file` | Write file (exists) | — |
| GET | `/api/workspace/git-status` | Git status (exists) | — |
| DELETE | `/api/workspace/file` | Delete file | 3 |
| POST | `/api/workspace/file/move` | Rename/move | 3 |
| GET | `/api/workspace/search` | Content search | 5 |

All endpoints use `safeWorkspacePath()`. Write/delete/move operations auto-commit via the existing git tracking in `workspace-git.ts` (or should be wired to do so).

## New Dependencies

| Package | Purpose | Phase | Size |
|---------|---------|-------|------|
| `@codemirror/merge` | Inline diff for conflict resolution | 2 (optional) | ~15KB |

Everything else is already installed. The merge extension is optional — Phase 2 can ship with simple reload-or-keep without diff visualization.

## Security

- **Path traversal**: Handled by existing `safeWorkspacePath()` in all endpoints
- **File size**: 1MB read limit enforced server-side; editor should show a warning banner for files > 500KB
- **Concurrent edits**: Agent and human can both write to the same file. Phase 2 handles this with stale detection and reload/keep prompt. No real-time collaboration (no CRDT) — last-write-wins at the API level
- **Auth**: All workspace endpoints sit behind the existing session auth middleware
- **DELETE/MOVE safety**: Confirmation dialogs in UI. Server validates paths. No recursive delete — single file/empty directory only

## What This Is NOT

- Not an IDE replacement — no LSP, no debugger, no integrated terminal, no extensions
- Not real-time collaborative editing — no CRDT, no operational transform
- Not trying to support large files — 1MB hard limit stays
- Goal: review agent work, make quick edits, watch agents modify files — without leaving the tab

## Relationship to Spec 17

- **Supersedes F-11** (ACP/IDE integration) — this is the native approach, no external editor needed
- **Complements F-10** (stream preview) — agent file edits are a form of streaming visibility
- **Closes** the "IDE integration" row in the 8-system architecture comparison
- **Feeds into** spec 17's E-3 (browser automation) — the editor could eventually show browser screenshots alongside code

## Effort Estimate

| Phase | Description | LOC (approx) | New files |
|-------|-------------|--------------|-----------|
| 1 | Multi-tab editor | ~150 | EditorTabs.svelte |
| 2 | Agent edit notifications | ~90 | — |
| 3 | File operations (create/delete/rename) | ~145 | TreeContextMenu.svelte |
| 4 | Clickable file paths | ~40 | — |
| 5 | Workspace search (stretch) | ~120 | — |
| **Total** | | **~545** | **2** |

Phase 1-2 are the core value. Phases 3-5 are incremental improvements.
