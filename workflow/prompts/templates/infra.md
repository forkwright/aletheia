---
number: [N]
description: "Infra: [CI change, tooling update, or standards edit]"
depends_on: []
model_tier: haiku     # mechanical/config work; use sonnet for complex CI logic
blast_radius:
  - [.github/workflows/ | scripts/ | standards/ | Cargo.toml | etc.]
acceptance_criteria:
  - "[CI check passes | script produces correct output | standard is consistent]"
  - "cargo clippy --workspace --features test-core --all-targets -- -D warnings clean (if Rust touched)"
---

# Role

Infrastructure, tooling, CI, or standards work. No product feature changes.

# Standards

- `standards/STANDARDS.md` - repository hygiene, dead weight rules
- `standards/CI.md` - CI tooling and workflow conventions
- `standards/SHELL.md` - shell script requirements (set -euo pipefail, quoting)
- `standards/WRITING.md` - prose standards for documentation edits

# Context

[Why this infrastructure change is needed. What is broken or missing. What
the correct state looks like.]

# Blast radius

Changes are scoped to:

[List affected files. Infrastructure changes often touch configuration files,
scripts, and CI definitions. Be specific.]

**No product code changes.** If product code must change to support this
infrastructure work, that is a separate prompt.

# Acceptance criteria

1. [Primary criterion - e.g., "CI workflow lint job runs and passes"]
2. [Secondary criterion - e.g., "Script exits 0 on valid input, non-zero on invalid"]
3. [Standards criterion - e.g., "No information is duplicated between the two docs"]

# Checklist (infra-specific)

- [ ] Shell scripts: `set -euo pipefail` on line 2, all variables quoted
- [ ] CI workflows: pinned action versions (`uses: actions/checkout@v4.2.2`)
- [ ] Documentation edits: no words from the WRITING.md banned list
- [ ] No dead weight added (scripts, empty dirs, placeholder docs)

# Task

[Describe the infrastructure change. Reference the specific files to create,
modify, or delete.]

[For standards edits: state exactly what changes and why, and confirm that
the information is not duplicated elsewhere in the standards tree.]

Commit with conventional format: `chore(scope): description` or `ci(scope): description`.
