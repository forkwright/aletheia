# The Aletheia harness lifecycle

Aletheia is one coherent harness for reproducible, inspectable, memoryful
agent work — not a set of independent surfaces that happen to share a
process. This document names the canonical run loop every crate, surface,
trace, and doc reinforces, maps each stage to the code that owns it, and
states which stages are implemented, experimental, planned, or intentionally
out of scope.

CLI (`crates/aletheia/src/commands/`), TUI (`koilon`), desktop (`proskenion`),
and HTTP/SSE (`pylon`) are four surfaces over the same nine-stage loop below —
none of them define their own lifecycle. Where a surface uses different
words for the same stage, that is a bug in the surface, not a second
lifecycle.

## The nine stages

### 1. Task/request enters the system

**Status: implemented**, except the general-purpose event-trigger boundary.

A turn starts from one of three entry points, all converging on the same
`nous` actor:

- CLI: `crates/aletheia/src/commands/` (`chat`, `run`, ...)
- HTTP/SSE: `crates/pylon/src/handlers/sessions/` (message-send + streaming)
- TUI/desktop: `koilon`/`proskenion`, both driving the same `pylon` HTTP/SSE
  surface as any other client (see `crates/theatron/skene/src/api/`, the
  shared HTTP client both use)

A fourth entry point — file-watch and webhook events feeding the task runner
without an operator or API call — is reserved at the config level
(`oikonomos::state::AllowedTriggers`) but has no handler registry, dispatch
wiring, or matching router type. It has no committed implementation plan;
tracked at aletheia#6789.

### 2. Configuration, provider, agent, and tool policy are resolved

**Status: implemented.**

- Config: `taxis` owns the TOML cascade (defaults → file → env), no figment.
  See `crates/taxis/src/config/` (`agents.rs`, `tools.rs`, `gateway.rs`, ...).
- Providers: `hermeneus::provider::ProviderRegistry` resolves which LLM
  backend serves a given model.
- Tool policy: `organon::registry::ToolRegistry` combined with
  `crates/taxis/src/config/tools.rs`'s `tool_groups`/`tool_allowlist`
  produces the turn's effective tool surface (`effective_surface` in
  `crates/nous/src/actor/turn.rs`).

### 3. Context and memory are selected

**Status: implemented.**

- Context assembly: `crates/nous/src/bootstrap/` builds the system prompt
  from sections (identity, tool summaries, working checkpoints, ...).
- Memory recall: the pipeline's `recall` stage
  (`crates/nous/src/pipeline/stages.rs`) retrieves relevant knowledge from
  vector/BM25 search via `episteme`.
- History selection: the pipeline's `history` stage loads conversation
  history within the turn's token budget.

### 4. The agent runs

**Status: implemented.**

`crates/nous/src/actor/turn.rs` drives the turn through
`crates/nous/src/pipeline/stages.rs`'s `execute` stage, which calls the
resolved LLM provider with optional cooperative deadline and streaming.
Transient provider failures fall back to `degraded_mode::build_degraded_response`
rather than propagating (see `DegradedMode::TurnBudgetExceeded`). The wire
vocabulary streamed to every client is `crates/pylon/src/stream_dto.rs`'s
tagged event set: `message_start`, `text_delta`, `thinking_delta`,
`tool_use`, `tool_result`, `message_complete`, `error`, `replay_gap`,
`turn_abort`.

### 5. Tools execute under declared policies

**Status: implemented.**

`organon::registry::ToolRegistry` dispatches to `ToolExecutor`
implementations (`crates/organon/src/builtins/`, registered via
`register_all()`). Every tool declares `organon::types::Reversibility` and
`organon::types::ApprovalRequirement`, which the execute stage enforces
before running a tool and which clients render (see the ops-tools panel
wiring in `crates/theatron/proskenion/src/views/ops/`).

### 6. Approvals/interventions are handled

**Status: implemented** for provider-declared tool approval; correction
hooks are a separate, also-implemented mechanism.

