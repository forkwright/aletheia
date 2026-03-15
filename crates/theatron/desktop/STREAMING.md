# Dioxus desktop streaming architecture

Research document for SSE and streaming patterns in the Dioxus desktop chat application.

## Dual-stream architecture

The desktop UI consumes two independent SSE streams, mirroring the TUI's proven pattern:

### Global SSE stream (`GET /api/v1/events`)

Cross-session awareness channel. Provides agent status changes, session lifecycle events, and memory distillation progress. Runs for the lifetime of the application.

**Events:** `init`, `turn:before`, `turn:after`, `tool:called`, `tool:failed`, `status:update`, `session:created`, `session:archived`, `distill:before`, `distill:stage`, `distill:after`, `ping`.

**Dioxus integration:** A `use_coroutine` at the app root consumes `SseConnection::next()` and writes into shared signals (agent list, connection indicator, notification badges).

**Implementation:** `src/api/sse.rs` provides `SseConnection`, a framework-agnostic struct that works with both the TUI event loop and Dioxus coroutines.

### Per-message fetch stream (`POST /api/v1/sessions/stream`)

Per-turn response channel. Carries LLM text deltas, tool invocations, plan execution, and completion signals. One stream per active turn; terminated when the turn completes, aborts, or errors.

**Events:** `turn_start`, `text_delta`, `thinking_delta`, `tool_start`, `tool_result`, `tool_approval_required`, `tool_approval_resolved`, `turn_complete`, `turn_abort`, `error`.

**Dioxus integration:** Initiated by `stream_turn()` when the user sends a message. A `use_coroutine` in the chat panel reads events and writes into the `StreamingState` signal via `ChatStateManager`.

**Implementation:** `src/api/streaming.rs` provides `stream_turn()`, which spawns a background task with `CancellationToken` support for user-initiated abort.

### Why two streams

| Concern | Global SSE | Per-message stream |
|---------|-----------|-------------------|
| Lifecycle | App lifetime | Single turn |
| Reconnection | Auto with backoff | Not applicable |
| Multiplexing | All agents | One agent, one session |
| Purpose | Dashboard awareness | Chat interaction |

Separating concerns means the global stream never carries high-frequency text deltas, and the per-message stream does not need reconnection logic (a failed turn shows an error).

## Signal-based stream state management

### Dioxus signal model

Dioxus uses `Signal<T>` for reactive state. Writing to a signal triggers re-render of all subscribing components. The key difference from the TUI:

- **TUI:** Event loop dispatches `Msg` variants to `update()` handlers that mutate `App` state. Dirty flag triggers full redraw.
- **Dioxus:** Signals are fine-grained. Writing `streaming_text.write()` only re-renders components that read `streaming_text`. No global dirty flag.

### Proposed signal layout

```rust
// App-level (provided via context)
let agents: Signal<Vec<Agent>> = use_signal(Vec::new);
let connection: Signal<ConnectionState> = use_signal(|| ConnectionState::Disconnected);

// Chat panel level
let messages: Signal<Vec<ChatMessage>> = use_signal(Vec::new);
let streaming: Signal<StreamingState> = use_signal(StreamingState::default);
let input_text: Signal<String> = use_signal(String::new);
```

### 100ms debounce for text deltas

Text deltas arrive at high frequency (every few tokens from the LLM). Writing to the signal on every delta causes excessive virtual DOM diffing.

**Strategy:** `ChatStateManager` (implemented in `src/components/chat.rs`) buffers deltas and flushes to the signal when:

1. 100ms has elapsed since the last flush, or
2. A newline character is received (users notice line breaks immediately), or
3. A non-delta event arrives (tool start, completion, etc.), or
4. A periodic tick fires (100ms interval from `use_coroutine`).

This is analogous to the TUI's 64-byte/newline markdown cache invalidation, but tuned for DOM diffing cost rather than terminal repainting.

### Coroutine pattern

