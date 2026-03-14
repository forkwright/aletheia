# theatron-core Extraction Plan

> Audit of `crates/theatron/tui` — file-level classification of every symbol,
> module-structure proposal, dependency analysis, and test strategy for
> extracting a reusable `theatron-core` crate.

---

## Background

`theatron-core` was scaffolded as a sibling crate to `theatron-tui` with the
intent of sharing API types and client code across multiple Aletheia UI
surfaces (TUI, future web, future IDE extension). Its `lib.rs` currently
contains only a module comment. All live code resides in `theatron-tui`, with
the entire `api/` subtree being the primary extraction target.

---

## File Inventory and Classification

### `api/client.rs` (642 lines) — **Move entirely to theatron-core**

`ApiClient` is a pure Aletheia REST client. It has no dependency on `ratatui`,
`crossterm`, or any TUI-specific type. Its only imports are `reqwest`, `snafu`,
`serde_json`, `tracing`, and the sibling `api::{error, types}` modules (both
also shared).

| Symbol | Classification | Rationale |
|---|---|---|
| `encode_path()` | **shared** | Pure URL-encoding utility. Zero deps. |
| `build_http_client()` | **shared** | Builds `reqwest::Client` with auth headers. Used by `SseConnection` and `stream_message` via `raw_client()`. |
| `ApiClient` struct | **shared** | Complete REST surface for all Aletheia API paths. No TUI state. |
| `ApiClient::new()` | **shared** | Construction — no TUI dep. |
| `ApiClient::token()` | **shared** | Token accessor for auth checks. |
| `ApiClient::raw_client()` | **shared** | Exposes the shared `reqwest::Client` to SSE and streaming callers. |
| `ApiClient::health()` | **shared** | Liveness check — no TUI dep. |
| `ApiClient::auth_mode()` | **shared** | Auth negotiation — no TUI dep. |
| `ApiClient::login()` | **shared** | Credential flow — no TUI dep. |
| `ApiClient::agents()` | **shared** | Loads `Vec<Agent>` — shared type. |
| `ApiClient::sessions()` | **shared** | Loads `Vec<Session>` — shared type. |
| `ApiClient::history()` | **shared** | Loads `Vec<HistoryMessage>` — shared type. |
| `ApiClient::create_session()` | **shared** | Returns `Session` — shared type. |
| `ApiClient::archive_session()` | **shared** | Void mutation — no TUI dep. |
| `ApiClient::unarchive_session()` | **shared** | Void mutation — no TUI dep. |
| `ApiClient::rename_session()` | **shared** | Void mutation — no TUI dep. |
| `ApiClient::abort_turn()` | **shared** | Void mutation — no TUI dep. |
| `ApiClient::approve_tool()` | **shared** | Tool approval — no TUI dep. |
| `ApiClient::deny_tool()` | **shared** | Tool denial — no TUI dep. |
| `ApiClient::approve_plan()` | **shared** | Plan approval — no TUI dep. |
| `ApiClient::cancel_plan()` | **shared** | Plan cancellation — no TUI dep. |
| `ApiClient::today_cost_cents()` | **shared** | Cost query — no TUI dep. |
| `ApiClient::compact()` | **shared** | Distillation trigger — no TUI dep. |
| `ApiClient::recall()` | **shared** | Memory recall query — no TUI dep. |
| `ApiClient::config()` | **shared** | Config load — no TUI dep. |
| `ApiClient::update_config_section()` | **shared** | Config update — no TUI dep. |
| `ApiClient::knowledge_facts()` | **shared** | Knowledge query — no TUI dep. |
| `ApiClient::knowledge_fact_detail()` | **shared** | Fact detail — no TUI dep. |
| `ApiClient::knowledge_forget()` | **shared** | Knowledge mutation — no TUI dep. |
| `ApiClient::knowledge_restore()` | **shared** | Knowledge mutation — no TUI dep. |
| `ApiClient::knowledge_update_confidence()` | **shared** | Knowledge mutation — no TUI dep. |
| `ApiClient::queue_message()` | **shared** | Message queuing — no TUI dep. |
| `check_auth()` / `check_status()` | **shared** | Response handling helpers. |

