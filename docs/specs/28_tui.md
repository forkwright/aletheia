# Aletheia TUI

**Status:** Draft
**Component**: `tui/` — Terminal User Interface Client
**Target**: Ratatui + Crossterm (Rust)
**Interface**: Third client alongside Web UI and Signal
**Gateway Dependency**: Existing Hono REST API + SSE on `:18789`
**Scope**: Conversational interface with agent management — not a web UI replacement

---

## 1. Design Principles

The TUI is a **thin client**. It owns no business logic, no memory management, no agent routing. It consumes the same gateway API that the Svelte UI and Signal client already use. This means zero gateway changes are required.

**In scope**: Message send/receive, SSE streaming, agent switching, session management, cost display, thinking block visualization, basic markdown rendering.

**Out of scope**: File upload, force-directed memory graph, full syntax highlighting (basic only), image rendering. These stay in the web UI.

---

## 2. Architecture

```
┌─────────────────────────────────────┐
│            Aletheia TUI             │
│                                     │
│  ┌───────────┐  ┌────────────────┐  │
│  │  App State │  │  Event Loop    │  │
│  │            │◄─┤  (crossterm +  │  │
│  │  messages  │  │   tokio select)│  │
│  │  agents    │  │                │  │
│  │  sessions  │  └────────────────┘  │
│  │  input buf │          │           │
│  └─────┬──────┘          │           │
│        │           ┌─────▼────────┐  │
│        │           │   Renderer   │  │
│        │           │  (ratatui)   │  │
│        │           └──────────────┘  │
│  ┌─────▼──────────────────────────┐  │
│  │       API Client (reqwest)     │  │
│  │  REST calls + SSE streaming    │  │
│  └────────────────────────────────┘  │
└──────────────────┬──────────────────┘
                   │ HTTP/SSE
                   ▼
            Gateway (:18789)
```

### Core Loop

Standard Ratatui event loop with `tokio::select!` multiplexing three event sources:

1. **Terminal events** — keystrokes, resize (via `crossterm::event`)
2. **SSE stream** — assistant response chunks (via `reqwest` + `eventsource-stream`)
3. **Tick timer** — UI refresh at 30fps for spinner animations, streaming cursor

---

## 3. Dependencies

```toml
[package]
name = "aletheia-tui"
version = "0.1.0"
edition = "2021"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["stream", "json"] }
eventsource-stream = "0.2"
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
dirs = "6"
toml = "0.8"
textwrap = "0.16"
syntect = "5"
pulldown-cmark = "0.12"
unicode-width = "0.2"
arboard = "3"
```

Estimated binary size: ~8-12MB (static link, release build with `strip = true`).

---

## 4. Layout

### Primary View — Chat

```
┌─ Aletheia ─────────────────────── agent: curator ─── session: 3f8a ──┐
│                                                                       │
│  ╭─ You ──────────────────────────────────────────────────────────╮   │
│  │ whats the term for text based images                          │   │
│  ╰────────────────────────────────────────────────────────────────╯   │
│                                                                       │
│  ╭─ curator ──────────────────────────────────────────────────────╮   │
│  │ You're thinking of **ASCII art** — images composed of text    │   │
│  │ characters from the ASCII character set.                      │   │
│  ╰────────────────────────────────────────────────────────────────╯   │
│                                                                       │
│  ╭─ You ──────────────────────────────────────────────────────────╮   │
│  │ then whats the rust tui library                               │   │
│  ╰────────────────────────────────────────────────────────────────╯   │
│                                                                       │
│  ╭─ curator ──────────────────────────────────────────────────────╮   │
│  │ ▌ Ratatui — it's the most popular Rust library for building   │   │
│  │ terminal user interfaces...                                   │   │
│  ╰────────────────────────────────────────────────────────────────╯   │
│                                                                       │
├───────────────────────────────────────────────────────────────────────┤
│ > _                                                                   │
│                                                            Ctrl+? help│
└───────────────────────────────────────────────────────────────────────┘
```

### Layout Breakdown

| Region           | Widget         | Height    |
|------------------|----------------|-----------|
| Title bar        | `Block` title  | 1 row     |
| Message area     | Scrollable `List` / custom widget | flex fill |
| Divider          | `Block` border | 1 row     |
| Input area       | `Paragraph` (editable) | 1-5 rows (auto-expand) |
| Status bar       | `Paragraph`    | 1 row     |

### Secondary Views (Modal Overlays)

| View             | Trigger       | Content |
|------------------|---------------|---------|
| Agent picker     | `Ctrl+A`      | List of agents with model info, arrow-key select |
| Session browser  | `Ctrl+S`      | Table of sessions with timestamps, agent, preview |
| Help             | `Ctrl+?`      | Keybinding reference |
| Cost summary     | `Ctrl+$`      | Token usage from `/api/costs/summary` |
| System status    | `Ctrl+I`      | From `/api/status` — version, uptime, services |

Modals render as centered floating `Block` widgets over the chat view.

---

## 5. Keybindings

### Normal Mode (input focused)

| Key              | Action |
|------------------|--------|
| `Enter`          | Send message (or newline with `Shift+Enter` / `Alt+Enter`) |
| `Ctrl+C`         | Cancel streaming response / exit if idle |
| `Ctrl+D`         | Exit |
| `Ctrl+A`         | Agent picker |
| `Ctrl+S`         | Session browser |
| `Ctrl+N`         | New session |
| `Ctrl+R`         | Reset current session |
| `Ctrl+?` / `F1`  | Help overlay |
| `Ctrl+$`         | Cost summary |
| `Ctrl+I`         | System status |
| `Ctrl+T`         | Toggle thinking block visibility |
| `Ctrl+Y`         | Copy last response to clipboard |
| `Ctrl+U`         | Clear input line |
| `PageUp/Down`    | Scroll message history |
| `Esc`            | Close overlay / cancel current action |