```rust
use_coroutine(|_rx| {
    let streaming = streaming.clone();
    let messages = messages.clone();
    async move {
        let mut mgr = ChatStateManager::new();
        let mut state = ChatState::default();
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                biased;
                Some(event) = stream_rx.recv() => {
                    if mgr.apply(event, &mut state) {
                        streaming.set(state.streaming.clone());
                        messages.set(state.messages.clone());
                    }
                }
                _ = interval.tick() => {
                    if mgr.tick(&mut state) {
                        streaming.set(state.streaming.clone());
                    }
                }
            }
        }
    }
});
```

## Reconnection with exponential backoff

### Parameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Initial backoff | 1s | Fast first retry for transient failures |
| Max backoff | 30s | Caps growth to avoid long waits |
| Heartbeat timeout | 45s | Server sends pings every 30s; 50% margin |
| Channel buffer | 256 | Matches TUI; prevents backpressure under burst |

### Backoff sequence

1s, 2s, 4s, 8s, 16s, 30s, 30s, ...

On successful connection (`EsEvent::Open`), the backoff resets to 1s.

### Heartbeat timeout

The server sends `ping` events every 30 seconds on the global SSE channel. If 45 seconds pass with no event (including pings), the connection is treated as stale:

1. Close the `EventSource`.
2. Emit `SseEvent::Disconnected`.
3. Enter the reconnection loop with current backoff.

This is an increase from the TUI's 30-second read timeout. The TUI's aggressive timeout caused spurious reconnects under load; 45 seconds provides margin while still detecting genuinely hung connections within a minute.

### Cancellation-safe shutdown

The SSE connection loop uses `CancellationToken` from `tokio_util` (per workspace standards). All `sleep` and `next` calls are wrapped in `tokio::select!` with the cancel token as a biased first branch. This ensures clean shutdown without task leaks.

```rust
tokio::select! {
    biased;
    _ = child.cancelled() => return,
    result = tokio::time::timeout(HEARTBEAT_TIMEOUT, es.next()) => result,
}
```

## Abort support

### Per-turn cancellation

Each `stream_turn()` call accepts a `CancellationToken`. The background task checks this token in a biased `select!` alongside the event stream:

```rust
tokio::select! {
    biased;
    _ = cancel.cancelled() => {
        es.close();
        tx.send(StreamEvent::TurnAbort { reason: "cancelled by user" }).await;
        return;
    }
    event = es.next() => { /* process */ }
}
```

When the user clicks **Stop** in the UI, the component cancels the token. The background task:

1. Closes the `EventSource` (drops the HTTP connection).
2. Sends a `TurnAbort` event so the UI can finalize state.
3. Exits.

### UI binding

```rust
// In the chat component:
let cancel_token = use_signal(CancellationToken::new);

// Send button handler:
let token = CancellationToken::new();
cancel_token.set(token.clone());
let rx = stream_turn(client, &url, &agent, &key, &msg, token.child_token());

// Stop button handler:
cancel_token.read().cancel();
```

### Partial text preservation

When a turn is aborted, `ChatStateManager` preserves any partial text already generated. If the streaming text buffer is non-empty, it is committed to history as an assistant message. If no text was generated, no message is added. This matches user expectations: aborting mid-sentence keeps what was already visible.

## Comparison with TUI's existing SSE implementation

### Architecture

| Aspect | TUI (`theatron-tui`) | Desktop (`theatron-desktop`) |
|--------|---------------------|------------------------------|
| Event loop | `tokio::select!` over 3 sources (terminal, SSE, stream) + tick | `use_coroutine` per stream; Dioxus manages render cycle |
| State mutation | `Msg` enum dispatched to `update()` handlers | Direct signal writes from coroutine via `ChatStateManager` |
| Re-render trigger | Dirty flag in full terminal repaint | Signal subscription for targeted component re-render |
| Text buffering | 64-byte or newline markdown cache invalidation | 100ms or newline debounce on signal writes |
| Abort | Not implemented (TUI has no stop button) | `CancellationToken` per turn with `select!` |
| Shutdown | App exit drops tasks | `CancellationToken` hierarchy with graceful drain |
| State machine | Scattered across `update/sse.rs` and `update/streaming.rs` handlers | Centralized in `ChatStateManager::apply()` |

### SSE connection

