# TUI stability contract

`koilon` is a stable keyboard-first operator console: chat, planning, memory
browsing, metrics, and ops views over SSH or a local terminal, using the same
`skene` API client as `proskenion`. It is not a demo or a staging ground for
speculative UI — every wired view is production surface.

`Msg` (`crates/theatron/koilon/src/msg.rs`) carries every application
message: key events, SSE events, streamed turn content, and API responses.
Because it is one flat enum, it also accumulates scaffolding for messages and
fields that a future feature will need but nothing constructs yet. Left
unclassified, that scaffolding is indistinguishable from genuine dead weight:
37 sites carried the single, undifferentiated
`#[expect(dead_code, reason = "planned TUI feature")]`, with no record of
which fields are active-but-partial versus fully unstarted.

## Classification

Every `#[expect(dead_code, ...)]` site in `msg.rs` falls into one of four
classes:

- **stable** — constructed and consumed; the lint fires only because of a
  match arm that ignores the field with `..` for a documented reason (a
  keybinding-dispatched command, a Tick-driven side effect, a render-only
  path). These already carry a specific `reason` naming *why* it looks dead;
  they are not part of the classification table below.
- **experimental** — the surrounding `Msg` variant has a live producer *and*
  a live consumer (a real event pipeline is wired end-to-end), but this
  specific field is not yet read by the handler. The feature works; this
  detail is not surfaced yet.
- **planned** — the variant has no producer anywhere in the crate. The match
  arm exists for exhaustiveness or as a forward stub, but nothing in koilon
  ever constructs it. Pure scaffolding for a feature that has not started.
- **removable** — scaffolding with no plan to ever be wired. None of the
  current sites qualify: absent a specific reason to delete a given site,
  the default is to hold it (`planned` or `experimental`), not remove it.
  Deleting a `planned` site is a scope decision for whoever picks up that
  feature, not a cleanup pass.

## Planning/Retrospective dashboard fields

`StreamDecisionRequired`, `DecisionCardNextField`, and `DecisionCardPrevField`
are the TUI-side surface for planning decision cards. `DecisionCardNextField`
and `DecisionCardPrevField` already carry the specific reason "planned TUI
feature: key bindings not yet wired" (not the generic one, so they are not in
the table below either). All three are the same class of `planned` work as
`StreamDecisionRequired`: they depend on a B23 planning backend that has not
landed. **Hold, do not delete, until B23 lands** — then wire both the
keybindings and `StreamDecisionRequired`'s consumer together, since a
half-wired decision card (fields navigable but no question ever arrives, or
vice versa) is worse than the current fully-inert state.

`RetrospectiveClose` and `PlanningClose` are unrelated to this ruling: both
already carry the specific reason "wired in keybinding handler" (the message
is dispatched outside the `Msg` pipeline, not unstarted), so they are also
outside the table below.

## `planned TUI feature` sites (generic reason)

Sites still carrying the undifferentiated reason string, reclassified below.
`file:line` is `crates/theatron/koilon/src/msg.rs`.

### experimental — variant has a live producer and consumer; only this field is unread

| Field | Line | Live handler |
|---|---|---|
| `SseTurnBefore.session_id` | 166 | `update/sse.rs::handle_sse_turn_before` (matches on `nous_id` only) |
| `SseTurnBefore.turn_id` | 168 | same |
| `SseToolFailed.tool_name` | 181 | `update/sse.rs::handle_sse_tool_failed` (matches on `nous_id` only) |
| `SseToolFailed.error` | 183 | same |
| `SseSessionCreated.session_id` | 192 | `update/sse.rs::handle_sse_session_created` (matches on `nous_id` only) |
| `StreamTurnStart.session_id` | 214 | `update/streaming/mod.rs::handle_stream_turn_start` (matches on `turn_id`, `nous_id`) |
| `StreamToolApprovalResolved.tool_id` | 242 | `update/streaming/mod.rs::handle_stream_tool_approval_resolved` (matches on nothing — clears pending-approval state generically) |
| `StreamToolApprovalResolved.decision` | 244 | same |
| `StreamPlanStepStart.plan_id` | 251 | `update/streaming/mod.rs::handle_stream_plan_step_start` (matches on `step_id`) |
| `StreamPlanStepComplete.plan_id` | 256 | `update/streaming/mod.rs::handle_stream_plan_step_complete` (matches on `step_id`, `status`) |
| `StreamPlanComplete.plan_id` | 262 | `update/streaming/mod.rs::handle_stream_plan_complete` (matches on `status`) |

None of these need a keybinding or a new event source to activate — the
event already arrives and is already handled. Wiring one in is a matter of
extending the existing handler to read and render the field (e.g. a
per-session inspector, a richer approval toast, or a plan-step location),
not building a new pipeline.

### planned — variant is never constructed anywhere in the crate

| Variant | Line |
|---|---|
| `AgentsLoaded` | 274 |
| `SessionsLoaded` | 276 |
| `HistoryLoaded` | 281 |
| `CostLoaded` | 286 |
| `AuthResult` | 290 |
| `ApiError` | 292 |
| `SettingsLoaded` | 295 |
| `SettingsSaved` | 297 |
| `SettingsSaveError` | 299 |
| `MemoryFactsLoaded` | 333 |
| `MemoryDetailLoaded` | 338 |
| `MemoryEntitiesLoaded` | 340 |
| `MemoryRelationshipsLoaded` | 342 |
| `MemoryTimelineLoaded` | 344 |
| `MemorySearchResults` | 346 |
| `MemoryActionResult` | 348 |
| `ExportConversation` | 404 |
| `SessionSearchSubmit` | 434 |
| `DiffOpen` | 479 |
| `DiffFromToolResult` | 488 |
| `StreamDecisionRequired` | 499 |
| `OverlayKind::SessionPickerAll` | 549 |
| `OverlayKind::Settings` | 553 |
| `AuthOutcome::Success` | 607 |
| `AuthOutcome::NoAuthRequired` | 609 |
| `AuthOutcome::Failed` | 611 |

A `planned` variant's match arm (where one exists in `update/mod.rs`) is
exhaustiveness scaffolding, not evidence of a working pipeline — several of
these (`AgentsLoaded`, `HistoryLoaded`, `SettingsLoaded`, ...) already have a
handler function written and ready; what is missing is the producer that
would ever construct the message (an async fetch dispatching it, an SSE
mapping, a command binding). `ExportConversation` is the one exception worth
naming explicitly: koilon's live export path is the `:export` command
(`update/command.rs::execute_command`, matching the string `"export"`
directly), which never routes through this `Msg` variant at all. The variant
is a leftover second entry point, not a stub for unbuilt work; delete it if
a future PR confirms nothing else is meant to route through the `Msg`
pipeline instead of the command dispatcher.

## Adding a new `Msg` field

A field that will not be read until a later PR is legitimate scaffolding,
not a lint to silence and forget:

1. Tag it `#[expect(dead_code, reason = "...")]` with a **specific** reason —
   what future work reads it, or why the match arm ignores it on purpose.
   Reserve the generic `"planned TUI feature"` string for a genuinely
   unstarted variant (this doc's `planned` table); anything already wired to
   a live handler is `experimental` and should say so in the reason.
2. If it depends on a named upstream capability (a backend endpoint, another
   PR), name it in the reason so `rg` finds every blocked-on site.
3. Do not delete a `planned` or `experimental` site as part of an unrelated
   cleanup pass — removal is a decision for whoever owns that feature's
   fate, made explicitly, not a side effect of a lint sweep.
