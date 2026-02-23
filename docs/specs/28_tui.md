# Spec 28 — Aletheia TUI

**Status:** Phase 1 — Complete ✅  
**Component:** `tui/` — Rust terminal client (4,070 LOC as of 2026-02-23)  
**Interface:** Primary working interface alongside Signal (mobile)  
**Gateway dependency:** Hono REST API + SSE on `:18789` — zero gateway changes  
**Design target:** Level 2 dashboard — multi-agent visibility with split panes  

---

## 0. Why

The web UI validated Aletheia's UX patterns — agent switching, streaming, tool visualization, thinking blocks — but the maintenance cost is unsustainable. Mobile viewport hacks, SSE heartbeat edge cases, Svelte reactivity complexity, and constant browser-specific workarounds consume time better spent on the system itself.

The TUI replaces the web UI as the primary working interface. Signal remains the mobile interface. The web UI enters maintenance mode — functional, no new features.

**Non-negotiable requirements:**
- Multi-agent dashboard: see all agents' status simultaneously, not just the one you're talking to
- Real-time SSE streaming with proper reconnection
- Works over SSH, on Linux (Fedora/GNOME Terminal) and macOS
- Bearer token authentication
- Markdown rendering sufficient for agent responses
- Sub-millisecond input latency
- **Looks good.** Not "functional but dated." Beautiful, modern, on par with the best terminal apps. This is the primary working interface — it needs to feel like it.

---

## 0.5. Current State (as of 2026-02-23)

The TUI is a working application at 4,070 lines across 18 source files. Phase 1 is complete.

**Working:**
- TEA architecture (App struct, Msg enum, async update, pure view functions)
- Tokio event loop with `tokio::select!` over terminal + SSE + tick
- Bearer token auth with login flow and `~/.config/aletheia/tui.toml` config
- Agent sidebar with status indicators (streaming/working/idle/compacting)
- Agent switching via `Ctrl+A` overlay with arrow navigation
- Chat rendering with scrollable message list
- SSE global event stream with all turn/tool/distill lifecycle events
- SSE init event sends full active turn details (agents working when TUI connects show status)
- Streaming responses via POST `/api/sessions/stream` with text + thinking deltas
- Tool status display (name + elapsed time in status bar)
- Inline tool call rendering in chat (compact chain: `╰─ exec (0.3s) → read (0.1s) → grep`)
- Tool approval overlay (A to approve, D to deny)
- Plan approval overlay (Space to toggle steps, A to approve, C to cancel)
- Input with cursor movement, history (Up/Down), `Ctrl+W` (delete word), `Ctrl+U` (clear line)
- Input text wrapping with dynamic height (3-8 rows)
- Markdown rendering: bold, italic, inline code, code blocks, headings, lists
- Debounced markdown re-parse during streaming (every 64 chars or newline)
- Message queuing during active turns (prompt changes to yellow "queued ›")
- Mouse scroll in chat area
- Session selection by most recently updated (handles distillation count resets)
- Distillation lifecycle display (before/stage/after indicators)
- Notification dot on agents with unread turns (clears on switch)
- Scroll position saved/restored per agent when switching
- `bin/aletheia` CLI wrapper with subcommands (status, health, agents, costs, logs, token)
- File-based tracing via tracing-appender
- Theme system with `ThemePalette` struct: true color (24-bit), 256-color, 16-color fallback
- True color detection via `COLORTERM`, `TERM_PROGRAM`, `VTE_VERSION` env vars
- Rounded borders on overlays
- Braille spinner (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) for streaming/working animations
- Exponential backoff SSE reconnection (1s → 30s max, reset on successful connect)
- 401 detection on API calls (bubbles auth error for re-login)
- CI pipeline: clippy + fmt + build + test via GitHub Actions
- All code passes `cargo clippy -- -D warnings` and `cargo fmt --check`

**Not yet implemented (Phase 2+):**
- Syntax highlighting in code blocks (no `syntect` dep yet)
- Clipboard operations (no `arboard` dep yet)
- `Ctrl+E` compose mode ($EDITOR delegation)
- `@agent` Tab completion in input
- Session browser overlay
- Cost summary overlay
- Fuzzy filtering in overlays
- OSC 8 hyperlinks for URLs
- Table rendering in markdown
- State recovery on reconnect (reload stale agent data)
- Responsive layout degradation for small terminals
- Mouse click in sidebar to switch agent
- Error toasts with auto-dismiss
- Comprehensive test suite

---

## 0.6. Design Philosophy

### One Agent, One Conversation

Aletheia's core UX principle is **single engagement point per agent**. Users talk to an agent — not a session. Background sessions, sub-agents, distillation cycles, and cross-agent routing are infrastructure. They never surface to the user. The TUI auto-selects the canonical conversation session and presents it as continuous history. There is no session browser, no session picker, no session management UI. If we're doing our job, the user never thinks about sessions.

### Visual Quality

The current TUI renders with basic 16-color ANSI. It works. It looks like 2003. That's not acceptable for a primary working interface.

**Target quality:** On par with modern terminal apps (lazygit, gitui, bottom) and Claude Code. Specific improvements:

- **True color (24-bit RGB).** Detect capability, fall back gracefully. GNOME Terminal, iTerm2, Kitty, WezTerm all support it. Over SSH, `COLORTERM=truecolor` env var is the signal.
- **Theme system.** A coherent color palette — not ad-hoc `Color::Cyan` / `Color::Green` scattered across view files. Use `ratatui-themes` or define our own `ThemePalette` struct with semantic colors: `accent`, `muted`, `surface`, `error`, `success`, `info`. Default to a dark theme that matches Aletheia's identity (deep blues/teals, warm accents — not generic terminal green).
- **Rounded borders** where supported (`symbols::border::ROUNDED`). Clean visual separation without the brutalist box-drawing look.
- **Whitespace is design.** Padding inside panels. Breathing room between messages. Not every pixel needs content.
- **Typography via formatting.** Bold for agent names, dim for metadata (timestamps, model names), italic for thinking blocks. Intentional use of modifiers, not decoration.
- **Code blocks with visible boundaries.** Background color differentiation (dark surface), language label, padding. Even without syntect highlighting, a code block should look like a code block.
- **Braille spinner for streaming.** Claude Code uses braille characters (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) for a smoother animation than our current `◐◓◑◒`.

