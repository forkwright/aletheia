# _llm/

On-demand reference for AI agents. Not always-loaded — load the file relevant to your task.

CLAUDE.md = instructions (always loaded, short). This directory = reference (on demand, structured).

## Loading order

1. **Cold start:** `architecture.toml` — crate tree, layers, dependency direction
2. **Working on a crate:** per-crate `CLAUDE.md` (auto-loaded by Claude Code)
3. **Turn pipeline:** `turn-pipeline.toml` — end-to-end message flow across crates
4. **CLI or API work:** `api.toml` — subcommands and HTTP endpoints
5. **Metrics/tracing:** `observability.toml` — all metrics, spans, log events by crate
6. **Why was X chosen:** `decisions.toml` — technology decisions with rationale

## Format

TOML for structured data (token-efficient, machine-parseable). Canonical sources are the `docs/` markdown files — these are compressed views, not replacements. When in doubt, read the linked doc.