---

### `api/types.rs` (518 lines) — **Move entirely to theatron-core**

All types are pure domain models: deserialization targets for REST responses
and the SSE event vocabulary. No TUI dependency anywhere in the file.

| Symbol | Classification | Rationale |
|---|---|---|
| `Agent` | **shared** | REST deserialization target. |
| `Agent::display_name()` | **shared** | Pure string accessor. |
| `Session` | **shared** | REST deserialization target. |
| `Session::label()` | **shared** | Pure string accessor. |
| `Session::is_archived()` | **shared** | Business-logic predicate — no UI dep. |
| `Session::is_interactive()` | **shared** | Business-logic predicate — no UI dep. |
| `HistoryMessage` | **shared** | REST deserialization target. |
| `HistoryResponse` | **shared** | Response envelope. |
| `TurnOutcome` | **shared** | Stream terminal event payload. |
| `PlanStep` | **shared** | Plan domain model. |
| `Plan` | **shared** | Plan domain model. |
| `SseEvent` | **shared** | Core SSE event vocabulary. Referenced by `SseConnection`, `StreamEvent`, `Msg`, and `mapping.rs`. The canonical set of events the server emits. |
| `ActiveTurn` | **shared** | SSE Init payload. |
| `AuthMode` | **shared** | Auth negotiation response. |
| `LoginResponse` | **shared** | Credential response. |
| `CostSummary` / `AgentCost` | **shared** | Cost reporting types. |
| `DailyResponse` / `DailyEntry` | **shared** | Daily cost breakdown. |
| `AgentsResponse` / `SessionsResponse` | **shared** | Response envelopes. |

---

### `api/error.rs` — **Move entirely to theatron-core**

| Symbol | Classification | Rationale |
|---|---|---|
| `ApiError` enum | **shared** | HTTP transport errors only. Depends solely on `reqwest::Error`. |
| `Result<T>` alias | **shared** | Convenience alias for `ApiError`. |

---

### `api/sse.rs` — **Move entirely to theatron-core**

The SSE connection manager. Depends on `reqwest`, `reqwest-eventsource`,
`tokio::sync::mpsc`, and `tracing`. Its only domain dependencies are
`SseEvent` (shared) and the five ID newtypes (shared after `id.rs` moves).

| Symbol | Classification | Rationale |
|---|---|---|
| `READ_TIMEOUT` | **shared** | Protocol constant — server-side ping frequency assumption. |
| `SseConnection` | **shared** | Background task + channel pair. Zero TUI dep after IDs move. |
| `SseConnection::connect()` | **shared** | Takes `reqwest::Client` + base URL. No TUI arg. |
| `SseConnection::next()` | **shared** | `async` recv wrapper. |
| `parse_sse_event()` | **shared** | JSON → `SseEvent` mapping. Pure, no TUI dep. |
| `str_field()` | **shared** | Helper for field extraction with tracing warning. |

---

### `api/streaming.rs` — **Move entirely to theatron-core**

Streams POST `/api/v1/sessions/stream`. Depends on `reqwest-eventsource`,
`tokio::sync::mpsc`, `tracing`, and `crate::events::StreamEvent`. `StreamEvent`
itself has no TUI dependency (see `events.rs` below) so the whole file moves.

| Symbol | Classification | Rationale |
|---|---|---|
| `stream_message()` | **shared** | Transport + parse only. No TUI dep once `StreamEvent` moves. |
| `parse_stream_event()` | **shared** | JSON → `StreamEvent` mapping. Pure. |
| `str_field()` | **shared** | Same helper as in `sse.rs` — can be deduplicated in core. |

---

### `api/mod.rs` — **Rewrite for theatron-core**

The current file re-exports the four submodules. After extraction, `theatron-core` gets a new `api/mod.rs` with the same re-export surface; `theatron-tui`'s `api/` directory is removed entirely and replaced with `use theatron_core::api::*`.

---

### `id.rs` — **Move entirely to theatron-core**

