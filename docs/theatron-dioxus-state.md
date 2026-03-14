# Theatron Desktop: Dioxus State Architecture

> Research document for the Aletheia Dioxus desktop UI state model.
> Covers state primitives, per-agent chat state, reactive collections,
> generation tracking, TUI-to-Dioxus mapping, and key hooks.
>
> Reference implementation: `crates/theatron/tui/` (ratatui TUI).
> Target implementation: `crates/theatron/desktop/` (Dioxus 0.6).

---

## 1. Dioxus 0.6 State Primitives

Dioxus 0.6 provides three primary reactive state primitives. Each operates at a
different granularity and suits different ownership patterns.

### Signal\<T\>

A `Signal<T>` is a single reactive cell. Reading a signal inside a component
subscribes that component to changes. Writing triggers re-render of all
subscribers. Best for simple scalar values.

```rust
// Simple boolean toggle — all readers re-render on change.
let sidebar_visible = use_signal(|| true);

// Read (subscribes the component):
if sidebar_visible() {
    render_sidebar();
}

// Write (notifies all subscribers):
sidebar_visible.set(false);
```

**Characteristics:**
- Whole-value comparison: any `set()` triggers subscribers even if the inner
  value is unchanged (unless `PartialEq` short-circuit is used via `set_if_neq`).
- No field-level granularity: reading `signal().some_field` subscribes to the
  entire signal, not just that field.
- Cheapest primitive for leaf values (`bool`, `u32`, `Option<String>`).

### Store\<T\>

A `Store<T>` wraps a struct and provides **field-level subscriptions**. Reading
`store.field_name` subscribes only to that field. Writing one field does not
re-render components that only read other fields.

```rust
#[derive(Store, Clone)]
struct AgentChatState {
    messages: Vec<ChatMessage>,
    streaming_turn: StreamingTurn,
    error: Option<ChatError>,
    generation: u64,
}

let chat = use_store(|| AgentChatState::default());

// This component re-renders only when `streaming_turn` changes:
let turn = chat.streaming_turn();

// This component re-renders only when `messages` changes:
let msgs = chat.messages();
```

**Characteristics:**
- Field-level reactivity reduces unnecessary re-renders.
- Requires `#[derive(Store)]` on the state struct.
- Best for medium-to-large state structs where different components read
  different fields.
- Nested structs can also derive `Store` for deeper granularity.

### Context Providers

Context providers make state available to an entire subtree without prop
drilling. Any component in the subtree can `use_context::<T>()` to access it.

```rust
// At the app root:
use_context_provider(|| Signal::new(AppConfig::default()));
use_context_provider(|| Signal::new(ConnectionStatus::Connected));

// In any descendant:
let config = use_context::<Signal<AppConfig>>();
let status = use_context::<Signal<ConnectionStatus>>();
```

**Characteristics:**
- Tree-wide visibility; no explicit prop threading.
- Typically wraps a `Signal<T>` or `Store<T>`.
- Best for truly global state: config, theme, connection status, agent registry.

### Comparison Table

| Primitive | Granularity | Subscription Scope | Best For |
|-----------|------------|-------------------|----------|
| `Signal<T>` | Whole value | Any component that reads | Simple scalars, toggles, counters |
| `Store<T>` | Per field | Components reading specific fields | Per-agent chat state, complex structs |
| Context | Tree-wide access | Via `use_context` | App config, theme, connection status |

### Recommendation

| State Domain | Primitive | Rationale |
|-------------|-----------|-----------|
| Per-agent chat state | `Store<AgentChatState>` | Field-level subscriptions: message list changes don't re-render the streaming indicator |
| Simple UI toggles | `Signal<bool>` | Sidebar visibility, thinking expanded, auto-scroll |
| App-wide singletons | Context + `Signal<T>` | Connection status, daily cost, agent registry |
| Derived/computed values | `use_memo` | Filtered message counts, sorted agent lists |

---

## 2. Per-Agent Chat State Model