- Tool approval: the SSE wire carries `tool_approval_required` /
  `tool_approval_resolved` (`crates/pylon/src/stream_dto.rs`); resolution is
  a first-class HTTP surface at `crates/pylon/src/handlers/sessions/approvals.rs`
  (`resolve`, `approve_tool`, `deny_tool`).
- Operator intervention mid-turn: `crates/nous/src/hooks/builtins/correction.rs`
  (`CorrectionInjector`/`CorrectionDetector`) lets an operator correct agent
  behavior without waiting for turn completion.

### 7. Memory and session state update

**Status: implemented.**

- Turn results persist via the pipeline's `finalize` stage
  (`crates/nous/src/pipeline/stages.rs`) into `mneme::store::SessionStore`
  (messages, usage records, notes).
- Durable agent-curated working memory persists via
  `nous::working_memory::FjallWorkingCheckpointStore`, written by the
  `update_working_checkpoint` tool and reinjected each turn by
  `crates/nous/src/hooks/builtins/working_checkpoint.rs`.
- Reflected facts promote via the pipeline's `reflection` stage into durable
  typed knowledge (`episteme`).

### 8. Trace/run records are emitted

**Status: implemented**, split across three durable/observable surfaces
rather than one unified "trace" store — this is the stage most worth
tightening if the harness lifecycle needs one canonical record format.

- Per-turn internal events: `koina::event::EventEmitter` +
  `crates/nous/src/pipeline/events.rs` (`StageCompleted` and friends), naming
  the pipeline's own stage vocabulary: `context`, `recall`, `history`,
  `microcompact`, `full_compact`, `guard`, `execute`, `finalize`,
  `reflection`.
- Durable per-turn records: `mneme::store::SessionStore` (messages, usage
  records) is the record a session export (`agent_io.rs`) reads back.
- Prompt audit trail: `nous::audit::PromptAuditLog`
  (`crates/nous/src/audit.rs`).
- Operational metrics: Prometheus counters/histograms documented in
  `docs/OBSERVABILITY.md`.

### 9. Result is reviewed, continued, retried, exported, or closed

**Status: implemented** for review/continue/export/close; **retry is
under-specified** (client convention, not a harness primitive).

The canonical session lifecycle state names live in one place:
`graphe::types::SessionStatus` (`Active` / `Archived` / `Distilled`,
re-exported as `mneme::types::SessionStatus`) — every surface (CLI, TUI,
desktop, HTTP) reads and writes this same enum; none defines its own status
vocabulary.

- Review: session read endpoints (`crates/pylon/src/handlers/sessions/`).
- Continue: send another message into the same session.
- Export/close: `crates/aletheia/src/commands/agent_io.rs`
  (`export_agent`/`import_agent`), `SessionStatus::Archived`.
- Retry: no dedicated endpoint or CLI verb re-submits a prior turn's input —
  an operator retries today by resending a message informally. Tracked at
  aletheia#6790.

## Vocabulary map

| Layer | Vocabulary | Canonical source |
|-------|-----------|-------------------|
| Session lifecycle | `active` / `archived` / `distilled` | `graphe::types::SessionStatus` |
| Turn wire events (SSE) | `message_start`, `text_delta`, `tool_use`, `tool_approval_required`, `tool_approval_resolved`, `tool_result`, `message_complete`, `error`, `replay_gap`, `turn_abort` | `crates/pylon/src/stream_dto.rs` |
| Internal pipeline stages | `context`, `recall`, `history`, `microcompact`, `full_compact`, `guard`, `execute`, `finalize`, `reflection` | `crates/nous/src/pipeline/stages.rs` / `events.rs` |
| Tool policy | `Reversibility`, `ApprovalRequirement` | `crates/organon/src/types/mod.rs` |

A client (TUI, desktop, a third-party integration) that introduces its own
name for one of these — a different session-status string, a differently
spelled SSE event tag — has drifted from the harness vocabulary, not
extended it. Fix the client, not this table.

## Stages needing follow-up

- Stage 1's general-purpose file-watch/webhook trigger boundary has no
  implementation plan and no reserved type (`AllowedTriggers` reserves the
  config slot only). Tracked at aletheia#6789.
- Stage 9's retry sub-behavior has no first-class primitive. Tracked at
  aletheia#6790.
