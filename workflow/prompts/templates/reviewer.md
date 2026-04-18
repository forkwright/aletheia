---
number: [N]
description: "QA review: [title of the PR being reviewed]"
depends_on: [[N-1]]   # the coder prompt this reviews
model_tier: opus      # semantic review requires Opus
blast_radius:
  - [same blast radius as the coder prompt]
acceptance_criteria:
  - "QA verdict emitted as Pass, Partial, or Fail with evidence"
  - "Each acceptance criterion from PR evaluated independently"
---

# Role

Evaluate a pull request against its stated acceptance criteria. Ground every
judgment in specific code from the diff. Evaluate each criterion independently.

# Standards

Review against: `standards/STANDARDS.md`, `standards/RUST.md`, `AGENTS.md`,
and the PR's own acceptance criteria (listed below).

# Context

PR #[number]: [PR title]
Branch: [branch name]
Author prompt: #[N-1]

[Brief description of what the PR is supposed to do.]

# Acceptance criteria to evaluate

[Copy the acceptance criteria from the coder prompt. These are the criteria
the worker was supposed to satisfy. Evaluate each one.]

1. [Criterion 1]
2. [Criterion 2]
3. `cargo test -p [crate]` passes.
4. `cargo clippy --workspace --features test-core --all-targets -- -D warnings` clean.
5. `cargo fmt --all -- --check` passes.

# Mechanical checks

Before semantic review, verify:

- [ ] No files outside blast radius modified
- [ ] No `unwrap()` in library code
- [ ] No bare `TODO` or `FIXME` without issue numbers
- [ ] No commented-out code blocks
- [ ] No `#[allow]` without `#[expect(lint, reason = "...")]` replacement

Any mechanical failure → verdict is `Fail`, skip semantic review.

# Output format

```
## Mechanical

[ ] blast-radius clean
[ ] no unwrap in library code
[ ] no bare TODO/FIXME
[ ] no commented-out blocks
[ ] no #[allow] without expect

Mechanical verdict: PASS | FAIL

---

## Criteria

### 1. [Criterion text]
Verdict: PASS | FAIL
Evidence: [specific file, line, or observation]

[Repeat for each criterion]

---

## Overall verdict

PASS | PARTIAL | FAIL

Reasons:
- [concise bullet per failure or concern]
```

# Task

Review PR #[number] against the criteria above. Emit the output in the format
specified. Do not suggest improvements outside the stated criteria — scope that
to a follow-up issue.