`NousId`, `SessionId`, `TurnId`, `ToolId`, `PlanId` are domain IDs. They
depend only on `compact_str` and `serde`. They are currently TUI-local but are
referenced by `api/types.rs`, `api/sse.rs`, `api/streaming.rs`, `events.rs`,
`msg.rs`, `app.rs`, `state/*`, and `mapping.rs`.

After moving, `theatron-tui` imports `theatron_core::id::*` and removes its
local `id.rs`.

---

### `events.rs` — **Split: `StreamEvent` → theatron-core, `Event` stays**

| Symbol | Classification | Rationale |
|---|---|---|
| `StreamEvent` enum | **shared** | Parsed streaming protocol vocabulary. Depends only on `Plan`, `TurnOutcome` (both shared), and the ID newtypes (shared). Zero TUI dep. |
| `Event` enum | **TUI-specific** | Wraps `crossterm::event::Event` — hard dependency on crossterm. Stays in `theatron-tui`. |

After extraction: `StreamEvent` moves to `theatron_core::api::streaming` (or
`theatron_core::stream`) alongside `stream_message`. `Event` stays in
`theatron-tui/src/events.rs` and imports `StreamEvent` from `theatron_core`.

---

### `config.rs` — **Stays in theatron-tui**

TUI-specific configuration: server URL, Bearer token, default agent/session,
workspace root. Reads from `~/.config/aletheia/tui.toml` using `dirs` and
`toml`. A future web client would have its own config surface.

---

### `error.rs` — **Stays in theatron-tui**

TUI-specific error enum. Wraps `ApiError` via `#[snafu(context(false))]` plus
IO, TOML, tracing-subscriber, and TUI-specific variants (`GatewayUnreachable`,
`TokenRequired`, `ProtocolMismatch`).

---

### `app.rs` — **Stays in theatron-tui**

The `App` struct is the root of TUI application state. Depends on `ratatui`,
`crossterm`, `state/*`, `view/*`, and `update/*`. Pure TUI orchestration.

---

### `msg.rs` — **Stays in theatron-tui**

`Msg` is the application message type: every possible state transition for the
TUI event loop. Deep dependency on ID types (shared → import from core) and
API types (shared → import from core), but the `Msg` enum itself is
TUI-specific dispatch vocabulary.

---

### `mapping.rs` — **Stays in theatron-tui**

Event-to-Msg translation. Hard dependency on `crossterm::event::Event`,
`KeyCode`, `KeyModifiers`, `MouseEventKind`. Purely TUI.

---

### `state/*`, `update/*`, `view/*`, `actions.rs`, `command/`, `clipboard.rs`, `diff.rs`, `highlight.rs`, `hyperlink.rs`, `keybindings.rs`, `markdown.rs`, `sanitize.rs`, `theme.rs` — **All stay in theatron-tui**

All rendering, layout, state management, and input handling. Every file in
these directories depends on `ratatui`, `crossterm`, or both.

---

## File-Level Extraction Plan

### What moves to `theatron-core/src/`

```
crates/theatron/core/src/
├── lib.rs                     # pub use re-exports; module declarations
├── id.rs                      # NousId, SessionId, TurnId, ToolId, PlanId (from tui/src/id.rs)
└── api/
    ├── mod.rs                 # pub use client::ApiClient; pub use error::ApiError; pub use types::*; etc.
    ├── client.rs              # ApiClient, build_http_client, encode_path (from tui/src/api/client.rs)
    ├── error.rs               # ApiError, Result<T> (from tui/src/api/error.rs)
    ├── types.rs               # All shared domain types (from tui/src/api/types.rs)
    ├── sse.rs                 # SseConnection, parse_sse_event (from tui/src/api/sse.rs)
    └── streaming.rs           # stream_message, StreamEvent, parse_stream_event (from tui/src/api/streaming.rs + events.rs:StreamEvent)
```

### What stays in `theatron-tui/src/`

