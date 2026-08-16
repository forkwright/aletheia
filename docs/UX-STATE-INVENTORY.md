# UX state inventory

A surface-by-surface account of what each app surface — TUI (`koilon`), desktop
(`proskenion`), and the HTTP/SSE API they both consume (`pylon`) — shows a user
for the states aletheia#4536's audit scope names: navigation, empty/first-run,
loading/pending, error and recovery, permission/approval, tool-call visibility,
memory/context visibility, session resume, failure explanation, and
accessibility. Every row below is a citation, not an opinion; where a state is
missing, the citation is of the code that would produce it but doesn't.

This document is the third leg of a set. It does not restate the other two:

- [`docs/TUI-CONTRACT.md`](TUI-CONTRACT.md) classifies every generic-reason
  `dead_code` site in koilon's `Msg` enum (stable/experimental/planned). Where
  a surface below depends on a `planned` `Msg` variant, this document points
  at that table instead of re-deriving it.
- [`docs/HARNESS-LIFECYCLE.md`](HARNESS-LIFECYCLE.md) states the backend
  implementation status of the nine-stage run loop every surface sits on top
  of. This document is about what a client *shows* for each stage, not
  whether the stage itself works.
- [`docs/GOLDEN-PATH.md`](GOLDEN-PATH.md) narrates the desktop-first v1.0
  workflow step by step and labels each step Implemented/Experimental/Planned.
  This document adds the TUI side (GOLDEN-PATH is desktop-only beyond one
  paragraph), adds file:line evidence, and adds the blocking/important/polish
  severity classification GOLDEN-PATH does not carry.
- [`docs/RUNTIME-CONTRACTS.md`](RUNTIME-CONTRACTS.md) names the canonical
  owner crate/type for each cross-surface contract (SSE events, failure
  taxonomy, tool metadata, ...). Findings below that are really about a
  contract owner rather than one client's rendering of it are filed as gaps
  against `RUNTIME-CONTRACTS.md`'s own rows, not restated here.

## Reading this inventory

Each surface gets a verdict per client (**working** / **degraded** / **absent**)
and, where relevant, an explicit empty/loading/error note — those three are
the states most often silently conflated. A gap is classified:

- **blocking** — breaks trust in the golden path itself: data loss, a
  recovery instruction that does not work, or a state a user cannot tell
  apart from a different state with a different correct response.
- **important** — a real, evidenced gap on a surface the audit scope names
  explicitly. Has a filed issue.
- **polish** — real but low-impact, or an intentional absence no doc
  contradicts. Documented here, not separately filed, to avoid issue-spam on
  low-value findings (mirrors the standing "route mechanical classes through
  one place" policy for a small number of low-severity items rather than
  scattering them).

Six concrete surfaces are covered end-to-end: effective tool capability
state, backend health/degradation, approval queues and recent decisions
(except the cross-agent gap below), tool-call stats/errors/durations, config
drift/restart-required/hot-reload at the data layer, and memory
health/extraction quality. Citations for those six live in aletheia#4536's PR
history rather than repeated here. This document covers the remaining
surfaces and the state-type breakdown (empty/loading/error/...) across all of
them.

## Connect and authentication

