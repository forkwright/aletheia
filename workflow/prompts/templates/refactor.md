---
number: [N]
description: "Refactor: [what is being restructured]"
depends_on: []
model_tier: sonnet
blast_radius:
  - [every file that will be touched - refactors touch many files]
acceptance_criteria:
  - "cargo test --workspace --features test-core passes (no regressions)"
  - "cargo clippy --workspace --features test-core --all-targets -- -D warnings clean"
  - "cargo fmt --all -- --check passes"
  - "Public API surface of [crate] unchanged (or migration documented in CHANGELOG)"
  - "[Structural property the refactor is meant to achieve]"
---

# Standards

- `standards/STANDARDS.md` - information hierarchy and define-once principles
- `standards/RUST.md` - Rust-specific layout and visibility rules
- `standards/ARCHITECTURE.md` - dependency direction, crate boundaries
- `AGENTS.md` - where to add things, common mistakes

# Context

[Why this refactor is happening. What problem it solves. What the code looks
like before and what it should look like after.]

[If there was a prior research prompt: reference its findings here.]

# Blast radius

This refactor touches:

[List every file or directory. Refactors often have wide blast radius - be
complete. Files not listed are off-limits.]

**Public API impact:** [None | Changes documented in CHANGELOG | Migration
required - details below]

# Constraints

- Tests pass before and after. If a test must change, explain why in the commit body.
- No behavior changes, only structural changes. If behavior must change, that
  is a separate prompt.
- No new dependencies unless the refactor requires them. Justify any additions.
- Preserve all existing `WHY:`, `WARNING:`, and `SAFETY:` comments unless the
  code they annotate is removed.

# Acceptance criteria

1. `cargo test --workspace --features test-core` passes.
2. `cargo clippy --workspace --features test-core --all-targets -- -D warnings` clean.
3. `cargo fmt --all -- --check` passes.
4. [Structural criterion - e.g., "No import cycles in crates/mneme/"]
5. [API criterion - e.g., "SessionStore trait signature unchanged"]

# Task

[Describe the structural transformation. Be specific: which types move where,
which traits are extracted, which modules are split or merged.]

[If the refactor has multiple steps, order them so each step leaves the
codebase compilable. A refactor that breaks compilation partway through
cannot be reviewed or bisected.]

Commit each logical step separately. Each commit must pass `cargo check`.
Use `Gate-Passed: kanon 0.1.0` in the final commit body.
