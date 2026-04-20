---
number: [N]
description: "[Short title for this task]"
depends_on: []
model_tier: sonnet
blast_radius:
  - [crates/affected-crate/src/]
acceptance_criteria:
  - "cargo test -p [crate] passes"
  - "cargo clippy --workspace --features test-core --all-targets -- -D warnings clean"
  - "cargo fmt --all -- --check passes"
  - "[Feature-specific criterion]"
---

# Standards

- `standards/STANDARDS.md` - universal principles
- `standards/RUST.md` - Rust-specific rules
- `AGENTS.md` - build commands, key patterns, where to add things

# Context

[Prior work, related PRs, or handoff from a prior prompt in this wave.
Omit if this is the first prompt in a plan.]

# Blast radius

Changes are scoped to:

[List the same files/directories as the frontmatter blast_radius. Be explicit
about what is OUT of scope if there is a risk of confusion.]

Files outside this scope require a separate prompt and separate PR.

# Acceptance criteria

1. [Restate each criterion from the frontmatter. One sentence each. Make them
   testable: "X passes" or "Y is present in Z".]
2. `cargo test -p [crate]` passes.
3. `cargo clippy --workspace --features test-core --all-targets -- -D warnings` clean.
4. `cargo fmt --all -- --check` passes.

# Task

[The specific work to do. Describe the outcome, not the approach. The worker
chooses the approach.]

[If there are multiple sub-tasks, list them in priority order. The worker
completes them in order and stops if one fails.]

Commit with `Gate-Passed: kanon 0.1.0` in the commit body.
