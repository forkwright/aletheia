# Golden Path

Aletheia's v1.0 target path is desktop-first. The desktop app (`proskenion`) is
the target user surface for configuring a client connection, starting work,
watching an agent act, inspecting memory, reviewing failures, and exporting the
useful result.

Today, the current supported first-run path for a public checkout is: initialize
an instance, start the server, then use the terminal dashboard (`koilon`,
launched with `aletheia tui`). The desktop app is available as a source-built
preview and uses the same HTTP API and SSE stream as the TUI, but it is not the
default public onboarding path until the release train ships matching desktop
artifacts.

This document describes the v1.0 target workflow and labels each surface by its
current implementation status.

Status labels in this document mean:

- **Implemented:** present in the current source and wired into a desktop view.
- **Experimental:** present in source, but the backing server endpoint or app
  integration is incomplete or documented as pending.
- **Planned:** not present as a complete app surface yet.

## 1. Configure Providers

**Implemented:** Configure model providers and tool policy in the server config,
then connect the desktop app to that server.

The server reads `instance/config/aletheia.toml` through the taxis config
cascade: compiled defaults, then TOML, then `ALETHEIA_` environment overrides.
The agent model path starts under `agents.defaults.model` and can be overridden
per `agents.list[]` entry. Provider backends live in `[[providers]]` tables.
Each provider declares `name`, `providerType`, optional `baseUrl`, optional
`apiKeyEnv`, subprocess-only `binary`/`workdir`/`timeoutSecs`,
`deploymentTarget`, whether health aggregation treats it as `optional`, and the
`models` it can serve.
When this list is non-empty, it is the complete provider ordering surface:
providers are registered in list order, and an `anthropic` entry without
`apiKeyEnv` uses the top-level credential chain at that declared position.

Credential discovery is configured by `[credential]`. The documented strategies
are `auto`, `api-key`, and `claude-code`; the Anthropic provider also reads
`ANTHROPIC_API_KEY` outside the config cascade.
`[credential].source = "claude-code"` is a credential-chain choice for the
Anthropic HTTP provider, not a `[[providers]]` declaration for the Claude Code
subprocess adapter.

Tool access is fail-closed. The `toolGroups` field accepts `"all"`, `"deny"`,
or a list of group names. Missing `toolGroups`, `"deny"`, and `[]` all deny
grouped tools. Starter configs use the named `least-privilege-starter` profile:

```toml
[agents.defaults]
toolGroups = ["read", "plan", "verify"]
```

Trusted single-operator deployments can opt into the named `full-power-local`
profile by setting `toolGroups` to `["read", "edit", "command", "mcp",
"spawn_subtask", "plan", "verify"]` after reviewing the file-editing, local
process, external MCP, and subtask-delegation risks.

The recognized group names are `read`, `edit`, `command`, `mcp`,
`spawn_subtask`, `plan`, and `verify`.

In the desktop app, the first-run wizard, Connect view, and Settings -> Servers
manage the client connection: server URL, optional bearer token, saved servers,
active server switching, and connection probes. They do not replace the server
provider config.

Ops -> Credentials renders a credential-management panel for listing, validating,
rotating, adding, and removing credentials. The server exposes the credential API
at `/api/v1/system/credentials`.

## 2. Start or Resume a Session

**Implemented:** Use Chat for new work and Sessions for existing work.

Open Chat, select an agent from the sidebar, type a message, and send it. The
chat view creates a tab for the active agent when needed and streams the turn
against the selected agent and session key. Session tabs keep active work
visible while you move through the app.

To resume existing work, open Sessions. The Sessions view lists sessions with
search, status filters, agent filters, sort controls, and optional visibility
for system sessions. Selecting a session opens its detail panel with message
counts, token summary when available, model, duration, message previews, and an
Open in Chat action. Archive and Restore actions close or re-open a session in
the working list without deleting its history.

**Current supported first-run and headless path:** Use `aletheia tui`. The TUI
accepts `--url`, `--token`, `--agent`, and `--session` flags and provides chat,
planning, memory, metrics, and ops views in a terminal.

## 3. Observe Tool Calls and Approvals

**Implemented:** Watch the current turn in Chat and the broader tool stream in
Ops.

During a streamed turn, Chat shows the routing stage as the pipeline moves
through bootstrap, recall, thinking, tool execution, completion, abort, or
error states. Assistant messages include tool counts. Tool calls render as
expandable panels with status, input JSON, output, error text, and duration.

