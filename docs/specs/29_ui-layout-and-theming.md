# Spec 29 — UI Layout Overhaul and Light Theme

| Field       | Value                          |
|-------------|--------------------------------|
| Status      | In Progress — light theme + agent activity done; layout overhaul pending |
| Author      | Demiurge                       |
| Created     | 2026-02-22                     |
| Scope       | `ui/`                          |
| Priority    | High                           |
| Depends On  | PR #153 (merged)               |

---

## Problem

The current UI has structural inefficiencies and unfinished areas:

1. **Sidebar wastes space.** The left sidebar shows 4–5 agent cards in a 260px-wide vertical list. Each agent card is a click target that swaps context. This consumes permanent horizontal real estate for what is functionally a 4-item selector — equivalent to a tab bar.

2. **No agent status visibility.** The sidebar shows agent names and unread badges but not what agents are *doing*. The `activeTurns` data from SSE already tracks this server-side, but the UI doesn't surface it. You can't tell if Syn is mid-turn, Akron is idle, or Demi is running a plan without clicking into each one.

3. **Streaming text renders as wall of text.** During `text_delta` streaming, the `streamingText` string accumulates and feeds into `<Markdown>` which calls `renderMarkdown()` (marked + DOMPurify). Markdown rendering *does* handle paragraph breaks — but the visual result during streaming often appears as a single run-on block because: (a) the markdown re-parses the entire accumulated string on every delta, which can cause layout thrash, and (b) partial markdown (incomplete paragraphs, incomplete tables) renders ambiguously mid-stream.

4. **Metrics and Settings overlap.** MetricsView shows: uptime, tokens, cache hit rate, turns, total cost, services health, agent table (sessions/messages/last activity/tokens/turns/cost), cron jobs, usage chart. SettingsView shows: instance name, uptime (duplicate), status (duplicate), agent list with models, theme toggle, font size, auth config, usage stats (duplicate: turns/tokens/cache), cost dashboard (duplicate), services (duplicate). Five categories of data appear in both views.

5. **Light mode doesn't exist.** The theme toggle in Settings switches `data-theme` to `"light"` but `:root` has no `[data-theme="light"]` override block. Selecting "Light" produces an unstyled mess. The Ardent site (`ardentleatherworks.com`) uses a warm light palette (`#F7F3E8` background, `#2A2725` text, same dye accents) that should be the reference.

---

## Design

### Phase 1 — Agent Bar (replace sidebar)

**Remove the sidebar.** Replace it with a horizontal agent strip in the topbar area.

