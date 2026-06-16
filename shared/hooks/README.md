# shared/hooks

> **Status: unsupported before v1.0.**  
> The YAML and shell files in this directory are **examples and templates only**. They are **not loaded, registered, or executed** by the current Aletheia runtime.

## What runs today

The implemented hook surface is the in-process `TurnHook` trait in [`crates/nous/src/hooks`](../../crates/nous/src/hooks). Hooks run inside the `nous` agent pipeline via `HookRegistry` at these points:

| Hook point | Purpose |
|------------|---------|
| `before_query` | Before the model call; can modify the system prompt or inject messages. |
| `on_turn_complete` | After the model responds; for audit, logging, and metrics. |
| `before_tool` | Before a tool runs; can approve or deny the call. |
| `after_tool` | After a tool runs; for result post-processing. |
| `session_start` | When a new `nous` session starts. |
| `before_compact` | Before context distillation. |
| `after_compact` | After context distillation. |

These hooks are built-in Rust implementations configured through the `[hooks]` section of agent config (for example, `cost_control`, `scope_enforcement`, `correction_hooks_enabled`, `self_audit_enabled`, and `working_checkpoint_enabled`).

## Future contract (post v1.0)

The rest of this document describes the *intended* declarative shell-hook contract. It is **not implemented today** and may change before stabilization. The event names and payload fields below are from the planned external event bus, not the in-process hook points above.

### Hook definition format

```yaml
name: my-hook              # unique identifier
event: turn:after           # which event to listen on
description: Log every turn # optional description
enabled: true               # default: true. Set false to disable without deleting.

handler:
  type: shell               # only "shell" planned
  command: /path/to/script  # absolute path to executable
  args: ["{{sessionId}}", "{{nousId}}"]  # template variables from event payload
  timeout: 30s              # max execution time (ms, s, m). Default: 30s
  failAction: warn          # warn | block | silent. Default: warn
  env:                      # optional extra environment variables
    MY_VAR: my-value
  cwd: /some/dir            # optional working directory

# Optional: restrict to specific agents
nousFilter: [agent-a, agent-b]
```

### Planned events

| Event | Payload Fields | When |
|-------|---------------|------|
| `turn:before` | nousId, sessionId, sessionKey, channel | Before API call |
| `turn:after` | nousId, sessionId, inputTokens, outputTokens, toolCalls | After API response |
| `tool:called` | nousId, sessionId, toolName, durationMs | Tool executed successfully |
| `tool:failed` | nousId, sessionId, toolName, error, durationMs | Tool execution failed |
| `distill:before` | sessionId, nousId, distillationNumber | Before context distillation |
| `distill:after` | sessionId, nousId, distillationNumber, tokensBefore, tokensAfter, factsExtracted | After distillation |
| `session:created` | sessionId, nousId | New session created |
| `session:archived` | sessionId | Session archived |
| `memory:added` | nousId, count | Facts extracted and stored |
| `boot:start` | (none) | Runtime starting |
| `boot:ready` | port, tools, plugins | Runtime ready |
| `config:reloaded` | added, removed | Config hot-reloaded |

### Planned handler protocol

If this format is implemented in the future, shell handlers would receive the full event payload as JSON on **stdin**:

```bash
#!/bin/bash
# Read event payload from stdin
PAYLOAD=$(cat)
SESSION_ID=$(echo "$PAYLOAD" | jq -r '.sessionId')
echo "Processing session: $SESSION_ID"
```

**Environment variables** would be set:
- `ALETHEIA_HOOK_NAME` - the hook's name
- `ALETHEIA_HOOK_EVENT` - the event that triggered it
- Plus any custom env vars from the `env:` field

**Exit codes:**
- `0` - success
- `1` - warning (logged if failAction is not "silent")
- `2+` - error (always logged unless "silent")

### Template variables

Args and the command itself would support `{{variable}}` substitution from the event payload:

- `{{sessionId}}` - session ID
- `{{nousId}}` - agent ID
- `{{toolName}}` - tool name (tool events only)
- `{{timestamp}}` - event timestamp
- Nested: `{{session.id}}` for nested objects

Missing variables would resolve to an empty string.

### Per-agent hooks

Hooks in `shared/hooks/` would apply globally. For agent-specific hooks, the planned layout is a `hooks/` directory in the agent's workspace:

```text
nous/<agent-a>/hooks/craft-journal.yaml
nous/<agent-b>/hooks/maintenance-log.yaml
```

Or a global hook could use `nousFilter` to restrict which agents trigger it.

### Allowed script extensions

If implemented, only these file extensions would be allowed as hook commands:
`.sh`, `.rb`, `.pl`

Commands without extensions (for example, `/usr/bin/curl`) would also be allowed.

### Fail actions

- **warn** (default) - log a warning on non-zero exit, do not affect the event
- **silent** - swallow all errors silently
- **block** - log an error (future: may block the triggering operation)

> **Warning:** `block` is not operational today. The current runtime does not execute external shell hooks, so these files cannot approve, deny, or protect against any operation.

### Examples and templates

See `_examples/` and `_templates/` for starter files. They are **not active today** and exist only to illustrate the planned contract.