### WebUI Replacement, Not Supplement

The TUI must handle everything the web UI handles for working sessions:
- Full tool call visualization (not just name in status bar — show context, results)
- Thinking block content with expand/collapse
- Plan approval with individual step control
- Streaming with proper markdown
- Multi-agent status awareness

Things the web UI handles that the TUI won't (and that's fine):
- Memory graph visualization → separate concern
- Image rendering → punt to web or Kitty protocol later
- Sharing / external access → web stays for this

### Platform Support

Must work on:
- **Linux:** GNOME Terminal (Fedora default), Kitty, Alacritty, WezTerm, any VTE-based terminal
- **macOS:** Terminal.app (limited — 256 color only), iTerm2, Kitty, WezTerm, Ghostty
- **Over SSH:** No X11/Wayland forwarding assumed. OSC52 for clipboard. True color if `COLORTERM` set.
- **Inside tmux:** Passthrough for OSC sequences. Handle `TERM=screen-256color`.

Detection strategy: Check `COLORTERM`, `TERM`, `TERM_PROGRAM` env vars at startup. Degrade color depth gracefully (24-bit → 256 → 16). Log detected capabilities at debug level.

---

## 1. Architecture

### 1.1 High-Level

```
┌─────────────────────────────────────────────┐
│               Aletheia TUI                  │
│                                             │
│  ┌──────────┐  ┌─────────────────────────┐  │
│  │   Model   │  │    Event Loop           │  │
│  │ (AppState)│◄─┤  tokio::select! over:   │  │
│  │           │  │  1. Terminal (crossterm) │  │
│  │  agents[] │  │  2. SSE stream (reqwest)│  │
│  │  messages │  │  3. Tick timer (30fps)  │  │
│  │  input    │  └─────────────────────────┘  │
│  │  panels   │            │                  │
│  └─────┬─────┘            │                  │
│        │           ┌──────▼──────────────┐   │
│        │           │   View (ratatui)    │   │
│        │           │   Pure render fn    │   │
│        │           └─────────────────────┘   │
│  ┌─────▼─────────────────────────────────┐   │
│  │   API Client (reqwest + eventsource)  │   │
│  │   REST calls + SSE event stream       │   │
│  └───────────────────────────────────────┘   │
└─────────────────┬───────────────────────────┘
                  │ HTTP/SSE
                  ▼
           Gateway (:18789)
```

### 1.2 Application Pattern: Elm Architecture (TEA)

The Ratatui community and documentation recommend TEA for non-trivial applications. We adopt it without modification:

- **Model** (`AppState`): All application state in one struct. No widget-local state.
- **Message** (`Msg` enum): Every possible user action or external event.
- **Update** (`fn update(&mut self, msg: Msg)`): Pure state transitions. No rendering, no I/O.
- **View** (`fn view(&self, frame: &mut Frame)`): Pure rendering. No mutation, no I/O.

This gives us:
- Testable update logic (unit test state transitions without a terminal)
- Unidirectional data flow (matches immediate-mode rendering)
- Clean separation of concerns

### 1.3 Core Event Loop

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::init(config).await?;
    
    ratatui::run(|terminal| {
        let mut events = EventStream::new(config, app.api.clone());
        
        loop {
            terminal.draw(|frame| app.view(frame))?;
            
            tokio::select! {
                Some(term_event) = events.terminal.next() => {
                    if let Some(msg) = app.map_terminal_event(term_event?) {
                        app.update(msg).await;
                    }
                }
                Some(sse_event) = events.sse.next() => {
                    if let Some(msg) = app.map_sse_event(sse_event) {
                        app.update(msg).await;
                    }
                }
                _ = events.tick.tick() => {
                    app.update(Msg::Tick).await;
                }
            }
            
            if app.should_quit { break Ok(()); }
        }
    })
}
```

Three multiplexed event sources via `tokio::select!`:
1. **Terminal events** — keystrokes, mouse, resize (crossterm `EventStream`)
2. **SSE stream** — agent status changes, turn lifecycle events (`reqwest-eventsource`)
3. **Tick timer** — UI refresh for animations (spinner, streaming cursor) at 30fps

---

## 2. Verified Dependencies

### Current Cargo.toml (as of 2026-02-23)

```toml
[package]
name = "aletheia-tui"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# Core TUI — ratatui 0.30 defaults to crossterm 0.29
ratatui = "0.30"
crossterm = { version = "0.29", features = ["event-stream"] }

# Async runtime
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync"] }

# HTTP + SSE
reqwest = { version = "0.12", features = ["stream", "json", "rustls-tls", "cookies"], default-features = false }
reqwest-eventsource = "0.6"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Markdown rendering
pulldown-cmark = "0.13"

# Text utilities
textwrap = "0.16"
unicode-width = "0.2"

# CLI
clap = { version = "4", features = ["derive", "env"] }

# Config
toml = "0.8"
dirs = "6"

# Logging (file-based, not stdout)
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"

# Error handling
anyhow = "1"

# Futures
futures-util = "0.3"