```
┌──────────────────────────────────────────────────────────────────┐
│ [☰] Aletheia  ●Syn Idle  ●Akron Idle  ◉Demi Working  ●Syl Idle │ Files Metrics Settings
│─────────────────────────────────────────────────────────────────│
│                                                                  │
│                         Chat area                                │
│                    (now full-width)                               │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

**Each agent rendered as a compact pill:**

```
[🔨 Demi · Working]     ← active agent, highlighted border
[⚙️ Syn · Idle]          ← inactive, muted
[🔧 Akron · Idle]       ← inactive, muted  
[🌿 Syl · 2 unread]     ← unread badge replaces status text
```

**Data model additions:**

The pill status text derives from existing data:
- `activeTurns[agentId] > 0` → "Working" (amber dot)
- `unreadCount > 0` → "{n} unread" (accent dot)
- Default → "Idle" (muted dot)
- Distilling → "Distilling" (from `distill:before`/`distill:after` SSE events)

The runtime will emit a `status:update` SSE event with a short label (current tool name, "Running plan", "Researching") on tool_start, plan_start, etc. Phase 1 wires the pill to display this label when present, falling back to the binary states above.

**Components affected:**

| Component | Change |
|-----------|--------|
| `Layout.svelte` | Remove `<Sidebar>` import and rendering. Remove `sidebarCollapsed` state, `toggleSidebar()`, `closeSidebar()`, sidebar-related CSS. |
| `TopBar.svelte` | Add agent pills between brand name and nav buttons. Import agent store + events. Each pill is a clickable button that calls `setActiveAgent()`. |
| `Sidebar.svelte` | Keep file, hidden. Reserved for future sub-agent/session list. |
| `AgentCard.svelte` | Refactor into `AgentPill.svelte` — horizontal compact layout. |
| `global.css` | Remove `--sidebar-width` variable. |
| `chat-shared.css` | No changes needed. |

**Layout savings:** Reclaims 260px horizontal space on desktop. Mobile gains the full viewport width. The sidebar toggle button in the topbar becomes unnecessary — remove it.

**Mobile behavior:** On narrow screens (<768px), the agent pills can overflow horizontally with `overflow-x: auto` and `-webkit-overflow-scrolling: touch`. The hamburger menu remains for view navigation (Files/Metrics/Settings) but no longer needs a sidebar drawer.

**Create agent button:** The `+` button from the sidebar moves to the end of the agent pill row. Clicking it navigates to Settings where the full agent creation form lives. Rare action — doesn't justify inline complexity.

---

### Phase 2 — Streaming Text Quality

The root issue: `streamingText` is a raw string that grows with each `text_delta`. The `<Markdown>` component re-renders the entire string through `marked.parse()` + `DOMPurify.sanitize()` on every update. This is both expensive and visually jarring.

**Approach: buffered paragraph rendering.**

Instead of re-parsing the full string on every delta:

1. **Split on double-newline.** Maintain a `completedParagraphs: string[]` and a `currentParagraph: string` in chat state. When `text_delta` contains `\n\n`, flush `currentParagraph` to `completedParagraphs` and start a new one.

2. **Render completed paragraphs as finalized HTML.** Each entry in `completedParagraphs` is parsed through `renderMarkdown()` once and cached. No re-parsing on subsequent deltas.

3. **Render `currentParagraph` as live text.** Only the in-progress paragraph gets re-parsed on each delta. This bounds the markdown parsing cost to one paragraph instead of the full response.

4. **Assembly in MessageList.** The streaming message renders `completedParagraphs` (as static `{@html}` blocks) followed by `currentParagraph` (as a live `<Markdown>` component).

**Components affected:**

| Component | Change |
|-----------|--------|
| `chat.svelte.ts` | Replace `streamingText: string` with `completedBlocks: string[]` + `currentBlock: string`. Update `text_delta` handler to detect paragraph boundaries. |
| `MessageList.svelte` | Render streaming message as: finalized blocks (each `{@html renderMarkdown(block)}`) + live block (`<Markdown content={currentBlock} />`). |
| `Markdown.svelte` | No changes needed — already handles partial text gracefully. |

**Edge cases:**
- Code blocks spanning multiple deltas: detect ` ``` ` fence state. Don't split on `\n\n` inside fenced blocks.
- Tables: similarly, don't split mid-table (detect `|` prefix lines).
- Heading followed by paragraph: `\n\n` after `### Title` should flush as a unit with the heading.

A simpler v1 approach that avoids the fence/table complexity: **just buffer and render in 500ms intervals** instead of on every delta. This prevents the visual "wall of text" appearance by batching updates, and the full re-parse is bounded by time rather than content structure. The paragraph-splitting optimization can follow as v2 if performance is an issue.

**v1 implementation (recommended start):**

```typescript
// In sendMessage, replace direct text_delta accumulation:
case "text_delta":
  state.streamingBuffer += event.text;
  if (!state.streamingFlushTimer) {
    state.streamingFlushTimer = setTimeout(() => {
      state.streamingText = state.streamingBuffer;
      state.streamingFlushTimer = null;
    }, 100); // 100ms debounce — smooth enough, parse-efficient
  }
  break;
```

This alone eliminates the per-character re-parse without any structural changes to the rendering pipeline. On `turn_complete`, flush immediately.

