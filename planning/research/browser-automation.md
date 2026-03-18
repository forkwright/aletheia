# Browser Automation Tool

Research document for adding browser automation capabilities to organon (tools layer).

---

## Question

How should Aletheia expose browser automation to agents? What protocol, crate, and interface design best fits the existing organon architecture, security model, and "pure Rust" preference?

---

## Findings

### 1. Current tool system architecture

Organon (`crates/organon/`) implements a trait-based tool registry with sandbox enforcement.

**Core interface** (`src/registry.rs`):

```rust
pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}
```

**Registration**: Tools are registered as `(ToolDef, Box<dyn ToolExecutor>)` pairs in `ToolRegistry`. Each `ToolDef` carries a name, description, JSON Schema for parameters, a `ToolCategory`, and an `auto_activate` flag.

**Invocation**: The registry looks up a tool by `ToolInput.name`, calls `executor.execute()`, records duration and status in Prometheus metrics, and returns a `ToolResult` (text or content blocks including images).

**Sandboxing** (`src/sandbox.rs`): Two-layer enforcement for subprocess tools:
- **Landlock LSM**: Filesystem access rules (read/write/exec paths) applied via `Command::pre_exec()`
- **Seccomp BPF**: Blocks dangerous syscalls (`ptrace`, `mount`, `chroot`, etc.)
- Policy is built from `SandboxConfig` + workspace path + allowed roots

**Lazy activation**: Tools can be registered with `auto_activate: false` and enabled per-session via the `enable_tool` meta-tool. This keeps the default tool surface small.

**Result types**: `ToolResult` supports text, image blocks (base64), and document blocks. The `view_file` tool already returns base64-encoded screenshots and PDFs to the agent.

**Key observation**: The exec tool runs subprocesses with Landlock+seccomp sandboxing. A browser automation tool has two design paths: (a) implement as a native Rust tool using a browser crate, or (b) delegate to an external process (Playwright) via the existing exec infrastructure. Both paths can use the sandbox layer, but with different trade-offs.

### 2. browser automation approaches

#### 2.1 chrome devTools protocol via `chromiumoxide`

| Attribute | Value |
|---|---|
| Version | 0.9.1 (Feb 2026) |
| Downloads | 1.47M total, 873K recent |
| License | MIT OR Apache-2.0 |
| Async | Tokio-native |
| Protocol | CDP over WebSocket |
| Browsers | Chrome/Chromium only |

CDP gives direct access to Chrome internals: DOM, network, rendering, JavaScript execution, screenshots. `chromiumoxide` generates Rust bindings from Chrome's protocol definition files.

**Strengths**: Full async/await, direct CDP access for maximum control, auto-download Chromium via optional `fetcher` feature, actively maintained.

**Weaknesses**: Chrome-only. Generated CDP bindings are ~60K lines, causing long compile times. English-language Chrome required for port detection.

**Dependency weight**: ~20 runtime deps including reqwest, async-tungstenite (WebSocket), full tokio. Medium-heavy.

#### 2.2 chrome devTools protocol via `headless_chrome`

| Attribute | Value |
|---|---|
| Version | 1.0.21 (Feb 2026) |
| Downloads | 1.42M total, 591K recent |
| License | MIT |
| Async | **Synchronous only** |
| Protocol | CDP over WebSocket (sync tungstenite) |
| Browsers | Chrome/Chromium only |

**Strengths**: Simple API, element-level screenshots (JPEG/PNG), incognito and extension support.

**Weaknesses**: Synchronous. Blocks threads. Incompatible with Aletheia's tokio runtime without `spawn_blocking` wrappers. Chrome-only. Missing frames, file pickers, WebSocket inspection.

**Disqualifying factor**: No async support. Aletheia runs on tokio. Wrapping every call in `spawn_blocking` adds complexity and negates the library's simplicity advantage.

#### 2.3 webDriver protocol via `fantoccini`

| Attribute | Value |
|---|---|
| Version | 0.22.1 (Feb 2026) |
| Downloads | 2.90M total (highest) |
| License | MIT OR Apache-2.0 |
| Async | Tokio-native |
| Protocol | W3C WebDriver |
| Browsers | Any WebDriver-compatible (Chrome, Firefox, Safari, Edge) |