[profile.release]
strip = true
lto = "thin"
opt-level = "z"
```

### Future dependencies (add when implementing)

| Crate | Version | Phase | Purpose |
|-------|---------|-------|---------|
| `syntect` | 5.3 | Phase 2 | Syntax highlighting in fenced code blocks |
| `arboard` | 3 | Phase 2 | Clipboard operations (with OSC52 fallback for SSH) |

### Resolved: Crossterm 0.29

Crossterm 0.29 is ratatui 0.30's **default** crossterm backend. The `crossterm` feature (enabled by default) resolves to `crossterm_0_29`. No feature override needed — just `crossterm = "0.29"` and ratatui's defaults.

Benefits: OSC52 clipboard copy (works over SSH without X11 forwarding), `query_keyboard_enhancement_flags` for detecting terminal capabilities.

### Resolved: Custom Input Widget

`tui-textarea` 0.7.0 depends on `ratatui ^0.29` — incompatible with ratatui 0.30. Forking adds maintenance burden. Custom widget is the right call. Current implementation handles: cursor movement, backspace/delete, Ctrl+U (clear), Ctrl+W (delete word), Up/Down (history), text wrapping with dynamic height (3-8 rows).

**Multi-line input:** `Shift+Enter` inserts a newline. `Enter` sends. This matches Claude Code and user expectation. Requires crossterm's `event-stream` feature with keyboard enhancement detection — most modern terminals (GNOME Terminal, iTerm2, Kitty, WezTerm) report Shift+Enter distinctly. For terminals that don't, fall back to `Ctrl+E` to open `$EDITOR`.

**Still needed:** `@agent` Tab completion, `Ctrl+E` $EDITOR delegation.

### Resolved: Custom Markdown Renderer

`pulldown-cmark` 0.13 parse → custom `ratatui::text::Line`/`Span` mapper. Full control over streaming-aware incremental parsing (re-parse on chunk arrival, debounced at 100ms). No external rendering dependency beyond pulldown-cmark itself.

---

## 3. Layout

### 3.1 Dashboard Mode (Default)

```
┌─ Aletheia ─────────────────────────────────────────── $0.43 today ──┐
│ agents ││ syn (streaming) ──────────────────────────────────────────│
│ ● syn  ││                                                          │
│ ◐ demi ││  You: fix the SSE pipeline                               │
│ ○ syl  ││                                                          │
│ ○ akron││  syn: The heartbeat mechanism uses SSE comment pings     │
│        ││  which browsers silently discard. The EventSource spec   │
│ ──────-││  says comments (lines starting with `:`) are consumed    │
│ sessions│  by the parser and never fire `onmessage`... ▌           │
│ 3f8a ● ││                                                          │
│ b2c1   ││──────────────────────────────────────────────────────────│
│ a917   ││ ⚙ demiurge: web_search (2.1s) │ syl: idle │ akron: idle │
│        │├──────────────────────────────────────────────────────────│
│        ││ > _                                                      │
│ Ctrl+? │└──────────────────────────────────────────────────────────│
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Layout Regions

| Region | Widget | Size | Content |
|--------|--------|------|---------|
| **Sidebar** | Agent list | Fixed 20 cols | Agent names with status + notification indicators |
| **Chat area** | Scrollable message list | Flex fill | Conversation with focused agent |
| **Status bar** | Agent activity summary | Fixed 1 row | Active agent states, keybinding hints |
| **Input area** | Custom text input | 3–8 rows (auto) | Message composition with Shift+Enter multiline |
| **Title bar** | App header | 1 row | App name, focused agent, SSE indicator |

### 3.3 Focused Mode (Toggle: `Ctrl+F`)

Hides sidebar, expands chat to full width. Status bar remains. For when you're in deep conversation with one agent and want maximum reading area.

### 3.4 Layout Implementation

Using ratatui 0.30's `Rect::layout()` with compile-time constraint checking:

```rust
fn view(&self, frame: &mut Frame) {
    let [sidebar, main] = frame.area().layout(
        &Layout::horizontal([Constraint::Length(10), Constraint::Fill(1)])
    );
    
    let [chat, status, input] = main.layout(
        &Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(self.input_height()),
        ])
    );
    
    self.render_sidebar(frame, sidebar);
    self.render_chat(frame, chat);
    self.render_status_bar(frame, status);
    self.render_input(frame, input);
}
```

---

## 4. State Model

```rust
/// Complete application state. Single source of truth.
pub struct App {
    // Connection
    api: ApiClient,
    connected: bool,
    gateway_url: String,
    
    // Agents
    agents: Vec<Agent>,
    focused_agent: usize,           // Index into agents[]
    
    // Per-agent state
    agent_state: HashMap<String, AgentViewState>,
    
    // UI mode
    mode: AppMode,
    show_sidebar: bool,
    
    // Input
    input: InputState,
    input_history: InputHistory,
    
    // Global
    daily_cost: f64,
    should_quit: bool,
    last_error: Option<(String, Instant)>,
}

pub struct AgentViewState {
    messages: Vec<ChatMessage>,
    scroll_offset: usize,
    streaming: bool,
    streaming_buffer: String,        // Accumulates chunks during streaming
    active_tools: Vec<ToolStatus>,
    last_activity: Option<Instant>,
}

pub struct Agent {
    id: String,
    display_name: String,
    model: String,
    status: AgentStatus,             // Idle, Working, Streaming, Error
}

#[derive(Clone)]
pub enum AgentStatus {
    Idle,
    Working { tool: String, elapsed: Duration },
    Streaming { tokens: usize },
    Error(String),
}

pub enum AppMode {
    Normal,                          // Input focused, chat scrollable
    Overlay(OverlayKind),            // Modal on top of main view
    Compose,                         // Full-screen editor for long messages
}

pub enum OverlayKind {
    AgentPicker,
    SessionBrowser,
    Help,
    CostSummary,
    SystemStatus,
    ToolApproval(ToolApprovalState),
    PlanApproval(PlanApprovalState),
}

pub struct ToolApprovalState {
    turn_id: String,
    tool_id: String,
    tool_name: String,
    input: serde_json::Value,
    risk: String,
    reason: String,
}

pub struct PlanApprovalState {
    plan_id: String,
    steps: Vec<PlanStepState>,
    total_cost_cents: u32,
    cursor: usize,            // Which step the cursor is on
}

pub struct PlanStepState {
    id: u32,
    label: String,
    role: String,
    checked: bool,            // Toggle with Space
    status: String,           // "pending", "running", "done", "failed"
}
```