### Overlay Mode

| Key              | Action |
|------------------|--------|
| `↑/↓` or `j/k`  | Navigate list |
| `Enter`          | Select |
| `Esc`            | Close |
| `/`              | Filter/search within overlay |

---

## 6. API Integration

All calls target `http://{host}:{port}` where defaults are `localhost:18789`, configurable via CLI args or config file.

### Endpoints Used

| Endpoint                        | Method | Purpose |
|---------------------------------|--------|---------|
| `/health`                       | GET    | Startup connectivity check |
| `/api/status`                   | GET    | Version, agents, uptime |
| `/api/agents`                   | GET    | Agent list with model info |
| `/api/sessions/stream`          | POST   | Send message, receive SSE stream |
| `/api/sessions`                 | GET    | List sessions (for browser) |
| `/api/costs/summary`            | GET    | Token/cost data |
| `/api/metrics`                  | GET    | System metrics |
| `/api/events`                   | GET    | Global SSE event stream |

### SSE Streaming

```rust
// Pseudocode
async fn stream_response(client: &Client, message: &str, agent: &str) {
    let response = client
        .post(format!("{}/api/sessions/stream", base_url))
        .json(&json!({
            "message": message,
            "agent": agent,
            "session_id": current_session
        }))
        .send()
        .await?;

    let mut stream = response.bytes_stream().eventsource();

    while let Some(event) = stream.next().await {
        match event.event.as_str() {
            "content_block_delta" => { /* append text delta */ }
            "thinking" => { /* append to thinking block */ }
            "message_stop" => { break; }
            "error" => { break; }
            _ => {}
        }
    }
}
```

---

## 7. State Model

```rust
pub struct App {
    pub mode: AppMode,
    pub gateway_url: String,
    pub messages: Vec<ChatMessage>,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub scroll_offset: usize,
    pub streaming_buffer: Option<StreamingMessage>,
    pub agents: Vec<Agent>,
    pub current_agent: String,
    pub current_session: Option<String>,
    pub sessions: Vec<SessionSummary>,
    pub show_thinking: bool,
    pub overlay: Option<OverlayState>,
    pub terminal_size: (u16, u16),
    pub status_message: Option<(String, Instant)>,
}

pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub thinking: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub token_count: Option<u32>,
}

pub enum AppMode { Normal, Streaming, Overlay(OverlayKind) }
pub enum OverlayKind { AgentPicker, SessionBrowser, Help, CostSummary, SystemStatus }
```

---

## 8. Message Rendering

### Markdown Subset

| Element           | Rendering |
|-------------------|-----------|
| `**bold**`        | `Style::new().bold()` |
| `*italic*`        | `Style::new().italic()` |
| `` `inline code` `` | `Style::new().fg(Color::Yellow).bg(Color::DarkGray)` |
| ```` ```code blocks``` ```` | Bordered block with `syntect` highlighting (top 10 languages) |
| `# Headings`      | Bold + underline |
| `- lists`         | Indented with bullet prefix |
| `> blockquote`    | Dim + left border character |
| Links             | Underlined, URL in parentheses |

Use `pulldown-cmark` to parse, then map events to `ratatui::text::Line` / `Span` sequences.

### Thinking Blocks

Collapsible. Collapsed: `▶ thinking (42 tokens)`. Expanded (`Ctrl+T`): full content in dim/italic with left border.

### Streaming

Append chunks to `StreamingMessage` buffer. Render with blinking cursor `▌`. Re-parse markdown debounced (every 100ms max).

---

## 9. Configuration

```toml
# ~/.config/aletheia/tui.toml

[gateway]
url = "http://localhost:18789"

[defaults]
agent = "curator"
show_thinking = false

[theme]
# Terminal-native — respects terminal colorscheme by default
```

CLI overrides:

```bash
aletheia-tui
aletheia-tui --url http://10.0.0.5:18789
aletheia-tui --agent researcher
aletheia-tui --session <id>
```

---

## 10. Directory Structure

```
tui/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs
│   ├── app.rs
│   ├── event.rs
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── chat.rs
│   │   ├── input.rs
│   │   ├── status_bar.rs
│   │   ├── overlay.rs
│   │   └── markdown.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   ├── streaming.rs
│   │   └── types.rs
│   ├── config.rs
│   └── clipboard.rs
└── README.md
```

---

## 11. Implementation Phases

| Phase | What | Milestone |
|-------|------|-----------|
| 1 | Core chat: event loop, `/health` check, input, SSE streaming, message display | Send message, see streamed response |
| 2 | Agent & session management: picker overlay, session browser, new/reset session | Switch agents, browse sessions |
| 3 | Rich rendering: markdown subset, code highlighting, thinking blocks, scroll | Formatted responses with code |
| 4 | Polish: cost/status overlays, clipboard, config file, error handling, tests | Feature-complete for daily use |

---

## 12. Open Questions

1. **Authentication** — TUI needs bearer token path (not session cookies). Config file or env var.
2. **Multi-line input** — `Alt+Enter` more portable than `Shift+Enter`. Consider compose mode toggle.
3. **Live notifications** — Should the `/api/events` SSE stream update the TUI when other interfaces send messages?
4. **Binary distribution** — `cargo-dist` or cross-compilation, or build from source for now?
