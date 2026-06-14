# Aletheia

*ἀλήθεια: un-concealment. Truth as revealing, not correspondence.*

Distributed cognition system. Multiple AI agents working in concert with a human operator to hold complexity, surface patterns, and persist understanding across sessions. Not an assistant. An architecture for thinking together.

Five design pressures shape every decision:

1. **Single static binary.** `scp + systemctl`. No runtime dependencies beyond glibc for core functionality. Pandoc (>= 3.0) is an optional runtime dependency for document export formats: `docx`, `html`, `md`, `latex`, `epub`. PDF export uses the in-process Typst fast-lane and does not require Pandoc.
2. **Portable by default.** Runs on any Linux and macOS. No OS-specific dependencies in core.
3. **True parallelism.** Multiple nous on Tokio threads, not interleaved on one event loop.
4. **Every decision deliberate.** Nothing carried forward unexamined.
5. **Correct primitives.** No event loop blocking, no GC pauses, no per-request DB connections.

The always-on ambient model shapes every design decision: Signal-native, independent routines, household access, autonomous background cycles.

Naming: Greek terminology carries more signal than English equivalents. See [lexicon.md](lexicon.md) for the crate registry.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full module map, crate tree, oikos instance structure, dependency graph, and release profile.

See [TECHNOLOGY.md](TECHNOLOGY.md) for technology decisions, dependency policy, pinning rules, and crate-to-module mapping.

## Milestones

Planning and milestones are tracked in the internal project roadmap.

## Interfaces

Current first-run default: TUI. The desktop app is the v1.0 target surface and
is not the default public onboarding path until release artifacts include the
desktop app.

| Interface | Status | Notes |
|-----------|--------|-------|
| TUI | Active | Current supported first-run interface; terminal dashboard with rich markdown and session management |
| Signal | Active | Always-on ambient messaging; inbound messages delivered as plain conversational turns |
| HTTP API | Active | REST on port 18789, SSE streaming |
| Desktop app | In progress | v1.0 target surface; source-built preview, not current default onboarding |

## Related documents

| Document | Purpose |
|----------|---------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module map, crate tree, oikos structure, dependency rules |
| [TECHNOLOGY.md](TECHNOLOGY.md) | Technology decisions, dependency policy, crate-to-module mapping |
| [standards/README.md](../standards/README.md) | Code standards and project governance |
| [CONFIGURATION.md](CONFIGURATION.md) | Config cascade, environment variables, per-nous overrides |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Installation, system requirements, instance setup |
| [RELEASING.md](RELEASING.md) | Release process, versioning, binary builds |
| [RUNBOOK.md](RUNBOOK.md) | Operational procedures, maintenance tasks |