---

## 5. Message Types

```rust
/// Every possible state transition.
pub enum Msg {
    // Terminal input
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    
    // SSE events (from gateway)
    SseInit { active_turns: HashMap<String, bool> },
    SseTurnBefore { nous_id: String, session_id: String, turn_id: String },
    SseTurnAfter { nous_id: String, session_id: String, outcome: TurnOutcome },
    SseToolCalled { nous_id: String, tool_name: String },
    SseToolFailed { nous_id: String, tool_name: String, error: String },
    SseStatusUpdate { nous_id: String, status: String },
    SseSessionCreated { nous_id: String, session_id: String },
    SsePing,
    SseDisconnected,
    SseReconnected,
    
    // Streaming response events (from POST /api/sessions/stream)
    StreamTurnStart { session_id: String, nous_id: String, turn_id: String },
    StreamTextDelta { text: String },
    StreamThinkingDelta { text: String },
    StreamToolStart { tool_name: String, tool_id: String },
    StreamToolResult { tool_name: String, tool_id: String, is_error: bool, duration_ms: u64 },
    StreamToolApprovalRequired { turn_id: String, tool_name: String, tool_id: String, input: serde_json::Value, risk: String, reason: String },
    StreamPlanProposed { plan: Plan },
    StreamPlanStepStart { plan_id: String, step_id: u32 },
    StreamPlanStepComplete { plan_id: String, step_id: u32, status: String },
    StreamPlanComplete { plan_id: String, status: String },
    StreamTurnComplete { outcome: TurnOutcome },
    StreamError { message: String },
    
    // API responses
    AgentsLoaded(Vec<Agent>),
    HistoryLoaded { agent_id: String, messages: Vec<ChatMessage> },
    CostsLoaded { daily: f64 },
    
    // Internal
    Tick,
    SendMessage,
    SwitchAgent(usize),
    ToggleSidebar,
    OpenOverlay(OverlayKind),
    CloseOverlay,
    ScrollUp(usize),
    ScrollDown(usize),
    CopyLastResponse,
    Quit,
}
```

---

## 6. API Contract

All endpoints are verified against the current gateway (`infrastructure/runtime/src/pylon/routes/`).

### 6.1 REST Endpoints

| Endpoint | Method | TUI Usage |
|----------|--------|-----------|
| `/health` | GET | Startup connectivity check |
| `/api/agents` | GET | Load agent list with models |
| `/api/agents/:id` | GET | Agent detail (model, status, workspace) |
| `/api/sessions` | GET | List sessions per agent |
| `/api/sessions/:id/history` | GET | Load conversation history |
| `/api/sessions/stream` | POST | Send message + receive streaming SSE response |
| `/api/sessions/:id/archive` | POST | Archive a session |
| `/api/sessions/:id/fork` | POST | Fork a session from a checkpoint |
| `/api/sessions/:id/queue` | GET/POST | Message queue (mid-turn course correction) |
| `/api/turns/active` | GET | Get active turns on reconnect |
| `/api/turns/:id/abort` | POST | Cancel streaming response |
| `/api/turns/:turnId/tools/:toolId/approve` | POST | Approve tool requiring confirmation |
| `/api/turns/:turnId/tools/:toolId/deny` | POST | Deny tool requiring confirmation |
| `/api/plans/:id` | GET | Get plan details |
| `/api/plans/:id/approve` | POST | Approve proposed plan |
| `/api/plans/:id/cancel` | POST | Cancel proposed plan |
| `/api/costs/summary` | GET | Daily cost for title bar |
| `/api/costs/agent/:id` | GET | Per-agent cost breakdown |
| `/api/status` | GET | System status overlay |
| `/api/auth/mode` | GET | Detect auth mode before prompting |
| `/api/auth/login` | POST | Authentication |
| `/api/auth/refresh` | POST | Token refresh |
| `/api/auth/logout` | POST | Invalidate session |

### 6.2 SSE Event Stream (`GET /api/events`)

Named events (not comments — this was a bug we already fixed). Ping interval: 15 seconds.

| Event | Data | TUI Action |
|-------|------|------------|
| `init` | `{ activeTurns }` | Sync agent status on connect |
| `turn:before` | `{ nousId, sessionId, turnId }` | Set agent status → Working |
| `turn:after` | `{ nousId, sessionId, outcome }` | Set agent status → Idle, reload history |
| `tool:called` | `{ nousId, toolName }` | Update agent's active tool display |
| `tool:failed` | `{ nousId, toolName, error }` | Show tool error in status |
| `status:update` | `{ nousId, status }` | General status change |
| `session:created` | `{ nousId, sessionId }` | Add to session list |
| `session:archived` | `{ nousId, sessionId }` | Remove from session list |
| `distill:before` | `{ nousId, sessionId }` | Show "compacting..." indicator on agent |
| `distill:stage` | `{ nousId, stage }` | Update compaction progress |
| `distill:after` | `{ nousId, sessionId }` | Clear compaction indicator |
| `ping` | `{}` | Reset heartbeat timer |

### 6.3 Streaming Response Events (`POST /api/sessions/stream`)

Per-request SSE stream for the message you just sent:

| Event | Data |
|-------|------|
| `turn_start` | `{ sessionId, nousId, turnId }` |
| `text_delta` | `{ text }` |
| `thinking_delta` | `{ text }` |
| `tool_start` | `{ toolName, toolId, input }` |
| `tool_approval_resolved` | `{ toolId, decision }` |
| `plan_proposed` | `{ plan }` |
| `queue_drained` | `{ count }` |
| `tool_result` | `{ toolName, toolId, result, isError, durationMs, tokenEstimate? }` |
| `tool_approval_required` | `{ turnId, toolName, toolId, input, risk, reason }` |
| `turn_complete` | `{ outcome: { text, nousId, sessionId, model, toolCalls, inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, error? } }` |
| `error` | `{ message }` |
| `turn_abort` | `{ reason }` |
| `plan_step_start` | `{ planId, stepId }` |
| `plan_step_complete` | `{ planId, stepId, status, result? }` |
| `plan_complete` | `{ planId, status }` |

### 6.4 Authentication

The gateway uses JWT access tokens + HTTP-only refresh cookies. For TUI:

1. On startup, GET `/api/auth/mode` to detect auth configuration (`none`, `token`, `password`, `session`)
2. If `none` → skip auth entirely. If `token` → use `--token` CLI arg or `ALETHEIA_TOKEN` env var. If `session` or `password` → proceed to login flow.
3. POST `/api/auth/login` with username/password from config or interactive prompt
4. Store access token in memory, attach as `Authorization: Bearer <token>` header
5. On 401, POST `/api/auth/refresh` (cookie-based) to get new access token
6. Refresh cookie is HTTP-only — `reqwest`'s cookie store handles this transparently

### Resolved: Prompt + Session File Credential Storage

This tool will be shared with others to run on their own machines, so the auth UX matters.

1. **First launch:** TUI detects no stored session, prompts for gateway URL + username + password interactively
2. **On successful login:** Store refresh token + gateway URL in `~/.config/aletheia/session.json` (0600 permissions). Access token stays in memory only.
3. **Subsequent launches:** Load session file, use refresh token to get new access token silently. If refresh fails (expired/revoked), re-prompt.
4. **`aletheia-tui logout`:** Delete session file.

The gateway also supports `mode: "token"` (static bearer token) and `mode: "none"` (no auth). For token mode, accept `--token` CLI arg or `ALETHEIA_TOKEN` env var. For no-auth, skip the login flow entirely.

Keyring was considered but rejected — heavy dependency, doesn't work over SSH, and the refresh token approach gives equivalent security for a single-user local tool.

---

## 7. Keybindings

### 7.1 Normal Mode (input focused)

| Key | Action | Status |
|-----|--------|--------|
| `Enter` | Send message | ✅ |
| `Shift+Enter` | Insert newline (multi-line input) | — |
| `Ctrl+E` | Open `$EDITOR` for complex input | — |
| `Ctrl+C` / `Ctrl+Q` | Quit | ✅ |
| `Ctrl+A` | Agent picker overlay | ✅ |
| `Ctrl+F` | Toggle sidebar | ✅ |
| `Ctrl+T` | Toggle thinking block visibility | ✅ |
| `Ctrl+Y` | Copy last response to clipboard | — |
| `Ctrl+U` | Clear input line | ✅ |
| `Ctrl+W` | Delete word | ✅ |
| `F1` | Help overlay | — |
| `F2` | Token/cost summary overlay | — |
| `Ctrl+I` | System status overlay | — |
| `PageUp` / `PageDown` | Scroll message history | ✅ |
| `Shift+Up` / `Shift+Down` | Scroll message history | ✅ |
| `Up` / `Down` | Input history | ✅ |
| `Tab` | Cycle `@agent` completion | — |
| `Esc` | Close overlay | ✅ |

**Removed from spec:** `Ctrl+S` (session browser) — sessions are not a user-facing concept. `Ctrl+N` (new session) — continuity model means no manual session boundaries. `Ctrl+$` — unreliable terminal keybinding.

### 7.2 Overlay Mode

| Key | Action |
|-----|--------|
| `↑` / `↓` or `j` / `k` | Navigate list |
| `Enter` | Select item |
| `Esc` | Close overlay |
| `/` | Filter/search within overlay |

### 7.3 Multi-Line Input

**Resolved: `Ctrl+E` opens `$EDITOR`** (the `git commit` pattern).

Writes current input buffer to a temp file, suspends TUI, opens `$EDITOR` (or `$VISUAL`, falling back to `vi`). On save+quit, file contents become the message. On empty save or editor error, discard.

This is proven, portable, handles arbitrarily long input, and — notably — fish shell already uses `Ctrl+E` for "edit command line in $EDITOR", so the keybinding is muscle memory.

`Alt+Enter` for inline newlines may be added later as a convenience, but the `$EDITOR` path is the primary multi-line mechanism.

---

## 8. Rendering

### 8.1 Message Rendering

Messages are **not** individually bordered — that's too heavy. Instead, use visual hierarchy:

```
  you                                                      4:18 PM
  Review the updated TUI spec and give me your honest thoughts

  🌀 Syn                                                   4:19 PM
  Good spec. Honest assessment:

  **What's strong:**
  - The "why" is right. Web UI maintenance cost is real…
  - TEA architecture is the correct call.

  ✓ read  (0.1s)
  ╰─ docs/specs/28_tui.md

  **What's off:**
  1. Status is "Draft" but we already have a working TUI…
```

- **Agent name** in bold + agent color, with emoji. Timestamps right-aligned in dim.
- **User messages** in default fg, no special treatment.
- **Blank line** between messages. No borders — whitespace is the separator.
- **Tool calls** render inline between text blocks (see §8.5).
- **Code blocks** get a subtle background color shift + language label + left border:
  ```
  │ rust
  │ controller.enqueue(": ping\n\n");
  │ let x = foo.bar();
  ```

### 8.2 Markdown Subset