When the server emits a tool approval request, Chat renders an inline approval
card with the tool name, risk level, reason, input preview, and Approve/Deny
buttons. The desktop sends those decisions back to the server through the tool
approval API.

Ops -> Tools shows active tools, total calls, succeeded calls, failed calls, and
tool history. Ops -> Dashboard shows agent cards, active turn counts, service
health, daemon and cron status, and toggle state fetched from the server.

## 4. Inspect Memory and Context

**Implemented:** Use Memory for facts first, then the graph lens when needed.

Memory opens on the Facts tab. Facts are readable memory rows with search,
type and tier filters, sort controls, confidence display, sensitivity badges,
and curation actions. Stated or verified facts are visually stronger than
inferred facts. Forget, Restore, confidence adjustment, and sensitivity changes
are available from the fact list.

The Graph tab is an opt-in entity view. It lists entities with search and
filters; selecting an entity shows properties, relationships, memories,
confidence, PageRank, metadata, and entity actions such as merge, flag, and
delete.

Theke, the desktop file workspace, complements Memory. It provides a file tree,
search, viewer, markdown preview/edit mode, save handling, conflict warnings,
and a diff view opened from file-change notifications.

Meta -> Insights provides aggregate memory and context signals: memory health,
confidence distribution, stale entities, knowledge growth, agent performance,
conversation quality, and system reflection. Use it to understand trends rather
than to edit facts.

## 5. Review Traces and Failure Causes

**Implemented:** Review live failures in Chat, Ops, Sessions, Metrics, and
connection surfaces.

Chat preserves partial output on aborts, shows stream errors, and exposes Retry
for the last failed user turn. Tool panels show per-call errors and durations.
The input bar shows Abort while a stream is active.

Ops -> Tools shows failed tool counts and per-tool history rows. Ops ->
Dashboard shows service-health failures, daemon task status, cron job results,
and agent connection health. Metrics shows token and cost trends, including
daily, weekly, and monthly cost ranges and per-agent cost breakdowns. Sessions
shows session-level message counts, token usage when present, model, duration,
and distillation history.

The Connect view and Settings -> Servers show connection failures, unreachable
servers, retry state, and the URL that a health probe actually tested.

**Planned:** A single desktop trace browser that opens a turn, shows every
pipeline stage, links directly to logs, and explains root failure causes is not
yet a complete primary surface. Today the evidence is split across Chat, Ops,
Metrics, Sessions, server logs, and retained trace files.

## 6. Continue, Retry, or Close Out Work

**Implemented:** Continue in Chat, retry failed turns, or close sessions through
Sessions.

Continue work by sending another Chat message in the active session. If a turn
is still streaming, Abort cancels it. If a stream fails after a user message,
Retry re-sends that last message without duplicating the user bubble.

Open Sessions to archive completed work, restore archived work, or bulk archive
and restore selected sessions. Open in Chat returns a selected session to the
working chat surface.

**Experimental:** Planning is reachable from the desktop navigation and
renders project dashboards, requirements, roadmap, checkpoints, execution,
verification, gap analysis, discussion, and project detail views — but parts
of the backing planning API are still incomplete, so some views do not load
live data yet. Treat Planning as an in-progress surface; ordinary
conversational work starts in Chat.

## 7. Export or Persist the Result

**Implemented:** Sessions and memory persist on the server. Desktop export
copies the current conversation as Markdown.

The server persists sessions, messages, knowledge facts, entity memories, and
workspace files in its configured instance data. Desktop settings persist
server entries, appearance, keybindings, notification preferences, and window
state under the user's desktop config directory.

In Chat, run `/export` from the command palette to copy the current
conversation to the clipboard as Markdown. The export contains user, assistant,
and system messages separated by Markdown dividers. If you invoke it outside
Chat or before any messages exist, the desktop shows a warning instead of
producing an empty export.

The Theke file view persists edited workspace files through the server API and
surfaces save conflicts, size-limit failures, and connection errors.

**Implemented outside the desktop:** The CLI includes session and agent export
commands for portable artifacts. Use those when you need a file on disk rather
than clipboard Markdown.

**Planned:** The desktop does not yet provide a full export dialog for choosing
Markdown versus JSON, selecting a destination file, or packaging a session with
its trace and memory evidence.

## Headless Fallback

Use the TUI for current public first-run, headless hosts, SSH-only work, or
Wayland remote-launch limitations. The TUI is a ratatui client over the same
server API and SSE events. It supports chat, planning, memory, metrics, ops,
session focus, agent focus, and setup wizard flows. The desktop remains the
v1.0 target app surface once the release path ships matching desktop artifacts.
