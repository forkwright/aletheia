# Logging and Observability Standards

> Additive to README.md. Read that first. Everything here covers structured logging, log levels, and what to log.

---

## Universal Logging Rules

- **Structured logging.** Key-value pairs, not interpolated strings. `session_id=abc123 action=load_config status=ok` not `"Loaded config for session abc123 successfully."`
- **Log at the handling site.** Not at the throw site. The handler has context about what to do with the error.
- **Log levels mean something:**

| Level | When |
|-------|------|
| `error` | Something failed that requires attention. Data loss, service degradation, unrecoverable state. |
| `warn` | Something unexpected happened but was handled. Approaching limits, deprecated usage. |
| `info` | Normal operations worth recording. Service start/stop, config loaded, connection established. |
| `debug` | Detailed operational data. Request/response details, state transitions, intermediate calculations. |
| `trace` | High-volume diagnostic data. Per-iteration values, wire-level protocol details. |

- **Never log secrets.** Credentials, tokens, API keys, passwords. Redact or omit.
- **Never log PII at info level or above.** User emails, names, IPs are debug-level at most, and only when necessary for diagnosis.
- **Handle errors once.** Each error is either logged **or** propagated — never both. Logging at the origin and propagating with context produces duplicate noise. Log at the point where the error is finally handled.
- **Guard expensive construction.** Don't compute values for log messages that won't be emitted at the current level. Check the level before building the message, or use lazy evaluation.
- **Include correlation IDs.** Every request, session, or operation chain carries an ID that appears in all related log entries.

## What to Log

- Service startup and shutdown with configuration summary
- External service connections (established, lost, reconnected)
- Authentication events (success, failure, token refresh)
- Error handling decisions (what was caught, what was done about it)
- Configuration changes at runtime

## What Not to Log

- Routine success on hot paths (every request succeeded, every query returned)
- Full request/response bodies at info level (use debug)
- Redundant messages (logging both the throw and the catch of the same error)