Each agent tab owns an `AgentChatState` stored as a `Store<T>`. This replaces
the TUI's monolithic `App` struct where streaming fields, messages, and scroll
state are all top-level fields swapped on tab switch.

**Reference:** `crates/theatron/desktop/src/state/chat.rs`

### Core Struct

```rust
#[derive(Store, Clone, Default)]
pub struct AgentChatState {
    /// Chat history for this agent's active session.
    pub messages: Vec<ChatMessage>,

    /// Current streaming turn state machine.
    pub streaming_turn: StreamingTurn,

    /// Live tool calls during the active turn.
    pub tool_calls: Vec<LiveToolCall>,

    /// Operations pane state (thinking text, tool results, diffs).
    pub ops: OpsState,

    /// Current error, if any.
    pub error: Option<ChatError>,

    /// Session ID for the active conversation.
    pub session_id: Option<SessionId>,

    /// Generation counter — incremented on agent/session switch.
    /// Used to discard stale API responses.
    pub generation: u64,

    /// Cost tracking for the active session.
    pub session_cost_cents: u32,
    pub context_usage_pct: Option<u8>,
}
```

### StreamingTurn Enum

Models the lifecycle of a single LLM turn as a state machine:

```rust
#[derive(Clone, Default, PartialEq)]
pub enum StreamingTurn {
    /// No active turn. The input box is editable.
    #[default]
    Idle,

    /// LLM is generating. Streaming text accumulates.
    Active {
        turn_id: TurnId,
        text: String,
        thinking: String,
        cached_markdown: Vec<Line<'static>>,
    },

    /// User requested abort. Waiting for server confirmation.
    Aborting { turn_id: TurnId },
}
```

**Transitions:**
```
Idle → Active       on TurnStart event
Active → Aborting   on user abort request
Active → Idle       on TurnComplete / TurnAbort / Error
Aborting → Idle     on TurnAbort confirmation from server
```

The `Active` variant holds all in-flight streaming data that the TUI currently
scatters across `app.streaming_text`, `app.streaming_thinking`,
`app.cached_markdown_text`, and `app.cached_markdown_lines`. Colocating these
in the enum variant ensures they are always created and destroyed together.

### LiveToolCall Lifecycle

Models an individual tool call within a turn:

```rust
#[derive(Clone, PartialEq)]
pub enum LiveToolCall {
    /// Tool is executing on the server.
    Running {
        tool_id: ToolId,
        name: String,
        started_at: Instant,
    },

    /// Server requests user approval before proceeding.
    AwaitingApproval {
        tool_id: ToolId,
        turn_id: TurnId,
        name: String,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },

    /// Tool finished (success or error).
    Completed {
        tool_id: ToolId,
        name: String,
        is_error: bool,
        duration_ms: u64,
    },
}
```

**Transitions:**
```
Running → AwaitingApproval   on ToolApprovalRequired event
Running → Completed          on ToolResult event
AwaitingApproval → Running   on user approval (ToolApprovalResolved)
AwaitingApproval → Completed on user rejection
```

This replaces the TUI's `ToolCallInfo` (a plain struct with optional fields)
and the `ToolApprovalOverlay` (a separate overlay struct). The enum makes
illegal states unrepresentable: a tool call cannot simultaneously be running
and awaiting approval.

### Abort Support

The TUI handles abort by clearing `app.stream_rx` and resetting
`app.active_turn_id` to `None`. In the Dioxus model, abort is a first-class
state:

1. User presses Escape or clicks abort during `StreamingTurn::Active`.
2. State transitions to `StreamingTurn::Aborting { turn_id }`.
3. The abort request is sent to the server via POST.
4. On `TurnAbort` event from the server, state returns to `Idle`.
5. If the server responds with `TurnComplete` instead (abort arrived too late),
   the response is committed normally and state returns to `Idle`.

The `Aborting` variant allows the UI to show a "cancelling..." indicator
without the ambiguity of checking multiple fields.

### ChatError Enum

