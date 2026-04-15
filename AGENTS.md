# AGENTS.md — Aletheia

Condensed guide for dispatch agents (Kimi, local LLM). Read CLAUDE.md for full conventions.

## Build

```bash
cargo check -p <crate>                 # fast compile check
cargo test -p <crate>                  # single crate tests
cargo clippy --workspace               # lint (zero warnings)
cargo test --workspace                 # all tests (~5,400)
```

Desktop crate (proskenion) is excluded from workspace. Build standalone:
```bash
cargo check --manifest-path crates/theatron/proskenion/Cargo.toml
```

## Crate map

| Crate | Purpose |
|-------|---------|
| eidos | Shared types, domain primitives, zero deps |
| koina | Constants, HTTP paths, shared config types |
| taxis | Configuration (figment TOML cascade) |
| graphe | Session/message SQLite persistence |
| krites | Vendored Datalog engine (CozoDB fork) |
| episteme | Memory pipeline: extraction, recall, consolidation, knowledge store |
| mneme | Memory facade re-exporting graphe + episteme + krites |
| hermeneus | LLM client: Anthropic API, streaming, retries, health |
| organon | Tool registry and built-in tool implementations |
| nous | Agent actor system, turn loop, orchestration |
| pylon | Axum HTTP server, API handlers, SSE streaming |
| daemon | Background task runners (maintenance, drift detection) |
| symbolon | Credential management, OAuth, API keys |
| melete | Autonomous action planning |
| dianoia | Workspace planning (roadmaps, phases) |
| agora | Message bus |
| theatron | UI umbrella: skene (core), koilon (TUI), proskenion (desktop) |
| thesauros | Storage abstraction (fjall KV) |
| eval | Behavioral evaluation scenarios |

## Key patterns

- Errors: `snafu` with `.context()`. No `unwrap()` in library code.
- Lints: `#[expect(lint, reason = "...")]` not `#[allow]`.
- Visibility: `pub(crate)` default. `pub` only for cross-crate API.
- Naming: Greek names per gnomon system. See docs/lexicon.md.
- Commits: `type(scope): description`. Scope is crate name.

## Gate trailer

All PRs need `Gate-Passed: kanon 0.1.0` in a commit body.
Desktop PRs: `Gate-Passed: kanon 0.1.0 desktop-only (excluded from workspace build)`.

## Skip Setup section

If you're reading this in a worktree, skip setup — you're already there.