---

### Phase 3 — Metrics/Settings Deduplication

**Principle:** Metrics = observability (how is the system performing?). Settings = configuration (how should the system behave?).

**Metrics keeps:**
- Uptime, token usage, cache hit rate, turn count (system health)
- Usage chart (trend visualization)
- Agent table: sessions, messages, last activity, tokens, turns, cost (agent health)
- Services health badges
- Cron job status
- Total cost card

**Settings keeps:**
- Theme toggle (dark/light)
- Font size control
- Authentication (token/session management)
- Agent list with *models* (configuration, not health)
- Instance name

**Remove from Settings:**
- Uptime (→ Metrics only)
- Status badge (→ Metrics only)
- Usage section (turns/tokens/cache — exact duplicate)
- CostDashboard component (→ Metrics only)
- Services section (→ Metrics only)

**Settings becomes lean:**

```
┌─────────────────────────────────┐
│ Settings                         │
│                                  │
│ ▸ Appearance                     │
│   Theme: [Dark] [Light]          │
│   Font:  [−] 14px [+]           │
│                                  │
│ ▸ Agents                         │
│   🔨 Demiurge  claude-opus-4-6    │
│   ⚙️ Syn       claude-opus-4-6    │
│   🔧 Akron     claude-sonnet-4   │
│   🌿 Syl       claude-sonnet-4   │
│                                  │
│ ▸ Authentication                 │
│   [Logout]                       │
│   Session Manager                │
│                                  │
└─────────────────────────────────┘
```

**Components affected:**

| Component | Change |
|-----------|--------|
| `SettingsView.svelte` | Remove: metrics fetch, uptime display, status display, usage section, CostDashboard import, services section. Keep: theme, font size, agents (with models), auth. |
| `MetricsView.svelte` | No additions needed — it already has everything. |
| `CostDashboard.svelte` | Remove from Settings import. Remains in Metrics via `UsageChart` or standalone. |

---

### Phase 4 — Light Theme

**Reference:** Ardent Leatherworks site. Warm parchment, not cold white. Same dye accents, same font families, inverted luminance.

**Add to `global.css`:**

```css
[data-theme="light"] {
  /* Backgrounds — warm parchment, not clinical white */
  --bg: #F7F3E8;
  --bg-elevated: #FFFFFF;
  --surface: #EDE8DC;
  --surface-hover: #E5DFD2;

  /* Borders — visible but soft */
  --border: #D4CEBD;
  --border-accent: #9A7B4F;

  /* Text — warm dark, not pure black */
  --text: #2A2725;
  --text-secondary: #6B6560;
  --text-muted: #9A9590;

  /* Accent — same brass, slightly deeper for light-bg contrast */
  --accent: #8A6B3F;
  --accent-hover: #7A5B2F;
  --accent-muted: rgba(138, 107, 63, 0.12);
  --accent-border: rgba(138, 107, 63, 0.35);

  /* Dye palette — same hues, adjusted for light background legibility */
  --aima: #581523;
  --aima-light: #6E1D2F;
  --thanatochromia: #2C1B3A;
  --thanatochromia-light: #3A2550;
  --aporia: #4A7A52;
  --aporia-muted: #5C8E63;
  --natural: #8B5A2B;
  --natural-light: #7A4E24;

  /* Status — same hues, darker for light-bg contrast */
  --status-success: #3A8A4B;
  --status-error: #B74440;
  --status-warning: #A8821F;
  --status-info: #7A6EA8;
  --status-active: #B48A2A;

  /* Syntax — adjusted for light background */
  --syntax-keyword: #A8654A;
  --syntax-string: #5A8D3E;
  --syntax-number: #4A7EA7;
  --syntax-comment: #9A9590;
  --syntax-function: #A48A4A;
  --syntax-type: #7A6EA8;
  --syntax-literal: #4A7EA7;
  --syntax-tag: #5A8D3E;
  --syntax-attr: #A8654A;
  --syntax-property: #8A7A5A;
  --syntax-meta: #5A7F93;
  --syntax-builtin: #A48A4A;
  --syntax-deleted: #B74440;
  --syntax-inserted: #3A8A4B;

  /* Shadows — lighter, warm-tinted */
  --shadow-sm: 0 2px 8px rgba(42, 39, 37, 0.08);
  --shadow-md: 0 4px 12px rgba(42, 39, 37, 0.12);
  --shadow-lg: 0 8px 24px rgba(42, 39, 37, 0.16);

  /* Scrollbar */
  color-scheme: light;
}

[data-theme="light"] ::-webkit-scrollbar-thumb {
  background: #D4CEBD;
}
[data-theme="light"] ::-webkit-scrollbar-thumb:hover {
  background: #9A9590;
}

[data-theme="light"] ::selection {
  background: rgba(138, 107, 63, 0.2);
}
```