```rust
#[derive(Clone, PartialEq, Snafu)]
pub enum ChatError {
    #[snafu(display("Connection lost: {message}"))]
    ConnectionLost { message: String },

    #[snafu(display("Stream error: {message}"))]
    StreamError { message: String },

    #[snafu(display("Server error {status}: {message}"))]
    ServerError { status: u16, message: String },

    #[snafu(display("Request timeout after {seconds}s"))]
    Timeout { seconds: u64 },
}
```

Replaces the TUI's `ErrorToast` (a string + timestamp). Structured errors
enable the UI to show different treatments: a reconnect button for
`ConnectionLost`, a retry button for `Timeout`, an inline error for
`StreamError`.

---

## 3. Reactive Collections

Standard `Vec<T>` inside a `Store` field triggers re-renders on any mutation,
even appending a single element. For large collections, this is wasteful.

**Reference:** `crates/theatron/desktop/src/state/collections.rs`

### ReactiveList\<T\>

A wrapper around `Vec<T>` with a version counter for granular change detection:

```rust
#[derive(Clone)]
pub struct ReactiveList<T> {
    items: Vec<T>,
    version: u64,
}

impl<T> ReactiveList<T> {
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.version += 1;
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.version += 1;
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
```

Components use `use_memo` keyed on `list.version()` to avoid recomputing
derived state (filtered views, rendered markdown) when the list has not
changed:

```rust
let messages = chat.messages();
let filtered = use_memo(move || {
    let msgs = messages();
    let ver = msgs.version();
    // Only recompute when version bumps
    msgs.items()
        .iter()
        .filter(|m| m.role == "assistant")
        .cloned()
        .collect::<Vec<_>>()
});
```

### File Tree: Flat Sorted Vec with Depth

File trees are stored as a flat `Vec<FileEntry>` sorted by path, with a depth
field for indentation. This avoids recursive tree structures that complicate
reactive updates.

```rust
#[derive(Clone, PartialEq)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub depth: u16,
    pub is_dir: bool,
    pub expanded: bool,
}
```

Expansion/collapse toggles the `expanded` flag and inserts/removes children
in-place. The flat structure maps directly to a virtualised list without tree
traversal.

### Entity List with Sort/Filter

The memory inspector's entity and fact lists use `ReactiveList<T>` with
separate sort and filter signals:

```rust
let facts = use_store(|| ReactiveList::<MemoryFact>::new());
let sort_key = use_signal(|| FactSort::Confidence);
let sort_asc = use_signal(|| false);
let filter_text = use_signal(|| String::new());

let visible_facts = use_memo(move || {
    let mut items = facts.items().to_vec();
    let filter = filter_text();
    if !filter.is_empty() {
        let lower = filter.to_lowercase();
        items.retain(|f| f.content.to_lowercase().contains(&lower));
    }
    items.sort_by(|a, b| match sort_key() {
        FactSort::Confidence => a.confidence.partial_cmp(&b.confidence).unwrap(),
        FactSort::Recency => a.last_accessed_at.cmp(&b.last_accessed_at),
        // ...
    });
    if !sort_asc() { items.reverse(); }
    items
});
```

Each signal (`sort_key`, `sort_asc`, `filter_text`) triggers recomputation
independently. Changing the sort order does not re-fetch from the API; it only
recomputes the memo.

---

## 4. History Generation Tracking

When the user switches agents or sessions, in-flight API responses from the
previous context must be discarded. The TUI handles this implicitly by
clearing `app.messages` on switch and trusting that `load_focused_session`
replaces the data. This is fragile with async responses: a slow history
response for agent A can arrive after the user has already switched to agent B.

The generation counter pattern makes this explicit.

### Pattern

```rust
impl AgentChatState {
    /// Increment the generation. Called on every agent/session switch.
    pub fn bump_generation(&mut self) {
        self.generation += 1;
    }

    /// Check whether a response belongs to the current generation.
    pub fn is_current_generation(&self, gen: u64) -> bool {
        self.generation == gen
    }
}
```