| Element | Rendering |
|---------|-----------|
| `**bold**` | `Style::new().bold()` |
| `*italic*` | `Style::new().italic()` |
| `` `inline code` `` | Yellow on dark gray |
| ```` ```lang ``` ```` | Bordered block + syntect highlighting (top 15 languages) |
| `# Heading` | Bold + colored |
| `- list` | Indented with `•` prefix |
| `> blockquote` | Dim + `│` left border |
| `| table |` | Ratatui `Table` widget |
| `[link](url)` | Underline + OSC 8 hyperlink if terminal supports it |

### 8.3 Streaming Rendering

During streaming:
1. Accumulate `text_delta` chunks into `streaming_buffer`
2. Re-parse markdown every 100ms (debounced, not per-chunk)
3. Render with blinking cursor `▌` at end
4. On `turn_complete`, parse final text, store as `ChatMessage`, clear buffer

### 8.4 Thinking Blocks

Collapsed by default: `▶ thinking (142 tokens)`  
Expanded (`Ctrl+T`): Full content in dim italic with `│` left border.

### 8.5 Tool Call Rendering (Inline)

Tool calls are **not** just status bar items. They render inline in the chat, like Claude Code does. This is critical for the TUI to replace the web UI.

**During execution:**
```
  ⠹ exec  (2.3s)
  ├─ command: git log --oneline -10
```

**After completion:**
```
  ✓ exec  (1.2s)
  ├─ command: git log --oneline -10
  ╰─ 10 lines returned

  ✓ read  (0.1s)
  ├─ path: src/app.rs
  ╰─ 1,266 lines
```

**On error:**
```
  ✗ exec  (0.5s)  ← red
  ├─ command: rm -rf /
  ╰─ denied: requires approval
```

Tool calls are collapsible — show the summary line by default, expand to see full input/output on selection. This keeps the chat clean while making all tool activity inspectable.

### 8.5.1 Status Bar (Agent Summary)

The status bar summarizes *all* agents' state in one line:

```
 ⠹ syn: exec (3.2s) │ ⠸ demi: web_search (1.4s) │ syl │ akron
```

Braille spinner characters cycle on tick: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` (smooth 10-frame animation vs. current 4-frame).

### 8.6 Tool Approval Flow

When a `tool_approval_required` event arrives during streaming, the TUI must pause the stream display and prompt the user:

```
┌─ Tool Approval Required ─────────────────────────────┐
│                                                       │
│  syn wants to run: exec                               │
│  Risk: high                                           │
│  Reason: Modifying system configuration file          │
│                                                       │
│  Input:                                               │
│  {"command": "sed -i 's/old/new/' /etc/config.yml"}   │
│                                                       │
│  [A]pprove    [D]eny    [V]iew full input             │
└───────────────────────────────────────────────────────┘
```

- Renders as a centered overlay (same system as help/picker overlays)
- `A` → POST `/api/turns/:turnId/tools/:toolId/approve`, close overlay, resume stream
- `D` → POST `/api/turns/:turnId/tools/:toolId/deny`, close overlay, stream continues with denial
- Multiple approvals can queue — show count badge if >1 pending

### 8.7 Plan Approval Flow

When a `plan_proposed` event arrives, render the plan as an interactive overlay:

```
┌─ Plan Proposed (4 steps, ~$0.32) ────────────────────┐
│                                                       │
│  [✓] 1. Explore codebase for auth patterns  (explorer)│
│  [✓] 2. Implement token refresh logic         (coder) │
│  [✓] 3. Write integration tests               (coder) │
│  [✓] 4. Review changes                      (reviewer)│
│                                                       │
│  Steps 1 can run in parallel with nothing             │
│  Steps 2-3 are sequential                             │
│  Step 4 depends on 2-3                                │
│                                                       │
│  [A]pprove all   [C]ancel   [Space] toggle step       │
└───────────────────────────────────────────────────────┘
```

- Each step has a checkbox (Space to toggle — pre-checked by default)
- `A` → POST `/api/plans/:id/approve` with checked steps, close overlay
- `C` → POST `/api/plans/:id/cancel`, close overlay
- During execution, `plan_step_start`/`plan_step_complete` events update a persistent status widget showing plan progress

### 8.8 Distillation Indicator

When `distill:before` fires for an agent:
- Agent status pill changes to `◉` (compacting)
- Status bar shows "compacting..." with the stage from `distill:stage` events
- On `distill:after`, revert to normal status and reload history (context has changed)

---

## 9. SSE Connection Management

This is where the web UI failed. The TUI must get it right from the start.

### 9.1 Connection Lifecycle

```
Start → Connect → Connected ←→ Receiving Events
                      │
                      ▼ (error/timeout)
                 Reconnecting
                      │
                      ▼ (backoff: 1s, 2s, 4s, 8s, max 30s)
                   Connect
```

### 9.2 Heartbeat

- Gateway sends `event: ping` every 15 seconds (named event, not comment)
- TUI tracks last received event (any event, not just ping)
- If no event received in 45 seconds, consider connection dead
- Close and reconnect with exponential backoff

### 9.3 State Recovery on Reconnect

On SSE reconnect:
1. `init` event provides `activeTurns` map — restore agent working status
2. GET `/api/turns/active` for detailed turn info
3. For any agent whose messages were last loaded before disconnect, reload history
4. Resume normal event processing

### 9.4 Dual SSE Streams

The TUI maintains **two** concurrent SSE connections:
1. **Global event stream** (`GET /api/events`) — agent status, always connected
2. **Per-message stream** (`POST /api/sessions/stream`) — response to user's message, one at a time

These are independent. The global stream runs continuously. The per-message stream is created per send and consumed to completion.

---

## 10. Configuration

```toml
# ~/.config/aletheia/tui.toml

[gateway]
url = "http://localhost:18789"
# url = "http://100.87.6.45:18789"  # Tailscale