| Aspect | TUI | Desktop |
|--------|-----|---------|
| Struct | `SseConnection` (no cancel token) | `SseConnection` (with `CancellationToken`) |
| Heartbeat timeout | 30s (`READ_TIMEOUT`) | 45s (`HEARTBEAT_TIMEOUT`) |
| Backoff range | 1s to 30s | 1s to 30s (identical) |
| Channel buffer | 256 | 256 (identical) |
| Shutdown mechanism | Task dropped on app exit | `CancellationToken` for graceful shutdown |
| Error surface | Toast notification | Signal-driven error banner component |
| Sleep/next cancellation | Not cancel-safe (no `select!` around sleep) | All waits wrapped in `select!` with cancel branch |

### Per-message streaming

| Aspect | TUI | Desktop |
|--------|-----|---------|
| Function | `stream_message()` | `stream_turn()` |
| Cancel support | None | `CancellationToken` in `select!` |
| Error recovery | Shows toast, user retries manually | Sets `StreamingState.error`, user retries |
| Text buffer | Immediate append to `app.streaming_text` | Debounced through `ChatStateManager` |
| Turn finalization | `handle_stream_turn_complete()` updates history | `ChatStateManager::apply(TurnComplete)` commits to `messages` |

### Event parsing

Both use the same `parse_sse_event()` and `parse_stream_event()` functions with identical event type mapping. The parsing logic was copied rather than extracted to `theatron-core` to avoid premature abstraction. The TUI and desktop may diverge as the desktop gains features the TUI lacks (rich media rendering, inline tool approval dialogs).

### What could be shared

If both UIs stabilize on identical parsing logic, candidates for extraction to `theatron-core`:

1. `parse_sse_event()` and `parse_stream_event()` functions.
2. `SseEvent` and `StreamEvent` enums.
3. `TurnOutcome`, `ActiveTurn`, and other serde types.
4. ID newtypes (`NousId`, `SessionId`, `TurnId`, `ToolId`).
5. `extract_error_message()` utility.

This extraction is deferred until the desktop UI is feature-complete and the shared surface is validated.

## Prototype component structure

The prototype lives in `src/components/chat.rs` and implements the full state machine without a Dioxus dependency. The module provides:

- **`ChatState`**: Complete chat state (messages, streaming, connection, agent, session).
- **`ChatStateManager`**: Event processor with 100ms debounce buffer for text/thinking deltas.
- **`ChatMessage`** and **`MessageRole`**: Committed conversation history entries.
- **`apply_connection_event()`**: Maps SSE connect/disconnect to `ConnectionState` signal updates.

The state machine handles the full turn lifecycle:

```text
Idle -[TurnStart]-> Streaming -[TextDelta*]-> Streaming
                                |                  |
                      [ToolStart]-> Tool Active -[ToolResult]-> Streaming
                                |
                      [TurnComplete]-> Idle (message committed)
                      [TurnAbort]---> Idle (partial text preserved)
                      [Error]-------> Error (streaming stopped)
```

Every state transition is covered by unit tests in the module, including a full turn lifecycle test that exercises: user message, turn start, text deltas, tool call, more text, turn complete, and final state verification.

## Open questions

1. **Markdown rendering:** The TUI uses `pulldown-cmark` with ratatui spans. Dioxus can render HTML natively. Should we parse markdown to HTML on each signal write, or use a Dioxus markdown component that subscribes to the raw text signal?

2. **Virtual scrolling:** The TUI implements its own virtual scroll for long conversations. Dioxus has `use_virtual_list` or CSS-based approaches. The streaming text append pattern needs to work with whichever scroll strategy is chosen.

3. **Multi-agent view:** The TUI's sidebar shows all agents with status badges driven by global SSE. The desktop version needs the same, but with richer UI (avatars, progress bars). The signal layout supports this via `agents: Signal<Vec<Agent>>`.

4. **Offline/reconnection UX:** The TUI shows a toast after five minutes of disconnection. The desktop should show a persistent banner with retry countdown, driven by `ConnectionState::Reconnecting { attempt }`.

5. **Shared type extraction timing:** The `theatron-core` crate is empty. When should the shared types be extracted? Proposed trigger: when the desktop reaches feature parity with the TUI on the streaming path and both parsers have been stable for two release cycles.