### Usage

```rust
// On agent switch:
chat.write().bump_generation();
let gen = chat.read().generation;

// Spawn async history load:
let chat_handle = chat.clone();
spawn(async move {
    let history = client.history(&session_id).await;
    // Only apply if the user hasn't switched away:
    if chat_handle.read().is_current_generation(gen) {
        chat_handle.write().messages = ReactiveList::from(history);
    }
});
```

The same pattern applies to:
- **SSE event routing:** Events carry `nous_id` and `session_id`. Before
  applying a `turn:after` event, check that the event's agent matches the
  current generation.
- **Streaming turn responses:** The `TurnStart` event captures the generation.
  Subsequent `TextDelta` events are discarded if the generation has advanced.
- **Cost queries:** `today_cost_cents` responses are applied unconditionally
  (they are global), but `session_cost_cents` checks generation.

This eliminates the class of bugs where stale data from a previous context
bleeds into the current view, which the TUI mitigates through synchronous
clearing in `load_focused_session` and `restore_from_active_tab`.

---

## 5. TUI State Model Mapped to Dioxus Idioms

The TUI's `App` struct (defined in `crates/theatron/tui/src/app.rs`) is a
monolithic state bag with 40+ fields. In Dioxus, this decomposes into
purpose-specific stores, signals, and context providers owned by the
appropriate component.

### What Disappears

These TUI concerns have no Dioxus equivalent because Dioxus handles them
natively:

| TUI Field/Concept | Why It Disappears |
|-------------------|-------------------|
| `dirty: bool`, `frame_cache: Option<Buffer>` | Dioxus has its own VDOM diffing; no manual dirty tracking |
| `terminal_width`, `terminal_height` | Dioxus layout engine (Taffy) handles sizing |
| `virtual_scroll: VirtualScroll` | Dioxus virtualised list components handle this |
| `pending_g: bool` (vim-style key chords) | Desktop uses standard key event handling, no chord state machine |
| `ViewStack` (stack-based navigation) | Dioxus Router with URL-based navigation |
| `tab_completion: Option<TabCompletion>` | Native text input with platform autocomplete |
| `scroll_offset`, `auto_scroll`, `scroll_states` | CSS `overflow-y: auto` + `scroll_into_view()` |
| `cached_markdown_text/lines` | Computed in `use_memo`; Dioxus caches automatically |
| `tick_count: u64` | No manual tick loop; reactivity is event-driven |
| `highlighter: Highlighter` | Kept but managed differently (web-based syntax highlighting or wasm) |

### What Stays (Restructured)

| TUI Field | Dioxus Form | Owner |
|-----------|-------------|-------|
| `agents: Vec<AgentState>` | `Signal<Vec<AgentState>>` in context | App root |
| `focused_agent: Option<NousId>` | `Signal<Option<NousId>>` in context | App root |
| `messages: ArcVec<ChatMessage>` | `Store<AgentChatState>.messages` | Per-tab `Store` |
| `streaming_text/thinking/tool_calls` | `StreamingTurn::Active { .. }` | Per-tab `Store` |
| `active_turn_id` | `StreamingTurn::Active { turn_id }` | Per-tab `Store` |
| `sse_connected`, `sse_disconnected_at` | `Signal<ConnectionStatus>` in context | App root |
| `sidebar_visible` | `Signal<bool>` | Layout component |
| `thinking_expanded` | `Signal<bool>` | Operations pane component |
| `overlay: Option<Overlay>` | `Signal<Option<Overlay>>` | App root |
| `daily_cost_cents` | `Signal<u32>` in context | App root |
| `session_cost_cents`, `context_usage_pct` | `Store<AgentChatState>` fields | Per-tab `Store` |
| `filter: FilterState` | `Signal<FilterState>` | Chat component |
| `ops: OpsState` | `Store<AgentChatState>.ops` | Per-tab `Store` |
| `tab_bar: TabBar` | `Signal<TabBar>` | Tab bar component |
| `memory: MemoryInspectorState` | `Store<MemoryInspectorState>` | Memory inspector route |
| `error_toast`, `success_toast` | `Signal<Option<Toast>>` in context | App root (toast layer) |
| `command_palette: CommandPaletteState` | `Signal<CommandPaletteState>` | Command palette component |
| `config: Config` | Context provider (read-only) | App root |

