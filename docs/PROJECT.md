# Aletheia - Project Plan

Roadmap and design intent for Aletheia's evolution.

## Vision

Aletheia is a distributed cognition system: a team of AI agents (nous) that operate as cognitive extensions of their human operator. Always-on, Signal-native, self-improving.

Five design pressures shape every decision:

1. **Single static binary.** `scp + systemctl`. No runtime dependencies beyond glibc.
2. **Portable by default.** Runs on any Linux and macOS. No OS-specific dependencies in core.
3. **True parallelism.** Multiple nous on Tokio threads, not interleaved on one event loop.
4. **Every decision deliberate.** Nothing carried forward unexamined.
5. **Correct primitives.** No event loop blocking, no GC pauses, no per-request DB connections.

The always-on ambient model shapes every design decision: Signal-native, independent routines, household access, autonomous background cycles.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full module map, crate tree, oikos instance structure, dependency graph, and release profile.

See [TECHNOLOGY.md](TECHNOLOGY.md) for technology decisions, dependency policy, pinning rules, and crate-to-module mapping.

## Milestones

| Milestone | Summary | Status |
|-----------|---------|--------|
| M0a | Oikos migration — instance structure, tool resolution, config cascade | Done |
| M0b | Foundation crates — koina errors, taxis config, newtypes, tracing | Done |
| M1 | Memory + LLM client — Anthropic streaming, CozoDB absorption, hybrid recall, embeddings | Done |
| M2 | Agent core — tool registry, nous pipeline, bootstrap assembly, execute stage | Done |
| M3 | Gateway + auth + channels — pylon HTTP, symbolon JWT, agora Signal, end-to-end wiring | Done |
| M4 | Multi-nous + background — NousActor, daemon, dianoia planning, cross-nous sessions | Done |
| M5 | Plugins + portability — WASM plugins, agent export/import (autarkeia #507, skills #676-#696) | In progress |
| M6 | Platform extensions — composable ops, A2A interop, eBPF sensing, NixOS module | Backlog |

See `Cargo.toml` workspace members for current crate inventory.

## Interfaces

| Interface | Status | Notes |
|-----------|--------|-------|
| TUI | Active | Terminal dashboard, rich markdown, session management |
| Signal | Active | 15 `!` commands, always-on ambient messaging |
| HTTP API | Active | REST on port 18789, SSE streaming |
| Desktop app | Planned | Design knowledge captured in planning docs |

## Related Documents

| Document | Purpose |
|----------|---------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module map, crate tree, oikos structure, dependency rules |
| [TECHNOLOGY.md](TECHNOLOGY.md) | Technology decisions, dependency policy, crate-to-module mapping |
| [STANDARDS.md](STANDARDS.md) | Code standards and project governance |
| [CONFIGURATION.md](CONFIGURATION.md) | Config cascade, environment variables, per-nous overrides |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Installation, system requirements, instance setup |
| [RELEASING.md](RELEASING.md) | Release process, versioning, binary builds |
| [RUNBOOK.md](RUNBOOK.md) | Operational procedures, maintenance tasks |