```
crates/theatron/tui/src/
├── lib.rs                     # Entry point: run_tui()
├── main.rs                    # Binary entry point
├── api/                       # REMOVED — replaced by theatron_core::api::*
├── id.rs                      # REMOVED — replaced by theatron_core::id::*
├── events.rs                  # Event enum only (crossterm wrapper); StreamEvent imported from core
├── app.rs                     # App struct (no change in structure)
├── config.rs                  # TUI config (unchanged)
├── error.rs                   # TUI error type (unchanged, except ApiError source now comes from core)
├── msg.rs                     # Msg enum (unchanged in logic)
├── mapping.rs                 # Event→Msg (unchanged)
├── state/                     # Application state (unchanged)
├── update/                    # State update handlers (unchanged)
├── view/                      # Rendering (unchanged)
└── actions.rs, clipboard.rs, command/, diff.rs, highlight.rs,
    hyperlink.rs, keybindings.rs, markdown.rs, sanitize.rs, theme.rs
                               # All unchanged
```

---

## Dependency Analysis

### theatron-core depends on

New dependencies to add to `theatron-core/Cargo.toml`:

```toml
[dependencies]
# Already present
serde = { workspace = true }

# To add
compact_str   = { workspace = true }   # TurnId inline storage
serde_json    = { workspace = true }   # SSE/stream event parsing
snafu         = { workspace = true }   # ApiError
tokio         = { workspace = true }   # async, mpsc channels
tracing       = { workspace = true }   # structured logging
reqwest       = { workspace = true, features = ["stream", "cookies"] }
reqwest-eventsource = { git = "...", branch = "..." }  # SSE client
futures-util  = "0.3"                  # StreamExt for event streams
```

### theatron-tui depends on (after extraction)

Dependencies **removed** from `theatron-tui/Cargo.toml`:

```toml
# These move to theatron-core — no longer needed directly in tui
reqwest             # now transitive through theatron-core
reqwest-eventsource # now transitive through theatron-core
futures-util        # now transitive through theatron-core
```

Dependencies **kept** in `theatron-tui/Cargo.toml`:

```toml
theatron-core       # path = "../core"
ratatui, crossterm  # TUI rendering
tokio               # event loop, spawn
serde, serde_json   # msg/state serialization
compact_str         # used in state beyond id.rs
clap, toml, dirs    # CLI and config
tracing, tracing-subscriber, tracing-appender
snafu
syntect, arboard, base64, regex, fuzzy-matcher, similar, open, pulldown-cmark
rustls              # crypto provider for reqwest (keep — explicitly installs ring provider in tests)
```

### What depends on theatron-core (consumers)

| Crate | Dependency type | Notes |
|---|---|---|
| `theatron-tui` | direct | Primary consumer. |
| `aletheia` (main binary) | transitive via `theatron-tui` optional feature `tui` | No direct dep on `theatron-core`; gets it transitively. |

No other workspace crate currently depends on `theatron-core` or
`theatron-tui`. The workspace has two theatron members:
`crates/theatron/core` and `crates/theatron/tui`.

---

## Test Strategy

### Invariant

TUI behavior must be byte-for-byte identical after extraction. No new
functionality, no regressions. All existing tests travel with their code.

### Migration Sequence

The extraction should proceed in this order to keep the workspace
compiling at every step. Each step is a standalone commit.

**Step 1 — Move `id.rs`**
- Copy `tui/src/id.rs` to `core/src/id.rs` unchanged.
- Add `pub mod id;` + `pub use id::*;` to `core/src/lib.rs`.
- Add `compact_str` + `serde` to `core/Cargo.toml`.
- In `theatron-tui`: replace `mod id; use crate::id::*;` with
  `use theatron_core::id::*;` across all files; delete `tui/src/id.rs`.
- Gate: `cargo test -p theatron-tui && cargo test -p theatron-core`.

**Step 2 — Move `api/error.rs`**
- Copy to `core/src/api/error.rs`. Add `snafu` to core deps.
- Update `tui/src/api/error.rs` to `pub use theatron_core::api::error::*;`
  (thin re-export for one step), then remove in step 4.
- Gate: compile check only.

**Step 3 — Move `api/types.rs`**
- Copy to `core/src/api/types.rs`. Rewrite ID imports to `crate::id::*`.
- Add `serde_json` to core deps.
- Update tui re-export.
- Gate: `cargo test -p theatron-core` (all type deserialization tests pass).