### What Is New

| Concept | Details |
|---------|---------|
| `Store<T>` field-level subscriptions | Components subscribe to individual fields, not the whole struct |
| Generation counter | Explicit staleness guard for async responses (see section 4) |
| `StreamingTurn` enum | Replaces scattered streaming fields with a state machine |
| `LiveToolCall` enum | Replaces `ToolCallInfo` struct + `ToolApprovalOverlay` with a unified lifecycle |
| `ChatError` enum | Structured errors replace string-based `ErrorToast` |
| `ReactiveList<T>` | Version-tracked collections for granular memo invalidation |
| Router-based navigation | Replaces `ViewStack` push/pop with URL segments |

### Component-State Ownership Sketch

```
App (root)
├── context: Signal<Vec<AgentState>>       // agent registry
├── context: Signal<Option<NousId>>        // focused agent
├── context: Signal<ConnectionStatus>      // SSE health
├── context: Signal<u32>                   // daily_cost_cents
├── context: Signal<Option<Toast>>         // toast notifications
├── context: Config                        // read-only config
│
├── TabBar
│   └── Signal<TabBar>                     // tab list + active index
│
├── Sidebar
│   ├── reads: context agents, focused_agent
│   └── Signal<bool> sidebar_visible
│
├── ChatPane (per tab)
│   ├── Store<AgentChatState>              // owns all per-agent state
│   │   ├── .messages                      // subscribed by MessageList
│   │   ├── .streaming_turn               // subscribed by StreamingIndicator
│   │   ├── .tool_calls                   // subscribed by ToolCallList
│   │   ├── .ops                          // subscribed by OperationsPane
│   │   ├── .error                        // subscribed by ErrorBanner
│   │   └── .generation                   // checked by async callbacks
│   ├── Signal<FilterState>               // local to chat
│   └── Signal<bool> auto_scroll          // local to chat
│
├── OperationsPane
│   ├── reads: Store<AgentChatState>.ops
│   ├── reads: Store<AgentChatState>.streaming_turn
│   └── Signal<bool> thinking_expanded
│
├── CommandPalette
│   └── Signal<CommandPaletteState>
│
├── OverlayLayer
│   └── Signal<Option<Overlay>>
│
└── Router
    ├── /                                  // Home (chat view)
    ├── /sessions/:agent_id                // Session picker
    ├── /memory                            // Memory inspector
    │   └── Store<MemoryInspectorState>
    └── /memory/facts/:fact_id             // Fact detail
```

---

## 6. Key Dioxus Hooks

### use_coroutine — SSE Connections

Long-lived async tasks that send events into the reactive system. Replaces the
TUI's `SseConnection` background task + `mpsc::Receiver` polling loop.

```rust
let sse = use_coroutine(|mut rx: UnboundedReceiver<SseCommand>| {
    let client = use_context::<ApiClient>();
    async move {
        loop {
            let mut es = EventSource::new(client.sse_url()).await;
            while let Some(event) = es.next().await {
                match event {
                    SseEvent::TurnBefore { nous_id, .. } => {
                        // Update agent status via context signal
                    }
                    SseEvent::Disconnected => {
                        // Set connection status to disconnected
                        break; // will reconnect via outer loop
                    }
                    _ => {}
                }
            }
            // Exponential backoff before reconnect
            tokio::time::sleep(backoff).await;
        }
    }
});
```

The coroutine also receives commands from the UI (e.g., `SseCommand::Reconnect`)
via its `UnboundedReceiver`.

### use_effect — Side Effects on Signal Changes

Runs a closure whenever its tracked signals change. Replaces the TUI's
imperative "if this field changed, do that" logic scattered across update
handlers.

