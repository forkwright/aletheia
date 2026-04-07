# Desktop app fixes (5 issues)

## Setup

You are in a worktree at `/data/worktrees/kimi-desktop`. Skip worktree creation.

Read `AGENTS.md` before writing code. Desktop crate is at `crates/theatron/desktop/`.

## Fix 1: "AGENTS" label → "NOUS"

Search for the string "AGENTS" and "agents" in the desktop source. Replace user-facing labels:
- "AGENTS" → "NOUS" (sidebar section header)
- "No agents" → "No nous" (empty state)
- "agent" → "nous" in user-visible text (but NOT in code identifiers like `agentId`, `AgentStore`)

Only change UI-visible strings, not struct names or API fields.

## Fix 2: Nous not loading in sidebar

The sidebar shows "No agents" despite a working API connection. The issue: `AgentStore` context is provided in two places (app.rs AND layout.rs), creating duplicate signals. The layout.rs provider creates a NEW empty store that shadows the one from app.rs.

Read `crates/theatron/desktop/src/layout.rs` — if there's a `use_context_provider(|| Signal::new(AgentStore::new()))` line, remove it. The provider should only be in `app.rs` (ConnectedApp).

Also check `crates/theatron/desktop/src/services/sse_coroutine.rs` — verify the SSE handler writes to the AgentStore signal when it receives agent status events.

## Fix 3: Theme consistency — force dark mode

Find the ThemeProvider initialization. Ensure it defaults to dark mode on all platforms. Search for `ThemeProvider`, `initial_mode`, or CSS variables related to background color.

The title bar color is OS-controlled (GTK/Wayland), but the content should be consistently dark.

## Fix 4: Command palette (#2409)

Check if a command palette component already exists. Search for `CommandPalette`, `command_palette`, `palette`. If it exists, wire it to Ctrl+K. If not, create a basic fuzzy-search overlay that filters navigation items.

## Fix 5: Hide sidebar by default (#2410)

The sidebar should start collapsed. Add a `sidebar_collapsed: bool` state (default true). Toggle with a button or Ctrl+B. When collapsed, show only icons, not labels.

## Validation

```bash
cargo build -p theatron-desktop --manifest-path crates/theatron/desktop/Cargo.toml 2>&1 | tail -5
```

Note: desktop is excluded from workspace, use manifest-path.

## Completion

1. `git add` changed files
2. Commit: `fix(theatron-desktop): nous naming, sidebar loading, dark theme, command palette, collapsed sidebar`
3. Trailer: `Gate-Passed: kanon 0.1.0`
4. Push and PR: `gh pr create --base main --title "fix(theatron-desktop): QoL fixes — naming, sidebar, theme, palette, collapse" --body "Closes #2409, closes #2410. Plus fixes for nous labeling, sidebar loading, theme consistency."`