**Step 4 — Move `api/client.rs`**
- Copy to `core/src/api/client.rs`. Add `reqwest`, `tracing` to core deps.
- Remove thin re-exports from tui; update tui imports to `theatron_core::api::*`.
- Delete tui `api/error.rs`.
- Gate: `cargo test -p theatron-core` + `cargo test -p theatron-tui`.

**Step 5 — Move `StreamEvent` + `api/streaming.rs` + `api/sse.rs`**
- Add `reqwest-eventsource`, `futures-util`, `tokio` to core deps.
- Move `StreamEvent` out of `tui/src/events.rs` into
  `core/src/api/streaming.rs`, co-located with `stream_message`.
- Move `api/sse.rs` to core.
- In `tui/src/events.rs`: shrink to `Event` enum only; import
  `StreamEvent` from `theatron_core::api::streaming`.
- Remove tui's direct `reqwest`, `reqwest-eventsource`, `futures-util` deps
  once no file in tui imports them directly.
- Gate: `cargo test -p theatron-core` + `cargo test -p theatron-tui`.

**Step 6 — Clean up tui `api/mod.rs`**
- Replace tui `api/mod.rs` with a single line:
  `pub use theatron_core::api::*;` (or remove the module entirely and
  update all call sites to import from `theatron_core` directly).
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`.

**Step 7 — Final validation**
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `git diff --stat` to verify only the expected files changed.

### Test Coverage After Extraction

All existing unit tests move with their source files:

| Test module | Moves to | Tests |
|---|---|---|
| `api/client.rs` `#[cfg(test)]` | `theatron-core` | `encode_path_*` (2 tests) |
| `api/types.rs` `#[cfg(test)]` | `theatron-core` | 18 deserialization tests |
| `api/sse.rs` `#[cfg(test)]` | `theatron-core` | `parse_*` (5 tests) |
| `api/streaming.rs` `#[cfg(test)]` | `theatron-core` | `parse_*` (4 tests) |
| `id.rs` `#[cfg(test)]` | `theatron-core` | Serde roundtrip, Display, deref (4 tests) |
| `events.rs` `#[cfg(test)]` | `theatron-tui` | `StreamEvent` tests stay in tui until Step 5 |

The TUI's integration-level tests (`app.rs`, `state/*`, `update/*`,
`view/*`) continue to run against `theatron-tui` unchanged. They exercise
the full stack including the imported core types.

---

## Server/Client Protocol Comparison

The TUI client parses events from two server-side protocols. Understanding the
overlap and divergence is critical for deciding what `theatron-core` should own.

### Global SSE (`/api/v1/events`)

The server emits global events through a dedicated SSE channel. The TUI
`SseEvent` enum in `api/types.rs` defines the client-side vocabulary (15
variants including `Connected`/`Disconnected` which are client-only lifecycle
markers). The server-side emitter is in `pylon` but the event definitions are
wire-format JSON — no shared Rust type exists today.

### Streaming protocol (`POST /api/v1/sessions/stream`)

| Server (`pylon/src/stream.rs` `WebchatEvent`) | TUI (`events.rs` `StreamEvent`) | Notes |
|---|---|---|
| `TurnStart` | `TurnStart` | Field-compatible. |
| `ThinkingDelta` | `ThinkingDelta` | Field-compatible. |
| `TextDelta` | `TextDelta` | Field-compatible. |
| `ToolStart` | `ToolStart` | Server includes `input: Value`; TUI omits it. |
| `ToolResult` | `ToolResult` | Server has `result: String`; TUI omits it, adds `duration_ms: u64`. |
| `TurnComplete` | `TurnComplete` | Both carry `TurnOutcome` — see type mismatch below. |
| `Error` | `Error` | Field-compatible. |
| — | `ToolApprovalRequired` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `ToolApprovalResolved` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `PlanProposed` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `PlanStepStart` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `PlanStepComplete` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `PlanComplete` | TUI-only. Not in pylon `WebchatEvent`. |
| — | `TurnAbort` | TUI-only. Not in pylon `WebchatEvent`. |