W3C WebDriver is a standardized protocol with broad browser support. `fantoccini` is maintained by Jon Gjengset (well-known Rust community member).

**Strengths**: Multi-browser, most downloaded, async/tokio-native, dedicated `Form` API, lighter dependency chain (hyper, not reqwest).

**Weaknesses**: Requires a separate WebDriver binary running (chromedriver, geckodriver). WebDriver is slower than CDP (HTTP round-trips per command). No direct CDP access for low-level features like network interception.

**Dependency weight**: ~13 runtime deps. Uses hyper directly. Lighter than chromiumoxide.

#### 2.4 webDriver protocol via `thirtyfour`

| Attribute | Value |
|---|---|
| Version | 0.36.1 (Jul 2025) |
| Downloads | 1.17M total |
| License | MIT OR Apache-2.0 |
| Async | Tokio-native |
| Protocol | W3C WebDriver (Selenium-compatible) |

**Strengths**: Most feature-complete WebDriver client (action chains, Shadow DOM, cookie management). Optional `selenium-manager` automates driver downloads.

**Weaknesses**: Repository ownership transferred (stability concern). Recent version yanks (0.36.0, 0.35.1). 8 months since last release. MSRV is "latest stable." Uses reqwest (heavier than fantoccini's hyper).

**Concern**: Repository transfer and version yanks suggest maintenance uncertainty.

#### 2.5 HTTP-Only approach (reqwest + scraper)

| Attribute | Value |
|---|---|
| `scraper` version | 0.25.0 (Dec 2025) |
| Downloads | 14.65M total |
| License | ISC |

Static HTML parsing with CSS selectors. No browser binary, no JavaScript execution.

**Strengths**: Lightest option (~6 deps). Pure Rust. No browser process management. Deterministic.

**Weaknesses**: No JavaScript execution. Cannot handle SPAs, dynamic content, authentication flows requiring JS, or screenshots.

**Use case**: Static content extraction only. Already partially covered by the existing `web_fetch` tool in organon's research builtins.

#### 2.6 external process delegation (Playwright via shell)

Shell out to Playwright (Node.js or Python) for browser control.

**Strengths**: Full browser support (Chromium, Firefox, WebKit). Battle-tested. Auto-wait, network interception, tracing, video recording. Handles browser binary management. Large community.

**Weaknesses**: Requires Node.js or Python runtime. IPC overhead (serialize/deserialize across process boundary). Startup latency (runtime cold start + browser launch). Memory overhead (separate interpreter process). Version management complexity. Testing requires mocking the external process.

**Mitigation pattern**: Use a long-lived Playwright process with JSON-RPC or stdio protocol, not one shell invocation per action. Wrap in a Rust trait so the implementation can be swapped later.

### 3. approach comparison

| Factor | chromiumoxide | headless_chrome | fantoccini | thirtyfour | scraper+reqwest | Playwright shell |
|---|---|---|---|---|---|---|
| Async/Tokio | Yes | **No** | Yes | Yes | N/A | External |
| Multi-browser | No | No | Yes | Yes | N/A | Yes |
| Screenshots | Yes | Yes | Limited | Yes | No | Yes |
| Form interaction | Yes | Yes | Yes | Yes | No | Yes |
| JS execution | Yes | Yes | Yes | Yes | No | Yes |
| Runtime deps | ~20 | ~14 | ~13 | ~18 | ~6 | Node.js/Python |
| External binary | Chrome | Chrome | Chrome+driver | Chrome+driver | None | Browser+runtime |
| Maintenance | Active | Active | Active | Uncertain | Active | Active |
| Pure Rust | Yes | Yes | Yes | Yes | Yes | **No** |
| Compile impact | Heavy (codegen) | Moderate | Light | Moderate | Light | None |

### 4. tool interface design

Browser automation should be exposed as a single tool with an `action` parameter, not as separate tools per operation. This matches the pattern used by Anthropic's computer use tool and keeps the tool surface area small.

#### Operations

| Action | Parameters | Returns |
|---|---|---|
| `move through` | `url: String` | Page title, final URL (after redirects) |
| `screenshot` | `selector?: String` | Base64 PNG image block |
| `read_text` | `selector?: String` | Extracted text content |
| `read_html` | `selector?: String` | Raw HTML of element or page |
| `click` | `selector: String` | Success/failure |
| `type_text` | `selector: String, text: String` | Success/failure |
| `select` | `selector: String, value: String` | Success/failure |
| `wait` | `selector?: String, timeout_ms?: u64` | Success/timeout |
| `execute_js` | `script: String` | Script return value as JSON |
| `list_elements` | `selector: String` | Array of `{tag, text, attributes}` |
| `cookies` | `action: "get" \| "set" \| "clear", ...` | Cookie data |
| `back` / `forward` / `refresh` | (none) | New page state |

#### Tool definition

```rust
ToolDef {
    name: "browser",
    description: "Interact with web pages: move through, read content, fill forms, take screenshots",
    category: ToolCategory::Research,
    auto_activate: false,  // Lazy: enabled per-session when needed
    input_schema: InputSchema {
        properties: indexmap! {
            "action" => PropertyDef { type: String, enum: [...], required: true },
            "url" => PropertyDef { type: String },
            "selector" => PropertyDef { type: String },
            "text" => PropertyDef { type: String },
            "timeout_ms" => PropertyDef { type: Integer, default: 10000 },
            "script" => PropertyDef { type: String },
        },
        required: vec!["action"],
    },
}
```

#### Result format

- **Text content**: Returned as plain text in `ToolResult::text()`
- **Screenshots**: Returned as `ToolResult::blocks()` with `ToolResultBlock::Image` (base64 PNG). The agent sees the screenshot inline, matching how `view_file` works today.
- **Structured data**: JSON-serialized into `ToolResult::text()`. The agent parses as needed.
- **Errors**: `ToolResult::error()` with descriptive message (element not found, timeout, navigation failure).

#### Authentication and cookies

Browser sessions are ephemeral by default. Each `browser` tool activation starts a fresh browser profile with no cookies or stored credentials. Persistence options:

1. **Session-scoped**: Cookies persist within a single agent session (browser stays alive between tool calls). This is the default.
2. **Explicit cookie injection**: Agent uses `cookies` action with `set` to inject authentication cookies received from other sources.
3. **No credential storage**: Browser tool never stores credentials to disk. Session ends, cookies are gone.

### 5. security model

#### Domain allowlist

The browser tool must restrict which domains the agent can move through to. Configuration:

```toml
[tools.browser]
allowed_domains = ["docs.rs", "crates.io", "github.com"]
# Empty list = all domains allowed (operator must explicitly opt in)
block_private_networks = true  # Block 10.x, 172.16-31.x, 192.168.x, 169.254.x, localhost
```

Domain checking happens before navigation. Redirects to blocked domains are intercepted and rejected.

#### Resource limits

| Limit | Default | Purpose |
|---|---|---|
| Session timeout | 300s | Kill browser after inactivity |
| Page load timeout | 30s | Abort slow pages |
| Max concurrent pages | 3 | Prevent tab explosion |
| Max screenshots per session | 50 | Bound image token cost |
| Max response size | 1 MB | Prevent memory exhaustion from large pages |

#### Process isolation

The browser process runs under the same Landlock+seccomp sandbox as the exec tool. Additional constraints:

- **Filesystem**: Browser gets write access only to its temp profile directory. No access to workspace files.
- **Network**: Filtered by domain allowlist at the tool level (before the request reaches the browser).
- **GPU**: Disabled (`--disable-gpu`). Headless mode only.
- **Extensions**: Disabled. No user data directory.

#### Content risks

- **Prompt injection via page content**: Web pages can contain text that looks like instructions to the agent. The tool should strip or truncate excessively long page content and warn if the page contains patterns that resemble prompt injection (e.g., "ignore previous instructions").
- **Drive-by downloads**: Browser profile is ephemeral and sandboxed. Downloads go to a temp directory that is deleted on session end.
- **Exfiltration**: Domain allowlist prevents the agent from navigating to attacker-controlled domains. The `execute_js` action is the highest-risk operation and should be gated behind a separate permission flag.

#### Sandbox integration

```
[Agent Session]
    |
    v
[browser tool executor]  -- validates domain, enforces limits
    |
    v
[Browser process]  -- Landlock: temp profile only
                   -- Seccomp: standard dangerous-syscall block
                   -- No GPU, no extensions, no user data
                   -- Headless Chrome with --no-sandbox (Landlock replaces Chrome's internal sandbox)
```

Chrome's internal sandbox (`--no-sandbox`) is disabled because Landlock provides equivalent filesystem isolation at the OS level. This avoids the nested-sandbox complexity and user namespace requirements.

### 6. relationship to computer use

Anthropic's computer use tool and browser automation serve different purposes:

| Aspect | Browser automation | Computer use |
|---|---|---|
| Scope | Web pages only | Any GUI application |
| Input | DOM selectors, URLs | Pixel coordinates on screen |
| Output | Text, structured data, targeted screenshots | Full-screen screenshots |
| Precision | High (CSS selectors target exact elements) | Lower (coordinate estimation) |
| Token cost | Low (text extraction is cheap) | High (every step requires a screenshot) |
| Speed | Fast (direct protocol commands) | Slow (screenshot-analyze-act loop) |
| Dependency | Browser binary | Full desktop environment (X11/Wayland) |

For web-specific tasks, browser automation is more reliable, cheaper, and faster. Computer use is needed when the task involves non-browser GUI applications. The two can coexist: use the browser tool for web tasks, computer use for desktop GUI tasks.

### 7. observations

- **Debt**: The existing `web_fetch` tool (`builtins/research.rs`) does HTTP-only fetching. Adding browser automation creates overlap. Consider deprecating `web_fetch` in favor of `browser` with a `read_text` action, or keeping `web_fetch` as the lightweight path for static content.
- **Idea**: A `browser_pool` service (similar to database connection pooling) could manage browser lifecycle across sessions, amortizing startup cost.
- **Idea**: Screenshot diffing between browser actions could reduce token cost by only sending the changed region to the agent.
- **Missing test**: The sandbox module has no integration tests for Landlock enforcement on browser-like processes (long-lived, network-active, writing to temp directories).

---

## Recommendations

### Recommended approach: `chromiumoxide` (CDP)

**Primary**: Use `chromiumoxide` for browser automation behind a feature flag.

**Justification**:

1. **Tokio-native**: Matches Aletheia's async runtime. No `spawn_blocking` wrappers needed.
2. **CDP over WebDriver**: CDP is faster (WebSocket vs HTTP round-trips), supports screenshots natively, and gives access to network interception for future features.
3. **No external driver process**: WebDriver approaches (fantoccini, thirtyfour) require a separate chromedriver/geckodriver process to be running. CDP connects directly to Chrome's debugging port.
4. **Pure Rust**: No Node.js or Python dependency. Stays within the project's dependency philosophy.
5. **Active maintenance**: Recent release (Feb 2026), edition 2024, Rust 1.85 MSRV.
6. **Auto-download**: Optional `fetcher` feature manages Chromium binary, simplifying deployment.

**Trade-off accepted**: Chrome-only. Multi-browser support (fantoccini) is not worth the operational complexity of managing WebDriver binaries. Chrome covers the practical use cases for agent web interaction.

**Trade-off accepted**: Compile time increase from generated CDP bindings. Mitigated by putting the browser tool behind a feature flag so it does not affect default builds.

### Fallback: Playwright delegation

If `chromiumoxide` proves insufficient (stability issues, missing CDP features), fall back to Playwright delegation via a long-lived subprocess with stdio JSON protocol. This path:
- Adds a Python/Node.js runtime dependency
- Requires IPC protocol design
- But gains Playwright's battle-tested reliability and multi-browser support

Design the `ToolExecutor` implementation behind a trait so the backend (chromiumoxide vs Playwright) can be swapped without changing the tool interface.

### Implementation plan

#### Phase 1: core tool (Medium)

Add `browser` tool with `move through`, `screenshot`, `read_text`, `click`, `type_text`, `wait` actions using `chromiumoxide`.

- New module: `crates/organon/src/builtins/browser.rs`
- Feature flag: `browser` in `crates/organon/Cargo.toml`
- Browser lifecycle: spawn headless Chrome on first `browser` tool call, reuse within session, kill on session end
- Domain allowlist from `ToolContext` or `ToolServices` configuration
- Lazy activation via `enable_tool`

Blast radius: `crates/organon/src/builtins/browser.rs` (new), `crates/organon/src/builtins/mod.rs`, `crates/organon/Cargo.toml`

#### Phase 2: security hardening (Small)

Add resource limits, domain validation, private network blocking, session timeout.

- Integrate with existing `SandboxPolicy` for browser process isolation
- Add configuration surface in `taxis` (config crate)
- Content truncation and prompt injection detection

Blast radius: `crates/organon/src/builtins/browser.rs`, `crates/taxis/src/tools.rs`

#### Phase 3: advanced actions (Small)

Add `execute_js`, `list_elements`, `cookies`, `read_html`, `select`, navigation actions.

- `execute_js` gated behind separate permission flag
- Cookie management for authentication flows

Blast radius: `crates/organon/src/builtins/browser.rs`

#### Phase 4: browser pool (Optional, future)

Connection pooling for browser instances across sessions. Only justified if browser startup latency becomes a measurable bottleneck.

### Effort estimate

| Phase | Scope | New deps |
|---|---|---|
| Phase 1 | 1 new file, ~400 lines, Cargo.toml changes | `chromiumoxide` (behind feature flag) |
| Phase 2 | Same file + config, ~150 lines | None |
| Phase 3 | Same file, ~200 lines | None |
| Phase 4 | New service, ~300 lines | None |

---

## Gotchas

1. **Chrome binary availability.** Headless Chrome must be installed on the host or auto-downloaded via `chromiumoxide_fetcher`. The feature flag approach means builds without `browser` have zero Chrome dependency, but deployments that enable it must ensure Chrome is present. Document the requirement and provide a Nix flake overlay.

2. **Landlock + Chrome sandbox conflict.** Chrome's internal sandbox uses user namespaces and seccomp-bpf. Running Chrome under Landlock with `--no-sandbox` disables Chrome's own sandbox, relying on Landlock for isolation instead. This is safe (Landlock is stricter) but non-obvious. Document the security rationale.

3. **CDP port detection.** `chromiumoxide` finds Chrome's debugging port by parsing stderr. Chrome must output English-language messages for this to work. Set `LANGUAGE=en` in the browser process environment.

4. **Headless Chrome memory.** Even headless, Chrome consumes 100-300 MB per page. With 3 concurrent pages (the recommended limit), a browser session uses up to 1 GB. Resource limits must be enforced to prevent OOM on constrained hosts.

5. **Compile time.** `chromiumoxide` generates CDP bindings from protocol definition files. First compilation adds 30-60 seconds. The feature flag keeps this cost out of default builds, but CI must test the `browser` feature explicitly.

6. **Content extraction quality.** `read_text` via CDP returns the DOM's `innerText`, which may include hidden elements, ad text, or navigation chrome. Consider a heuristic (similar to `readability` algorithms) to extract the main content. This is a future enhancement, not a phase 1 requirement.

7. **WebSocket lifecycle.** CDP uses a persistent WebSocket connection. If the agent pauses for extended periods (waiting for LLM response), the connection may time out. Implement reconnection logic or periodic keepalive pings.

8. **Token cost from screenshots.** A 1024x768 screenshot costs ~1,050 tokens. Agents that screenshot every action will burn through context quickly. Consider returning text-based page summaries by default and only screenshots when explicitly requested.

---

## References

- `chromiumoxide`: https://crates.io/crates/chromiumoxide (0.9.1, Feb 2026)
- `fantoccini`: https://crates.io/crates/fantoccini (0.22.1, Feb 2026)
- `headless_chrome`: https://crates.io/crates/headless_chrome (1.0.21, Feb 2026)
- `thirtyfour`: https://crates.io/crates/thirtyfour (0.36.1, Jul 2025)
- `scraper`: https://crates.io/crates/scraper (0.25.0, Dec 2025)
- Anthropic computer use API: beta tool `computer_20251124`
- Chrome DevTools Protocol specification: https://chromedevtools.github.io/devtools-protocol/
- W3C WebDriver specification: https://www.w3.org/TR/webdriver2/
- Organon tool system: `crates/organon/src/registry.rs`, `crates/organon/src/types.rs`
- Organon sandbox: `crates/organon/src/sandbox.rs`
- Existing web fetch tool: `crates/organon/src/builtins/research.rs`
