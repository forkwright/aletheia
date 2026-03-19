# R1465: Computer Use Tool (Anthropic Computer Use API)

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1465

---

## Executive Summary

Anthropic's computer use API lets Claude control a desktop environment via `screenshot`, `key`, `type`, `mouse_move`, `left_click`, `right_click`, `middle_click`, `double_click`, and `left_click_drag` tool calls. Adding this to aletheia means implementing a `ComputerUseTool` in `crates/organon/` that captures screenshots and dispatches input events on the operator's machine (or a sandboxed VM).

**Recommendation: Implement with sandboxing guardrails.** The API is stable as of `claude-sonnet-4-5-20251022`. The primary design question is containment: computer use gives the model unrestricted desktop access, which conflicts with aletheia's sandbox model (Landlock + seccomp). A headless Xvfb + dedicated container is the correct default. Direct host display is an opt-in operator override.

---

## 1. Problem Statement

Current aletheia tools interact with the filesystem, shell, and HTTP. There is no way to automate GUI applications — browsers, desktop apps, visual workflows — that have no CLI or API surface. Computer use fills this gap: the model can navigate a web browser, fill forms, interact with Electron apps, and perform any task a human operator could do at a keyboard and mouse.

Use cases in scope for aletheia:
- Browser automation (login flows, CAPTCHA-free scraping, form submission)
- GUI testing of desktop applications
- Visual verification of rendered output (charts, PDFs, UI screenshots)
- Operator-delegated desktop tasks via agent session

---

## 2. Proposed Approach

### 2.1 API Model

Computer use is enabled by passing `computer_use` tools in the `tools` array of a `/v1/messages` request with `anthropic-beta: computer-use-2024-10-22`:

```json
{
  "type": "computer_20241022",
  "name": "computer",
  "display_width_px": 1280,
  "display_height_px": 800,
  "display_number": 1
}
```

Claude returns `tool_use` blocks with actions like:

```json
{ "type": "screenshot" }
{ "type": "left_click", "coordinate": [640, 400] }
{ "type": "type", "text": "hello world" }
{ "type": "key", "text": "Return" }
```

The executor captures a screenshot (PNG), encodes it as base64, and returns it as an `image` tool result. Input actions are dispatched via `xdotool` (X11) or `ydotool` (Wayland).

### 2.2 Tool Implementation

New tool in `crates/organon/src/builtins/computer_use.rs`:

```rust
pub struct ComputerUseTool {
    config: ComputerUseConfig,
    display: DisplayBackend,
}

pub enum DisplayBackend {
    Xvfb { display_number: u8, width: u16, height: u16 },
    HostDisplay { display: String },  // opt-in, operator config
}
```

The `ToolExecutor::execute` implementation dispatches on `action`:

| Action | Implementation |
|---|---|
| `screenshot` | `scrot -z -o /tmp/screenshot.png`; read + base64-encode |
| `key` | `xdotool key {text}` |
| `type` | `xdotool type --clearmodifiers -- {text}` |
| `mouse_move` | `xdotool mousemove {x} {y}` |
| `left_click` | `xdotool mousemove {x} {y} click 1` |
| `right_click` | `xdotool mousemove {x} {y} click 3` |
| `middle_click` | `xdotool mousemove {x} {y} click 2` |
| `double_click` | `xdotool mousemove {x} {y} click --repeat 2 1` |
| `left_click_drag` | `xdotool mousedown 1 mousemove {x2} {y2} mouseup 1` |

On Wayland, substitute `ydotool` for `xdotool`. The backend is detected from `$WAYLAND_DISPLAY`.

### 2.3 Sandboxed Display Environment

Default: spin up an `Xvfb` virtual framebuffer before the first computer use tool call.

```
Xvfb :99 -screen 0 1280x800x24 &
export DISPLAY=:99
```

The `ProcessGuard` wrapper (already in `organon`) manages the Xvfb lifetime. It is killed when the tool session ends.

Config section in `taxis`:

```toml
[computer_use]
enabled = false                  # opt-in at operator level
backend = "xvfb"                 # or "host"
display_width = 1280
display_height = 800
screenshot_dir = "/tmp/aletheia-screenshots"
max_actions_per_turn = 50        # circuit breaker
```

### 2.4 Security Model

Computer use is **disabled by default** and requires explicit opt-in (`computer_use.enabled = true` in `aletheia.toml`).

Threat surface:

| Risk | Mitigation |
|---|---|
| Model exfiltrates data via GUI clipboard | Xvfb isolation: clipboard is not shared with host |
| Keystroke injection into host applications | Xvfb display `:99` is isolated; host display requires explicit `backend = "host"` |
| Screenshot captures sensitive host content | Xvfb only shows what the agent launched inside it |
| Runaway action loops | `max_actions_per_turn` limit; session kill on threshold |
| Filesystem writes via GUI file dialogs | Landlock sandbox still applies to child processes |

The existing `organon` seccomp sandbox does **not** apply to the Xvfb subprocess — that process needs `fork`, `execve`, and X11 socket access. The `ProcessGuard` launches it outside the seccomp filter.

### 2.5 Integration with hermeneus

Computer use requires a specific model and beta header. The `ModelRequest` in `hermeneus` must:

1. Include `computer_use` in the `tools` array (alongside organon's other tools).
2. Add `anthropic-beta: computer-use-2024-10-22` to the request.
3. Handle `tool_use` blocks with `"name": "computer"` by routing to `ComputerUseTool` rather than the standard tool registry.

The beta header merges with any existing beta flags (e.g., `oauth-2025-04-20`). `hermeneus` already accepts a `Vec<String>` beta flags list; extend it.

### 2.6 Tool Registration

Register in `crates/organon/src/builtins/mod.rs`:

```rust
if config.computer_use.enabled {
    registry.register("computer", ComputerUseTool::new(config.computer_use.clone()));
}
```

The tool is **not** auto-activated. Agents must declare it in `TOOLS.md` or enable it via `enable_tool`.

---

## 3. Alternatives Considered

### 3.1 Playwright/Puppeteer Instead of Computer Use

Use the existing `exec` tool to run Playwright scripts for browser automation.

**Partially accepted.** Playwright is better for structured browser tasks (it understands DOM, not pixels). Computer use is complementary: it handles non-browser GUIs and visual verification. Both should exist.

### 3.2 VNC-Based Isolation

Run a full desktop in a Docker container with VNC, and have the tool connect remotely.

**Deferred.** More robust isolation but significant operational overhead (container lifecycle, image management, VNC auth). Suitable as a future "secure mode" for high-trust deployments.

### 3.3 macOS / Windows Support

Implement with `cliclick` (macOS) or `SendInput` (Win32) instead of `xdotool`.

**Out of scope.** Aletheia targets Linux. Cross-platform abstraction can be added later behind a feature flag.

---

## 4. Open Questions

1. **Beta header expiry:** `computer-use-2024-10-22` may be superseded. Monitor Anthropic changelog for a stable GA header. Is there a `computer-use-2025-*` already?

2. **Model compatibility:** Which models support computer use? Confirmed: `claude-sonnet-4-5-20251022` and newer. Does `claude-opus-4-6` support it?

3. **Screenshot latency:** `scrot` adds ~50ms per screenshot. For tight action loops this compounds. Investigate `ffmpeg` framebuffer capture or `xwd` as faster alternatives.

4. **Cursor visibility:** Xvfb does not render a cursor. Some UI interactions depend on cursor position feedback (tooltips, hover states). Is this a practical limitation?

5. **Audio:** Some workflows require audio feedback (e.g., browser media). Xvfb has no audio. Is `pulseaudio` + virtual sink needed?

6. **Multi-monitor:** The API supports a single `display_number`. If an operator needs multi-monitor simulation, is multiple-tool-instance registration the approach?

7. **Clipboard isolation:** Confirm that X11 clipboard (`CLIPBOARD` and `PRIMARY` selections) is fully isolated between Xvfb `:99` and the host display.

---

## 5. Implementation Sketch

```
crates/organon/src/builtins/
  computer_use.rs     # ComputerUseTool: screenshot + input dispatch
  xvfb.rs             # Xvfb lifecycle management via ProcessGuard

crates/taxis/src/config.rs
  # ComputerUseConfig struct + AletheiaConfig field

crates/hermeneus/src/
  # Add beta flag merge for computer-use header
  # Route "computer" tool_use blocks to ComputerUseTool
```

---

## 6. References

- Anthropic computer use documentation (claude.ai)
- `anthropic-beta: computer-use-2024-10-22`
- xdotool man page
- RFC for ProcessGuard pattern: `crates/organon/src/sandbox/`
- Landlock + seccomp integration: `crates/organon/src/sandbox.rs`