Seven event types parsed by the TUI are absent from pylon's `WebchatEvent`
enum. These are emitted by the webchat handler but the `WebchatEvent` enum
was not updated to include them — the pylon enum and actual wire format have
drifted.

### `TurnOutcome` type mismatch

| Field | Server (`pylon`) | Client (`theatron-tui`) |
|---|---|---|
| `model` | `Option<String>` | `String` (non-optional) |
| `tool_calls` | `usize` | `u32` |
| `input_tokens` | `u64` | `u32` |
| `output_tokens` | `u64` | `u32` |
| `cache_read_tokens` | `u64` | `u32` |
| `cache_write_tokens` | `u64` | `u32` |

The client uses `u32` with `#[serde(default)]` which means if the server
sends a value above `u32::MAX` (4B tokens), deserialization silently defaults
to 0. For current LLM context sizes this is safe, but the type divergence
should be resolved during extraction — either both sides use `u64`, or the
client explicitly documents the truncation.

---

## Observations (Out of Scope)

The following issues were noticed during the audit. They are captured here as
required by the prompt but left unaddressed.

| Tag | Location | Description |
|---|---|---|
| Debt | `api/sse.rs:85-99`, `api/streaming.rs:71-83` | Identical error-body parsing logic (`InvalidStatusCode` arm) duplicated verbatim in both files. Should be extracted to a shared `parse_error_body(status, resp) -> String` helper once both files live in the same crate. |
| Debt | `api/sse.rs:136-141`, `api/streaming.rs:104-109` | `str_field()` helper duplicated verbatim in both files. Should be a single shared function in `theatron-core`. |
| Doc gap | `id.rs:15-17` | The `WHY` comment explains only why `TurnId` uses `CompactString`. `NousId`, `SessionId`, `ToolId`, and `PlanId` all use plain `String` without explanation. Consider documenting the size reasoning for all five types. |
| Bug | `api/client.rs:400` | `(today_cost * 100.0) as u32` converts dollars to cents via float cast, which truncates rather than rounds. A cost of `$1.999` would yield `199` instead of `200`. Should use `(today_cost * 100.0).round() as u32`. |
| Debt | `pylon/src/stream.rs:77-116` | `WebchatEvent` enum is missing 7 event types that the server actually emits and the TUI parses (`tool_approval_required`, `tool_approval_resolved`, `plan_proposed`, `plan_step_start`, `plan_step_complete`, `plan_complete`, `turn_abort`). The enum has drifted from the wire format. |
| Debt | `pylon/src/stream.rs:133-145` vs `api/types.rs:84-103` | `TurnOutcome` is defined independently on both sides with different field types (`u64` vs `u32`, `Option<String>` vs `String`). Should be a single shared definition or at minimum documented as intentionally divergent. |
| Idea | `mapping.rs` | The file is 53 KB and handles all keymap translation. Splitting into sub-modules by domain (terminal, sse, stream, keybindings) would improve navigability. |
| Missing test | `api/client.rs` | `check_status()` has no unit tests for the JSON body extraction path. It is the sole place that parses server error messages; a test with a mock response body would prevent silent regressions. |
| Missing test | `api/client.rs` | `build_http_client()` has no test that verifies required headers (`x-requested-with`, `Content-Type`) are present on the built client. |
| Bug | `api/types.rs:20-22` | `Agent::display_name()` returns empty string when `name = Some("")`. Inconsistent with `Session::label()` at line 46 which filters empty strings via `.filter(\|s\| !s.is_empty())`. |
| Debt | `api/client.rs:487` | `knowledge_facts` query params built via string interpolation, not `.query()`. Injection-safe only because params are controlled, but inconsistent with `recall()` at line 427 which uses `.query()`. |
| Missing test | `api/streaming.rs:165-222` | `tool_approval_required`, `tool_approval_resolved`, and all `plan_*` stream event parsing have no test coverage. |
| Debt | `api/mod.rs:7-21` | Three `#[expect(unused_imports)]` on re-exports — barrel-file pattern fighting the linter. Will be resolved when `api/` module moves to core. |
