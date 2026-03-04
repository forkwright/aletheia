# Aletheia - Project Plan

Roadmap and design intent for Aletheia's evolution from TypeScript prototype to Rust production system.

## Vision

Aletheia is a distributed cognition system: a team of AI agents (nous) that operate as cognitive extensions of their human operator. Always-on, Signal-native, self-improving.

The TypeScript + Python implementation works but carries inherited decisions from its OpenClaw fork lineage. The Rust rewrite is driven by five pressures:

1. **Single static binary.** `scp + systemctl`. No Node, no Python venv, no npm, no bundling.
2. **Portable by default.** Runs on any Linux and macOS. No OS-specific dependencies in core.
3. **True parallelism.** Multiple nous on Tokio threads, not interleaved on one event loop.
4. **No inherited debt.** Every decision deliberate, nothing carried forward unexamined.
5. **Correct primitives.** No event loop blocking, no GC pauses, no per-request DB connections.

The always-on ambient model shapes every design decision: Signal-native, independent routines, household access, autonomous background cycles.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full module map, crate tree, oikos instance structure, dependency graph, and release profile.

See [TECHNOLOGY.md](TECHNOLOGY.md) for technology decisions, dependency policy, pinning rules, and crate-to-module mapping.

## Milestones

| Milestone | Summary | Status |
|-----------|---------|--------|
| M0a | Oikos migration (TypeScript) - instance structure, tool resolution, config cascade | Done |
| M0b | Foundation crates (Rust) - koina errors, taxis config, newtypes, tracing | Done |
| M1 | Memory + LLM client - Anthropic streaming, CozoDB absorption, hybrid recall, embeddings | Done |
| M2 | Agent core - tool registry, nous pipeline, bootstrap assembly, execute stage | Done |
| M3 | Gateway + auth + channels - pylon HTTP, symbolon JWT, agora Signal, end-to-end wiring | Done |
| M4 | Multi-nous + background - NousActor, daemon, dianoia planning, cross-nous sessions | In progress |
| M5 | Plugins + portability + cutover - WASM plugins, agent export, TS retirement | Not started |
| M6 | Platform extensions - composable ops, A2A interop, eBPF sensing, NixOS module | Backlog |

See `Cargo.toml` workspace members for current crate inventory.

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