```rust
// Auto-scroll to bottom when new messages arrive:
use_effect(move || {
    let msgs = chat.messages();
    let version = msgs.version();
    // Scroll the container to the bottom
    if auto_scroll() {
        scroll_container.set(ScrollPosition::Bottom);
    }
});

// Auto-show operations pane when streaming starts:
use_effect(move || {
    let turn = chat.streaming_turn();
    match turn {
        StreamingTurn::Active { .. } => ops_visible.set(true),
        StreamingTurn::Idle => ops_visible.set(false),
        _ => {}
    }
});
```

### use_memo — Cached Derived State

Computes a value from signals and caches it until inputs change. Replaces the
TUI's manual cache fields (`cached_markdown_text`, `cached_markdown_lines`,
`text_lower`).

```rust
// Rendered markdown, recomputed only when streaming text changes:
let rendered_markdown = use_memo(move || {
    let turn = chat.streaming_turn();
    match turn {
        StreamingTurn::Active { ref text, .. } => {
            markdown::render(text)
        }
        _ => vec![],
    }
});

// Filtered message count for the filter bar:
let match_count = use_memo(move || {
    let filter = filter_state();
    let msgs = chat.messages();
    if filter.text.is_empty() {
        return msgs.len();
    }
    let (pattern, inverted) = filter.pattern();
    msgs.items()
        .iter()
        .filter(|m| m.text_lower.contains(pattern) != inverted)
        .count()
});
```

### use_resource — Async Reactive Values

Fetches data asynchronously when dependencies change. Replaces the TUI's
`load_focused_session` imperative async call.

```rust
// Load chat history when the session changes:
let history = use_resource(move || {
    let session_id = chat.session_id();
    let gen = chat.generation();
    async move {
        let session_id = session_id?;
        let history = client.history(&session_id).await.ok()?;
        Some((history, gen))
    }
});

// Apply loaded history, checking generation:
use_effect(move || {
    if let Some(Some((history, gen))) = history.value().as_ref() {
        if chat.read().is_current_generation(*gen) {
            chat.write().messages = ReactiveList::from(history.clone());
        }
    }
});
```

`use_resource` automatically re-runs when `chat.session_id()` changes,
and the result is available as a reactive `Resource<T>` that components can
read.

---

## 7. Open Questions

The following items are out of scope for this document but noted for future
investigation:

- **Tab state persistence across app restarts.** The TUI's `TabBar` is
  ephemeral. Should the desktop app persist open tabs to disk (SQLite or
  local storage)?

- **Multi-window support.** Dioxus desktop supports multiple windows. Should
  agent tabs be detachable into separate OS windows? This complicates shared
  context providers.

- **Undo/redo for input state.** The TUI has no undo. The desktop text input
  can leverage platform undo stacks, but should `AgentChatState` mutations
  (e.g., message deletion) also be undoable?

- **Offline mode.** What happens when the gateway is unreachable? The
  `ConnectionStatus` signal can drive a read-only mode, but caching strategy
  for previously loaded conversations is undefined.

- **WebSocket vs SSE.** The TUI uses SSE (`/api/v1/events`). Dioxus desktop
  could use WebSocket for bidirectional communication (abort signals, typing
  indicators). This would change the `use_coroutine` connection model.

- **Optimistic updates.** When the user sends a message, should it appear in
  the message list immediately (optimistic) or only after the server confirms
  the turn start? The TUI waits for `TurnStart`. Optimistic updates improve
  perceived latency but require rollback on error.

- **State serialization for dev tools.** Dioxus has a devtools protocol.
  Should `AgentChatState` implement `Serialize` for inspection? This adds a
  trait bound to all nested types.

- **Memory inspector as a separate route vs inline pane.** The TUI uses a
  `ViewStack` push to show the memory inspector. The desktop could use a
  router route (`/memory`) or a slide-out panel. Router is cleaner but loses
  the "drill in from chat" context.