[defaults]
agent = "syn"
show_thinking = false
show_sidebar = true

[theme]
# Uses terminal colors by default (respects your colorscheme)
# Override specific elements:
# agent_working = "yellow"
# agent_idle = "dim"
# user_message_border = "blue"
# agent_message_border = "green"

[keybindings]
# Overrides (not implemented in Phase 1)
```

CLI overrides:

```bash
aletheia-tui                                    # defaults
aletheia-tui --url http://10.0.0.5:18789       # remote gateway
aletheia-tui --agent demiurge                   # start focused on agent
aletheia-tui --session <id>                     # resume specific session
```

---

## 11. Directory Structure

```
tui/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── src/
│   ├── main.rs                     # Entry point, arg parsing, terminal init
│   ├── app.rs                      # App struct, update(), TEA orchestration
│   ├── msg.rs                      # Msg enum, all message types
│   ├── config.rs                   # Config file + CLI arg merging
│   │
│   ├── api/
│   │   ├── mod.rs
│   │   ├── client.rs               # reqwest HTTP client, auth token management
│   │   ├── sse.rs                  # Global SSE stream + heartbeat + reconnect
│   │   ├── streaming.rs            # Per-message SSE stream consumer
│   │   └── types.rs                # API request/response types (serde)
│   │
│   ├── view/
│   │   ├── mod.rs                  # Top-level view() dispatch
│   │   ├── sidebar.rs              # Agent list with status + notification indicators
│   │   ├── chat.rs                 # Message list rendering
│   │   ├── input.rs                # Input widget (custom, wrapping)
│   │   ├── status_bar.rs           # Agent activity summary
│   │   ├── title_bar.rs            # App name + daily cost
│   │   └── overlay.rs              # Modal overlays (agent picker, tool/plan approval)
│   │
│   ├── markdown.rs                 # pulldown-cmark → ratatui spans
│   ├── events.rs                   # Event type definitions (SSE + stream)
│   ├── msg.rs                      # Msg enum (all state transitions)
│   ├── highlight.rs                # (Phase 2) syntect code highlighting
│   └── clipboard.rs                # (Phase 2) Clipboard operations (arboard + OSC52)
│
└── tests/
    ├── update_tests.rs             # Unit tests for state transitions
    ├── markdown_tests.rs           # Markdown rendering tests
    └── api_types_tests.rs          # Serialization round-trip tests
```

---

## 12. Coding Standards

### 12.1 Rust Standards

- **Edition:** 2024 (requires rustc 1.86+)
- **Clippy:** `#![deny(clippy::all, clippy::pedantic)]` with explicit `#[allow()]` for justified exceptions
- **Formatting:** `rustfmt` with default settings, enforced in CI
- **Error handling:** `anyhow` for application errors (this is an application, not a library). Typed errors via `thiserror` only if we extract reusable library crates later.
- **Unsafe:** Zero `unsafe` blocks. No exceptions.
- **Unwrap:** Zero `.unwrap()` in non-test code (currently holds). Use `.expect("reason")` only for provably-infallible cases.
- **Dependencies:** Minimal. Every dependency must be justified. Prefer `default-features = false`.

### 12.2 Architecture Rules

- **No widget-local state.** All state lives in `App`. View functions are `&self` only.
- **Minimize I/O in update.** The `update()` method is async and currently performs I/O directly (API calls, spawned tasks). Acceptable for an application of this scope. If update logic grows significantly, consider extracting a `Command` pattern where update returns effects to be executed in the event loop.
- **No panics.** Every error path returns `Result` or logs and continues.
- **No blocking.** All I/O is async. Terminal event reading uses crossterm's async `EventStream`.
- **No global state.** No `static mut`, no `lazy_static` for mutable data.

### 12.3 Testing

- State transitions: Unit tests for every `Msg` variant
- Markdown rendering: Snapshot tests for markdown → spans conversion
- API types: Serde round-trip tests against actual gateway JSON samples
- Integration: `ratatui::backend::TestBackend` for rendering verification

---

## 13. Implementation Phases

### Phase 1: Dashboard MVP ✅ Complete

**Milestone:** Multi-agent dashboard with streaming chat, sidebar, and real-time status. Usable as primary interface.

**Foundation**
- [x] `Cargo.toml` with verified deps
- [x] `config.rs`: Load `~/.config/aletheia/tui.toml` + CLI args
- [x] `api/client.rs`: reqwest client with bearer token auth, login flow
- [x] `main.rs`: `ratatui::run()` with TEA event loop
- [x] `app.rs`: `App` struct with `update()`/`view()` split (1,300+ lines)
- [x] `msg.rs`: All message types (110 lines, 40+ variants)
- [x] `events.rs`: Event type definitions for SSE + streaming
- [x] 401 detection on API calls (bubbles `UNAUTHORIZED` error for re-login flow)
- [x] CI: `cargo clippy -- -D warnings`, `cargo test`, `cargo fmt --check`, `cargo build --release` (GitHub Actions)
- [x] **Theme system:** `ThemePalette` struct with 30+ semantic colors, auto-detection of color depth
- [x] **Rounded borders** (`symbols::border::ROUNDED`) on overlays
- [x] **True color detection:** `COLORTERM`, `TERM_PROGRAM`, `VTE_VERSION` → degrade to 256/16 gracefully
- [x] **Braille spinner:** `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` for streaming/working animations

**Dashboard layout**
- [x] Three-region layout: sidebar | chat + status + input
- [x] `view/sidebar.rs`: Agent list with status indicators (`●` streaming, `⠋` working, `○` idle)
- [x] **Notification dot:** `•` next to agent name when a turn completes while unfocused. Clears on switch.
- [x] ~~Session list under agents~~ — removed. Sessions are not user-facing. Sidebar shows agents only.
- [x] `view/status_bar.rs`: All-agent one-line activity summary with tool names and elapsed time
- [x] Agent switching: `Ctrl+A` overlay with arrow navigation, loads history
- [x] Focused mode toggle (`Ctrl+F`) — hide sidebar, expand chat
- [x] Scroll position saved/restored per agent when switching

