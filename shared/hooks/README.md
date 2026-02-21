# Hooks

Declarative shell hooks that fire at runtime lifecycle events.

## How It Works

Drop a `.yaml` file in this directory. Each file defines a hook that runs a shell command when an event fires on the internal event bus. No TypeScript required.

## Hook Definition Format

```yaml
name: my-hook              # unique identifier
event: turn:after           # which event to listen on
description: Log every turn # optional description
enabled: true               # default: true. Set false to disable without deleting.

handler:
  type: shell               # currently only "shell" supported
  command: /path/to/script  # absolute path to executable
  args: ["{{sessionId}}", "{{nousId}}"]  # template variables from event payload
  timeout: 30s              # max execution time (ms, s, m). Default: 30s
  failAction: warn          # warn | block | silent. Default: warn
  env:                      # optional extra environment variables
    MY_VAR: my-value
  cwd: /some/dir            # optional working directory

# Optional: restrict to specific agents
nousFilter: [demiurge, akron]
```

## Supported Events

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
| `boot:start` | — | Runtime starting |
| `boot:ready` | port, tools, plugins | Runtime ready |
| `config:reloaded` | added, removed | Config hot-reloaded |

## Handler Protocol

Shell handlers receive the full event payload as JSON on **stdin**. This means your script can read structured data:

```bash
#!/bin/bash
# Read event payload from stdin
PAYLOAD=$(cat)
SESSION_ID=$(echo "$PAYLOAD" | jq -r '.sessionId')
echo "Processing session: $SESSION_ID"
```

**Environment variables** are always set:
- `ALETHEIA_HOOK_NAME` — the hook's name
- `ALETHEIA_HOOK_EVENT` — the event that triggered it
- Plus any custom env vars from the `env:` field

**Exit codes:**
- `0` — success
- `1` — warning (logged if failAction is not "silent")
- `2+` — error (always logged unless "silent")

## Template Variables

Args and the command itself support `{{variable}}` substitution from the event payload:

- `{{sessionId}}` — session ID
- `{{nousId}}` — agent ID
- `{{toolName}}` — tool name (tool events only)
- `{{timestamp}}` — event timestamp
- Nested: `{{session.id}}` for nested objects

Missing variables resolve to empty string.

## Per-Agent Hooks

Hooks in `shared/hooks/` apply globally. For agent-specific hooks, create a `hooks/` directory in the agent's workspace:

```
nous/demiurge/hooks/craft-journal.yaml
nous/akron/hooks/maintenance-log.yaml
```

Or use `nousFilter` in a global hook to restrict which agents trigger it.

## Allowed Script Extensions

For security, only these file extensions are allowed as hook commands:
`.sh`, `.py`, `.js`, `.ts`, `.rb`, `.pl`

Commands without extensions (e.g., `/usr/bin/curl`) are also allowed.

## Fail Actions

- **warn** (default) — log a warning on non-zero exit, don't affect the event
- **silent** — swallow all errors silently
- **block** — log an error (future: may block the triggering operation)

## Examples

See `_examples/` in this directory for starter hooks.