**Additional light-mode overrides needed:**

| Component | Fix |
|-----------|-----|
| `chat-shared.css` | `.chat-msg:hover` and `.chat-msg.assistant` use `rgba(255,255,255,...)` — needs `rgba(0,0,0,...)` in light mode. Use CSS variables instead: `--msg-hover-bg` and `--msg-assistant-bg`. |
| `Markdown.svelte` | Code block background uses `--surface` — works automatically. Copy button uses `--bg-elevated` — works. |
| `InputBar.svelte` | Check that input background, border, placeholder colors use variables (likely already does). |
| `TopBar.svelte` | Check topbar background contrast. |
| `global.css` | Add `[data-theme="light"] .chat-msg:hover { background: rgba(0,0,0,0.02); }` etc. |

**Persist on load:** Currently `SettingsView.svelte` reads `localStorage.getItem(THEME_KEY)` but doesn't apply on page load. Add to `main.ts` or `Layout.svelte`:

```typescript
const savedTheme = localStorage.getItem("aletheia_theme") ?? "dark";
document.documentElement.setAttribute("data-theme", savedTheme);
```

---

## Implementation Order

| Phase | Scope | Effort | Risk |
|-------|-------|--------|------|
| **1** | Agent bar (replace sidebar) | Medium — layout restructure, new component | Medium — touches Layout/TopBar/Sidebar, mobile responsive |
| **2** | Streaming text quality | Low — chat store change, 100ms debounce | Low — isolated to stream path |
| **3** | Metrics/Settings dedup | Low — deletion-only in Settings | Low — pure removal |
| **4** | Light theme | Medium — CSS variables + overrides + testing | Low — additive CSS, no structural changes |

Phases 2–4 are independent of each other. Phase 1 is the structural change; phases 2–4 can ship before or after it.

**Recommended: Phase 2 → Phase 3 → Phase 4 → Phase 1.** Start with the quick wins (streaming fix, dedup cleanup, light theme) before the layout restructure.

---

## Decisions (2026-02-22)

1. **Sub-agent display.** Keep sidebar hidden, not deleted. Reserved for future sub-agent/session list.

2. **Rich agent status.** Yes — add `status:update` SSE event from the runtime. Pills should show current activity (tool name, "Running plan", "Researching") not just binary working/idle. Requires runtime change: emit on tool_start, plan_start, etc.

3. **Create agent UX.** Navigate to Settings. Rare action doesn't justify inline popover complexity.

4. **Mobile agent bar.** No cap needed — practical limit is ~5 agents. Long-term: group agents or collapse to icon-only on overflow.

---

## Non-Goals

- Redesigning the chat message layout (bubble vs. flat). Current flat layout is correct for information density.
- Adding agent avatars/images. Emoji is sufficient and loads instantly.
- Dark/light auto-detection (`prefers-color-scheme`). Manual toggle is fine for a single-user system.
- Redesigning the Graph view. Out of scope.