**Chat + streaming**
- [x] `view/chat.rs`: Scrollable message list with role headers + whitespace typography
- [x] `view/input.rs`: Custom input with cursor, history (Up/Down), Ctrl+U/Ctrl+W, text wrapping
- [x] `api/streaming.rs`: POST `/api/sessions/stream`, yields SSE events as `Msg` variants
- [x] `markdown.rs`: Core subset — bold, italic, inline code, code blocks, headings, lists
- [x] Streaming cursor (braille spinner)
- [x] Send message → stream response → render in chat → store in state
- [x] Message queuing during active turns (yellow "queued ›" prompt)
- [x] **Inline tool call rendering** — compact tool chain in chat: `╰─ exec (0.3s) → read (0.1s) → grep`
- [x] Debounced markdown re-parse during streaming (every 64 chars or newline boundary)

**Multi-agent awareness**
- [x] `api/sse.rs`: Global SSE event stream with heartbeat detection
- [x] SSE `init` → sync all agent statuses (sends full `ActiveTurn` details, not just counts)
- [x] SSE `turn:before`/`turn:after` → update agent status in sidebar + status bar
- [x] SSE `tool:called` → show tool name in status bar for that agent
- [x] `distill:before/stage/after` → compaction indicator on agent
- [x] Exponential backoff reconnect (1s → 30s max, reset on successful connection)
- [x] On `turn:after` for focused agent → reload history automatically

### Phase 2: Rich Rendering + Overlays (3-4 days)

**Milestone:** Code highlighting, thinking blocks, tables, all modal overlays, compose mode.

- [ ] `highlight.rs`: syntect integration for fenced code blocks (top 15 languages)
- [ ] `markdown.rs`: Tables, blockquotes, links (OSC 8 hyperlinks), nested lists
- [x] Thinking block toggle (`Ctrl+T`)
- [x] `view/overlay.rs`: Modal overlay system (centered floating block over main view)
- [x] Agent picker overlay (`Ctrl+A`)
- [ ] Help overlay (`F1`)
- [ ] Token summary overlay (`F2`) — nice-to-have, not priority
- [ ] System status overlay (`Ctrl+I`)
- [ ] Fuzzy filtering in overlays (`/` to search, arrow keys to navigate)
- [ ] `clipboard.rs`: Copy last response (`Ctrl+Y`) — arboard with OSC52 fallback
- [ ] `Ctrl+E` compose mode: suspend TUI, spawn `$EDITOR`, send on save+quit
- [ ] New session / topic boundary (`Ctrl+N`)
- [x] Tool approval overlay: render on `tool_approval_required`, A/D to approve/deny
- [x] Plan approval overlay: render on `plan_proposed`, toggle steps, approve/cancel
- [ ] Plan execution progress: persistent mini-widget during plan execution

### Phase 3: Polish + Production Hardening (3-4 days)

**Milestone:** Daily-driver quality. Handles every edge case gracefully.

- [ ] Reconnection resilience: network drops, gateway restarts, laptop sleep/wake
- [ ] State recovery on reconnect: reload history for stale agents, restore streaming state
- [ ] `@agent` mention completion in input (Tab to cycle)
- [x] Mouse support: scroll wheel in chat
- [ ] Mouse support: click agent in sidebar to switch
- [ ] Error display: inline toast with auto-dismiss after 5s, persistent errors in status bar
- [ ] Responsive layout: graceful degradation for small terminals (hide sidebar < 60 cols)
- [x] Tracing to file via tracing-appender
- [ ] Log rotation configuration
- [ ] `aletheia logout` subcommand
- [ ] Comprehensive tests: state transitions, markdown rendering snapshots, API type round-trips
- [ ] README with screenshots and install instructions

---

## 14. Open Questions

1. **Binary distribution:** Build from source for now? Cross-compile for release? `cargo-dist` for GitHub releases? Source-only is fine for Phase 1 since Rust toolchain is a prerequisite anyway.
2. **Notification:** Terminal bell (`\x07`) on turn completion when TUI is backgrounded? Desktop notification via `notify-rust`? Or silent?
3. **Inline images:** Kitty/Sixel protocol support for image-capable terminals, or always punt to web UI? Low priority — most agent responses are text.
4. **tmux integration:** Detect tmux, handle passthrough sequences for OSC 8 links and clipboard? Many users will run inside tmux.
5. ~~**Message queue UX:**~~ **Resolved.** Prompt changes to yellow "queued ›" when agent is mid-turn. Messages sent during active turns POST to `/api/sessions/:id/queue`. No additional UX needed.
6. ~~**Session forking:**~~ **Deferred.** Sessions are not user-facing. Forking is an infrastructure concern exposed via API if needed.
7. ~~**Cost display granularity:**~~ **Resolved.** Cost is not a priority feature. Token count is a nice-to-have in a status overlay (`F2`), not a dashboard element. Remove daily cost from title bar — replace with something more useful or leave clean.

---

## 15. What This Replaces

The web UI (`ui/`) enters maintenance mode:
- Fix critical bugs only (e.g., #163 SSE tracking)
- No new features
- Remains available for: showing people what Aletheia is, memory graph visualization, image content
- Eventually: dormant, replaced by TUI + Signal

---

*Drafted: 2026-02-23 by Syn*  
*Revised: 2026-02-23 — reconciled with implementation (3,240 LOC), fixed dep list, marked completed items*  
*Gateway API verified against: infrastructure/runtime/src/pylon/routes/*
