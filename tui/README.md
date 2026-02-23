# Aletheia TUI

Terminal dashboard for [Aletheia](https://github.com/forkwright/aletheia) — a multi-agent orchestration system.

The TUI is the primary working interface alongside Signal (mobile). It connects to the Aletheia gateway over HTTP/SSE and provides a real-time multi-agent dashboard.

## Features

- **Multi-agent dashboard** — see all agents' status (idle/working/streaming/compacting) simultaneously
- **Real-time SSE streaming** — live tool calls, thinking blocks, markdown rendering
- **Syntax-highlighted code blocks** — via syntect (base16-ocean.dark)
- **Markdown rendering** — bold, italic, code, headings, lists, tables, blockquotes, rules
- **Tool approval/denial** — inline overlay when a tool requires human approval
- **Plan approval** — toggle individual steps, approve or cancel proposed plans
- **Message queuing** — send messages while an agent is mid-turn (yellow "queued ›" prompt)
- **@mention completion** — type `@` then Tab to cycle through agent names
- **Clipboard** — `Ctrl+Y` copies last response (native + OSC52 fallback for SSH)
- **Compose mode** — `Ctrl+E` opens `$EDITOR` for multi-line input
- **Mouse support** — scroll wheel in chat, click agents in sidebar
- **Responsive** — sidebar auto-hides below 60 columns
- **Theme system** — true color (24-bit), 256-color, and 16-color ANSI with auto-detection
- **Reconnection resilience** — exponential backoff SSE reconnect with state recovery
- **Error toasts** — transient error display with 5-second auto-dismiss

## Requirements

- Rust 1.85+ (edition 2024)
- A running Aletheia gateway (default: `http://localhost:18789`)

## Install

```bash
# From the repository root
cd tui
cargo install --path .

# Or build without installing
cargo build --release
# Binary at tui/target/release/aletheia-tui
```

## Configuration

Config file: `~/.config/aletheia/tui.toml`

```toml
url = "http://localhost:18789"
token = "your-bearer-token"
default_agent = "syn"
```

All values can be overridden via CLI flags or environment variables:

```bash
aletheia-tui --url http://host:18789 --token $ALETHEIA_TOKEN --agent syn
```

Environment variables: `ALETHEIA_URL`, `ALETHEIA_TOKEN`

## Keybindings

### Navigation
| Key | Action |
|-----|--------|
| `Ctrl+A` | Switch agent (overlay) |
| `Ctrl+F` | Toggle sidebar |
| `Ctrl+T` | Toggle thinking blocks |
| `Ctrl+I` | System status overlay |
| `Ctrl+N` | New session / topic |
| `F1` | Help overlay |

### Input
| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `@agent Tab` | Mention completion |
| `Ctrl+E` | Open $EDITOR |
| `Ctrl+Y` | Copy last response |
| `Ctrl+U` | Clear input |
| `Ctrl+W` | Delete word |
| `Up/Down` | Input history |

### Scroll
| Key | Action |
|-----|--------|
| `Shift+Up/Down` | Scroll line |
| `PgUp/PgDn` | Scroll page |
| `Mouse wheel` | Scroll chat |
| `Click sidebar` | Switch agent |

### During Turns
| Key | Action |
|-----|--------|
| `A` | Approve tool/plan |
| `D` | Deny tool call |
| `Space` | Toggle plan step |

### General
| Key | Action |
|-----|--------|
| `Ctrl+C` / `Ctrl+Q` | Quit |
| `Esc` | Close overlay |

## Architecture

Built on the [Elm Architecture](https://guide.elm-lang.org/architecture/) (TEA):

- **Model** — `App` struct holds all state
- **Message** — `Msg` enum for every possible event
- **Update** — `async fn update(&mut self, msg: Msg)` — state transitions
- **View** — `fn view(&self, frame: &mut Frame)` — pure rendering

Three multiplexed event sources via `tokio::select!`:
1. Terminal events (crossterm `EventStream`)
2. SSE global stream (agent status, turns, tools, distillation)
3. Tick timer (30fps for animations)

## Terminal Support

| Terminal | Color | Clipboard |
|----------|-------|-----------|
| GNOME Terminal | True color | OSC52 |
| iTerm2 | True color | Native + OSC52 |
| Kitty | True color | OSC52 |
| WezTerm | True color | OSC52 |
| Alacritty | True color | OSC52 |
| Ghostty | True color | OSC52 |
| Terminal.app | 256 color | Native |
| tmux | 256+ color | OSC52 passthrough |
| SSH | Depends on client | OSC52 |

## Logging

Logs to `~/.local/share/aletheia/tui.log` (daily rotation). Set `RUST_LOG=debug` for verbose output.

## License

Same as Aletheia — AGPL-3.0 (infrastructure) / Apache-2.0 (templates and tools).
