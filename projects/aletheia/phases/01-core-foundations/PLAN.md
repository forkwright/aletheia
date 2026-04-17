# Phase 01: Core foundations

## Goal
Establish the shared type system, error taxonomy, tracing, and identifier infrastructure that all downstream crates depend on.

## Success criteria
- All crates share a single error taxonomy with source chains
- ULID generation produces lexicographically sortable IDs
- Tracing initialization supports JSON and pretty formats
- Configuration defaults are centralized and versioned

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| All crates share a single error taxonomy with source chains | `cargo check` shows duplicate error definitions in any two crates |
| ULID generation produces lexicographically sortable IDs | Sorting 1000 ULIDs yields a different order than their creation timestamps |
| Tracing initialization supports JSON and pretty formats | Integration test emits invalid JSON or crashes with pretty format enabled |
| Configuration defaults are centralized and versioned | Two crates define the same default constant independently |

## Scope

### In scope
- koina crate: errors, tracing, IDs, filesystem wrappers
- taxis crate: config schema, path resolution, secret references
- CI pipeline: fmt, clippy, test gates

### Out of scope
- Application-level features (HTTP, memory, tools)
- Packaging and release automation

## Requirements
- REQ-01: Error types use `snafu` with `.context()` and no bare `unwrap()` in library code
- REQ-02: ULID generation is deterministic given timestamp and entropy input
- REQ-03: Tracing subscriber supports env-filter and structured JSON output
- REQ-04: Config defaults are documented with a "WHY" comment

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| ID format | ULID over UUID | Lexicographic sortability simplifies debugging and log correlation |
| Error library | snafu over thiserror | `.context()` pattern is mandatory for source chains |
| Time library | jiff over chrono | Smaller dependency tree, better timezone handling |

## Open questions
- Should we add a compact string type for small strings? (Resolved: yes, compact_str)
- What is the MSRV policy? (Resolved: latest stable minus 6 weeks)

## Dependencies
- Rust toolchain 1.94+
- cargo-deny configured in CI