| Client | State | Notes |
|---|---|---|
| Desktop | working | Full `ConnectionState` taxonomy — `Disconnected`/`Connecting`/`Connected`/`ConnectedDegraded{status}`/`Reconnecting{attempt}`/`TimedOut`/`Failed{reason}` (`crates/theatron/proskenion/src/views/connect.rs:154-176`), each with distinct copy and color; in-flight connects are cancelable; exponential-backoff reconnection is implemented and tested (`services/connection.rs:331-410`). No-auth-required is a clean path (empty token maps to `None`, `connect.rs:187`). |
| TUI | working pre-launch only | Token resolves once at startup (`--token`/`ALETHEIA_TOKEN`/`tui.toml`, OS keyring or AES-256-GCM file fallback). `set_token()` exists on the client but has zero call sites in koilon — there is no in-session re-auth path; an expired token requires quitting and relaunching. The startup failure message for a rejected token reuses `Error::GatewayUnreachable`'s fixed "Server not running" template, which is wrong advice for this specific case. **Important — [#6818](https://github.com/forkwright/aletheia/issues/6818).** |
| API | partial | `Claims::from_request_parts` (`crates/pylon/src/extract.rs:29-86`) cleanly distinguishes `auth_mode=none` from everything else, but a missing header, a non-`Bearer` header, and an expired/malformed/revoked token all collapse into the identical `ApiError::Unauthorized` — a client cannot render "log in" versus "re-authenticate" differently. **Important — [#6826](https://github.com/forkwright/aletheia/issues/6826).** |

## Chat and turn execution (the golden path)

| Client | State | Notes |
|---|---|---|
| TUI | working, most complete surface | Distinct `StreamPhase`-driven loading indicator with per-tool-call status; empty/first-run, loading, and error states are all present and distinct (`crates/theatron/koilon/src/view/chat/mod.rs:109-131`); 30s stall warning, 60s stall message (does not auto-cancel despite the `STALL_CANCEL_SECS` name — user must Ctrl+C). A mid-stream error discards the partial response instead of committing it, while a user-initiated cancel commits it with an `"[interrupted by user]"` marker — the two termination paths disagree on which one deserves to survive. **Important — [#6817](https://github.com/forkwright/aletheia/issues/6817).** |
| Desktop | working | Four distinct branches — initial-loading / post-load-empty / loaded / error (`crates/theatron/proskenion/src/views/chat.rs:895-936`); mid-turn error shown both inline and as a dismissable banner with one-click retry that reuses the client turn id (`chat.rs:1213-1286`); older-message pagination has its own "Loading older messages..." state. |
| API | implemented | The turn wire vocabulary (`message_start`/`text_delta`/`tool_use`/`tool_result`/`message_complete`/`error`/`replay_gap`/`turn_abort`) is the canonical contract per `docs/HARNESS-LIFECYCLE.md` stage 4; retry is a client convention with no dedicated endpoint (tracked at aletheia#6790, named in that document). |

## Permission and approval flows

koilon's sidebar gives no signal that a *non-focused* agent has a tool call
awaiting approval, filed as
[#6807](https://github.com/forkwright/aletheia/issues/6807). The desktop side
reaches the identical structural gap independently
([#4871](https://github.com/forkwright/aletheia/issues/4871), closed by
shipping an honest "Unavailable" toggle rather than a working one). Both
issues' proposed fixes assume data that does not exist.

Neither client can read a background agent's approval state from any
connection it holds today. `tool_approval_required`/`tool_approval_resolved`
are per-turn stream events (`crates/pylon/src/stream_dto.rs`), delivered on
`GET /sessions/{id}/turns/{turn_id}/events` — and both clients hold exactly
one such stream open at a time (koilon: `ConnectionState.stream_rx:
Option<mpsc::Receiver<StreamEvent>>` / `.active_turn_id: Option<TurnId>`,
`crates/theatron/koilon/src/app/mod.rs:91-92`). The separate, always-open
domain-event connection both clients also hold (`GET
/api/v1/events/subscribe`) never carries an approval event — verified against
every real `EventBus::publish` call site in the workspace, which covers only
turn lifecycle, `fact.created`, `nous.lifecycle`, and `credential.*`
(`crates/pylon/src/event_bus_dto.rs:21-30`). The client-side `SseEvent` type
defines several more variants than pylon ever emits (`ToolCalled`,
`ToolFailed`, `StatusUpdate`, `SessionCreated`, ... —
`crates/theatron/skene/src/api/types/mod.rs:487-650`, parsed by
`crates/theatron/skene/src/api/sse.rs:228-288`) — client-side scaffolding
with no server producer, a distinction `docs/TUI-CONTRACT.md`'s classification
of the equivalent koilon `Msg` fields as `experimental` does not draw out.

**Blocking for the multi-agent operator-attention case; the real blocker is
external and newly filed — [#6813](https://github.com/forkwright/aletheia/issues/6813).**
Both #6807 and #4871 are correctly-filed findings; #6807 additionally received
a correction comment pointing its "desired correction" at #6813 instead of a
same-crate wiring task.

| Client | State | Notes |
|---|---|---|
| TUI | degraded | The focused agent's live approval prompt works (`Msg::ToolApprovalAction`, `crates/theatron/koilon/src/view/ops.rs`); no cross-agent signal, blocked on #6813. |
| Desktop | degraded, self-documented | In-chat approval card works for the open turn (`components/tool_approval.rs`); the notification preference for background approvals is shown but wired to a documented no-op (`crates/theatron/proskenion/src/state/notifications.rs:21-47`), blocked on #6813. |
| API | partial | Resolving a known decision works, but a 404 (routing failure) collapses "already resolved," "timed out server-side," and "never existed" into one shape, and `ApprovalResponse.routed` can never be observed `false` on a 200 in practice. **Important — [#6822](https://github.com/forkwright/aletheia/issues/6822).** No endpoint lists currently-pending approvals across sessions (`POST /sessions/{id}/approvals` only resolves a known id — `crates/pylon/src/handlers/sessions/approvals.rs`); this is subsumed by #6813 once a global event exists. |

## Tool-call visibility and trace review

| Client | State | Notes |
|---|---|---|
| TUI + Desktop | covered for the live/recent case | `proskenion::views::metrics::{tools,tool_detail,tool_duration}`, `koilon::state::ops`, `pylon::handlers::insights` are all present and wired. |
| Both | no bounded, filterable review of one completed turn | `GET /sessions/{id}/replay` is whole-session and unbounded (no `turn_id` filter, no pagination); `GET /ops/tools` is global-recent-only, capped at 100 entries, no filter. Neither can answer "show me this turn's tools" or "page 2." This is the concrete backend prerequisite for `docs/GOLDEN-PATH.md` §5's desktop trace browser — surfaced as a comment on [#6008](https://github.com/forkwright/aletheia/issues/6008), which already owns the desktop-surface half of this gap. |

## Memory and context visibility

| Client | State | Notes |
|---|---|---|
| TUI | working via a direct-call pipeline, not the `Msg` plumbing | `MemoryFactsLoaded`/`MemoryDetailLoaded`/etc. are `planned` (never constructed) per `docs/TUI-CONTRACT.md`, but the real handlers (`update/memory/handlers.rs`, `data_loading.rs`) are called directly on open, same pattern as agent-list load at startup — the feature works despite the dead `Msg` variants, not because of them. Health bar is real (`view/memory/mod.rs:259-305`, backed by client-computed `GraphHealthMetrics`). Two gaps: loading state is architecturally unobservable (multiple sequential `.await`s inside one `update()` call with no intermediate render — **important, [#6815](https://github.com/forkwright/aletheia/issues/6815)**), and graph/entities/relationships/timeline fetch failures render identically to genuine-empty with no log or toast at all (facts gets a debug-only log) — **important, [#6816](https://github.com/forkwright/aletheia/issues/6816).** |
| Desktop | working, comprehensive | Facts tab (search/filters/curation actions), Graph tab (entities, relationships, PageRank, merge/flag/delete), Theke file workspace, and Meta → Insights (aggregate trend view) are all implemented per `docs/GOLDEN-PATH.md` §4. |
| API | partial | Facts/entities/relationships/timeline/search/structural-health (`check_graph_health`) are all exposed. Two gaps: `list_facts`/`list_entities` cannot distinguish genuinely-empty from store-unavailable, while sibling endpoints (`entity_relationships`, `check_graph_health`) correctly 503 — **important, [#6821](https://github.com/forkwright/aletheia/issues/6821).** The server-computed memory-health score (`MemoryHealthMetrics`, `crates/pylon/src/handlers/knowledge/health_metrics.rs`) is exported only as a Prometheus gauge, not a JSON route; proskenion independently recomputes the same three inputs client-side from raw fact/entity data, a duplication the module's own doc comment already names as a divergent-inputs risk — **important, [#6823](https://github.com/forkwright/aletheia/issues/6823).** koilon has no equivalent computation at all, so it cannot show a health score even though the server already computes one. |

## Session list, resume, and continue

| Client | State | Notes |
|---|---|---|
| TUI | working, one dead code path | Per-agent session picker works (Enter to resume, real `client.history()` call, error toast on failure); the "all sessions across every agent" picker koilon's own code enumerates (`OverlayKind::SessionPickerAll`) is confirmed never constructed anywhere — genuinely `planned`, matching `docs/TUI-CONTRACT.md`, not a new finding. The agents-fetch-failure path at startup tells the user to run a `:reconnect` command that does not exist. **Important — [#6814](https://github.com/forkwright/aletheia/issues/6814).** |
| Desktop | working | Full `SessionLoadState` taxonomy (`Loading`/`Empty`/`Loaded`/`TransportError`/`HttpError`/`ContractError`, `crates/theatron/proskenion/src/views/sessions/mod.rs:260-268`), per-state retry, archive/restore with toast, resume-into-Chat action. |
| API | strong on reconnect mechanics, missing a lightweight status check | `GET /sessions/{id}/turns/{turn_id}/events` reconnect is well-engineered: honors `Last-Event-ID`, emits `turn_reconnect_state` before replay, and discloses `replay_gap` honestly on buffer eviction rather than silently dropping events. What is missing is the step before that: `GET /sessions/{id}` carries no field for "is a turn currently running, and what's its id" — a client that lost its remembered `turn_id` (restart, different device, long-idle reconnect) must fetch the heavy `/replay` endpoint just to find out. **Important — [#6824](https://github.com/forkwright/aletheia/issues/6824).** Related to the broader canonical-turn-identity gap tracked at aletheia#4853. |

## Settings, config drift, and hot-reload

| Client | State | Notes |
|---|---|---|
| TUI | working | Typed fields (Bool/Integer/Text/ReadOnly) with a per-field `requires_restart` flag and footer legend; save posts per-section and names the specific paths needing a restart rather than falsely claiming "reloaded" (tested). |
| Desktop | working | Four tabs (Servers/Appearance/Keybindings/Notifications); server health probe with live "Testing…" status and drift detection; keybinding rebinding has a live capture UI. |
| API | partial | The hot/cold classification itself is complete and canonical (`docs/HOT-RELOAD.md`, schema-level). What is missing is a *runtime* signal: `restart_required` is returned only by the mutating `POST /config/reload`/`PUT /config/{section}` calls themselves — `GET /config`/`GET /config/{section}` return the redacted current value with no drift metadata, so a client that did not perform the last change (or reconnected after it) cannot observe that a restart is pending. **Important — [#6825](https://github.com/forkwright/aletheia/issues/6825).** |

## Metrics and ops dashboards

Both clients cover more than tool stats/errors/durations alone.
Desktop additionally has Tokens/Costs tabs with trend charts and dimensional
breakdowns, and a 4-tab Ops surface (Dashboard/Tools/Credentials/Providers)
with auto-refresh. No queue-depth/backlog view was found on either client;
given both are single-connection clients over one pylon instance rather than
a dispatch orchestrator, this reads as scope rather than a gap, but no doc
confirms that explicitly — flagged as **polish, not filed** pending that
confirmation.

## Planning and retrospective

Both clients are blocked on the same external, already-tracked dependency —
the B23 planning backend, tracked at aletheia#4482 (open). This is the
inventory's clearest example of a *legitimate* defer: the blocker is a named,
open, external issue, not size. TUI (`docs/TUI-CONTRACT.md`) and desktop
degrade differently against the identical block: koilon's planning `Msg`
scaffolding is fully inert with no user-visible acknowledgment beyond a
static string, while proskenion renders an explicit, honestly-worded
"Planning verification only" placeholder gated on a real capability flag
(`crates/theatron/proskenion/src/views/planning/dashboard.rs:102-166`) — the
same block, better surfaced. One proskenion sub-surface
(`views/planning/verification.rs`) is not gated behind B23 and works
independently. Not filed as new issues — #4482 already owns the blocker, and
`docs/TUI-CONTRACT.md` already owns koilon's specific scaffolding inventory.

## Files, diff, and workspace review

| Client | State | Notes |
|---|---|---|
| TUI | working for the manual path | `:diff` spawns a real `git diff HEAD`, three render modes, clean "No uncommitted changes" degrade. Auto-diff-on-tool-edit (`DiffFromToolResult`) has a real, tested handler but zero live construction sites — matches `docs/TUI-CONTRACT.md`'s `planned` classification exactly, not a new finding. |
| Desktop | working, more complete than the TUI equivalent | Theke: tree explorer with git status, markdown preview/edit, unified/side-by-side diff, debounced search — each sub-surface (tree/viewer/diff) has its own `Loading`/`Error`/`Empty` states. |

## Export

TUI supports both Markdown (`:export`) and a full replay-faithful JSON export
(`:export json`), both tested, both guarding the empty-conversation case.
Desktop supports Markdown-to-clipboard only; a full export dialog (format
choice, destination picker, packaging trace + memory evidence) is the second
half of `docs/GOLDEN-PATH.md` §7's already-tracked, already-filed gap
([#6008](https://github.com/forkwright/aletheia/issues/6008)) — not
duplicated here.

## First-run and setup

`docs/GOLDEN-PATH.md`'s Headless Fallback section claimed the TUI supports
"setup wizard flows." It does not: `koilon::run_wizard` is exported from the
TUI crate but its only caller anywhere in the repo is `aletheia init`
(`crates/aletheia/src/init/mod.rs:214-221`) — a separate command.
`aletheia-tui`'s own `main.rs` has no wizard-related flag and never invokes
it. **Corrected in `docs/GOLDEN-PATH.md` directly** (public docs describing
an unimplemented workflow is the exact defect aletheia#4536's acceptance
criteria names) rather than filed as an issue, since the fix was a
one-paragraph doc correction, not a code gap.

The wizard itself works as a form once reached via `aletheia init`, with one
asymmetry worth naming as **polish, not filed**: the interactive TUI wizard
path performs no validation (API key format, agent ID, no test-the-key call),
while the sibling non-interactive `cliclack` fallback on the same `aletheia
init` command does validate the agent ID field.

Desktop's first-run wizard, Connect view, and Settings → Servers are all
implemented per `docs/GOLDEN-PATH.md` §1.

## Navigation and information architecture, and accessibility

The two clients take structurally different approaches: koilon is a flat
command/keybinding namespace with no nested menus (`:command` strings plus a
`KeyMap`/`Action` dispatch table); proskenion is a routed multi-tab app
(`Route` enum, `crates/theatron/proskenion/src/app.rs:42-63`) with global
keyboard shortcuts (Ctrl+1–7, Ctrl+K palette, F1 help) layered on top.

Both have the same shape of gap: a discoverability surface that has drifted
from the thing it is supposed to describe.

- **TUI**: the Help overlay and status-bar footer are driven by a
  hand-authored table (`keybindings/registry.rs`) that is never
  cross-validated against the real dispatch table (`keymap.rs`). Four live
  global keybindings (`Ctrl+M`/`Ctrl+D`/`Ctrl+P`/`Ctrl+H`) are absent from
  it, and a fifth default binding is dead by collision with
  `Action::ToggleThinking`. **Important — [#6819](https://github.com/forkwright/aletheia/issues/6819).**
- **Desktop**: global keyboard nav and landmark roles (`nav`/`main`,
  `layout.rs:67-118`) are solid, and ARIA markup is real and substantial —
  but concentrated in 3 of the app's 9 top-level view domains (Sessions,
  Files, Chat). Memory, Metrics, Ops, Planning, Settings, and Meta carry zero
  `aria-*`/`role=` attributes. **Important — [#6820](https://github.com/forkwright/aletheia/issues/6820).**

Neither client has a mouse-only or otherwise keyboard-inaccessible feature —
koilon's mouse handling is minimal and never load-bearing (scroll + sidebar
click, both with keyboard equivalents); proskenion's global shortcuts work
regardless of which view is focused. The gap in both cases is *discovery and
legibility* of an already keyboard-reachable surface, not reachability itself.

## Failure explanation (cross-cutting)

"What happened, why, and what can be done next" is well-served at the API
layer in the general case — `ApiError::classify()` (`crates/pylon/src/error.rs:178-224`)
gives every known error variant an explicit `(FailureCategory, Recoverability,
NextAction)` triple with no catch-all, applied unconditionally to every
`ApiError`-returning handler across the crate, and a second path
(`classify_by_status()`) covers the handful of sites that build a response
directly (CSRF, rate limiters, the plain-text `/metrics` route via a global
enrichment middleware). This is genuinely comprehensive, not spot-covered.

The specific gaps are all about *granularity* within an otherwise-working
mechanism, and are filed individually above rather than repeated here: the
approval-resolve 404 (#6822), the three-way 401 collapse (#6826), koilon's
mid-stream-error data loss (#6817), and koilon's memory silent-failure
asymmetry (#6816). Each is a case where a failure is correctly caught and
turned into *a* response, but the response does not distinguish causes a user
would act on differently.

## A smoke path through normal, pending, error, and review states

No single script exercises this whole sequence today — `demo/smoke-test.sh`
covers server startup/health/agent-registration (process-level only,
described in `demo/README.md`), and `crates/integration-tests/tests/tool_approval.rs`
covers the approval round-trip at the engine/API level (`approval_required_approved_tool_executes`,
`approval_required_denied_tool_does_not_execute`, `approvals_with_no_active_turn_returns_404`,
`approvals_with_invalid_decision_returns_422`, `approvals_unauthenticated_returns_401`).
Identifying the path — the acceptance criterion's actual ask — rather than
building new automation: run the demo (`demo/README.md`) and walk one
session through every state named above in one pass:

1. **Empty/first-run** — a fresh `demo/instance` has no sessions; the TUI's
   session picker shows "No sessions found," or the desktop's Sessions view
   shows its `Empty` state.
2. **Normal** — send a message in Chat. Watch the loading/pending state
   (koilon's phase-labeled spinner, or the desktop's streaming indicator)
   resolve into streamed text.
3. **Tool call + approval** — prompt the agent to use a tool configured to
   require approval. Watch the inline approval card render (both clients);
   approve it and watch the turn resume. This exercises the one state
   currently degraded on both clients if a *second* agent's approval is
   triggered while the first is focused — the cross-agent gap #6813/#6807/#4871
   describe.
4. **Error** — kill the configured LLM provider mid-turn (stop `ollama` or
   equivalent) and send another message. Watch the error state render with
   its recovery action (retry banner on desktop, error toast + manual retry
   on TUI).
5. **Review** — open Ops → Tools (desktop) or `:memory`/tool-stats (TUI) to
   see the failed and succeeded calls from steps 2-4; open Sessions and
   resume the same session to confirm state survives a reconnect.
6. **Close** — export the conversation (`:export` on TUI; clipboard export on
   desktop) and archive the session.

Every stage above cites a real, working code path elsewhere in this document
except where a citation says otherwise (the approval cross-agent gap in step
3, and the desktop export dialog's absence past clipboard Markdown in step
6). No stage requires a surface that does not exist.

## Open follow-up issues

| Issue | Surface | Severity |
|---|---|---|
| [#6813](https://github.com/forkwright/aletheia/issues/6813) | pylon/skene — no global tool-approval domain event | blocking (cross-agent case) |
| [#6814](https://github.com/forkwright/aletheia/issues/6814) | koilon — phantom `:reconnect` command | important |
| [#6815](https://github.com/forkwright/aletheia/issues/6815) | koilon — loading state architecturally unobservable | important |
| [#6816](https://github.com/forkwright/aletheia/issues/6816) | koilon — memory fetch failures indistinguishable from empty | important |
| [#6817](https://github.com/forkwright/aletheia/issues/6817) | koilon — mid-stream error discards partial output | important |
| [#6818](https://github.com/forkwright/aletheia/issues/6818) | koilon — no in-TUI re-auth, misleading error message | important |
| [#6819](https://github.com/forkwright/aletheia/issues/6819) | koilon — keybindings registry drift | important |
| [#6820](https://github.com/forkwright/aletheia/issues/6820) | proskenion — ARIA coverage concentrated in 3/9 view domains | important |
| [#6821](https://github.com/forkwright/aletheia/issues/6821) | pylon — knowledge list empty-vs-unavailable ambiguity | important |
| [#6822](https://github.com/forkwright/aletheia/issues/6822) | pylon — approval-resolve 404 granularity | important |
| [#6823](https://github.com/forkwright/aletheia/issues/6823) | pylon — memory-health score has no JSON route | important |
| [#6824](https://github.com/forkwright/aletheia/issues/6824) | pylon — no in-flight-turn visibility on session GET | important |
| [#6825](https://github.com/forkwright/aletheia/issues/6825) | pylon — config drift not observable at rest | important |
| [#6826](https://github.com/forkwright/aletheia/issues/6826) | pylon — three 401 causes collapse to one shape | important |

Plus a correction comment on the already-open
[#6807](https://github.com/forkwright/aletheia/issues/6807) (its proposed fix
assumed data that does not reach koilon; #6813 is the real dependency) and a
backend-prerequisite comment on the already-open
[#6008](https://github.com/forkwright/aletheia/issues/6008) (the desktop
trace browser needs a bounded, filterable tool-trace endpoint before it needs
a UI).

Every gap in this inventory either has a filed issue above or a stated,
genuinely external blocker — the planning surface's B23 dependency
(aletheia#4482) is the one case of the latter, named above rather than
repeated.
