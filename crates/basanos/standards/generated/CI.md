# Continuous Integration Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CI/kanon-ci-concurrency-caps](#cikanon-ci-concurrency-caps)
- [CI/kanon-ci-lint-stage-required](#cikanon-ci-lint-stage-required)
- [CI/no-ci-workflows](#cino-ci-workflows)
- [CI/no-clippy-check](#cino-clippy-check)
- [CI/no-deny-check](#cino-deny-check)
- [CI/no-format-check](#cino-format-check)
- [CI/no-op-command](#cino-op-command)
- [CI/no-test-step](#cino-test-step)

## `CI/kanon-ci-concurrency-caps` {#cikanon-ci-concurrency-caps}

- Severity: `error`
- Scope: `language:toml`
- Enforcer: `CI/kanon-ci-concurrency-caps`
- See also: `CI/no-op-command`

Kanon CI cargo stages must cap cargo and nextest concurrency. Fleet runners expose many logical CPUs, so uncapped build, check, clippy, test, or nextest stages can exceed the host memory budget.

### Examples

**Good:** Cap cargo and nextest parallelism in kanon CI stages.

```text
cmd = "cargo test --workspace --jobs 8"
nextest = "cargo nextest run --build-jobs 8 --test-threads 8"
```

**Bad:** Let a cargo stage use the host's full parallelism.

```text
cmd = "cargo test --workspace"
```

## `CI/kanon-ci-lint-stage-required` {#cikanon-ci-lint-stage-required}

- Severity: `error`
- Scope: `language:toml`
- Enforcer: `CI/kanon-ci-lint-stage-required`
- See also: `CI/no-clippy-check`

Kanon CI pipelines must include a lint stage or lint command. Tests catch regressions, but linting catches mechanical rule violations and structural issues before they land.

### Examples

**Good:** Declare a lint stage in kanon CI.

```text
[stages.lint]
cmd = "cargo clippy --workspace -- -D warnings"
```

**Bad:** Run tests without any lint stage.

```text
[stages.test]
cmd = "cargo test --workspace"
```

## `CI/no-ci-workflows` {#cino-ci-workflows}

- Severity: `warning`
- Scope: `language:yaml`
- Enforcer: `CI/no-ci-workflows`
- See also: `CI/no-clippy-check`, `CI/no-format-check`, `CI/no-test-step`

GitHub-hosted workspaces must include a `.github/workflows/` directory. CI catches regressions before merge; without workflow files, broken code can reach the default branch unchecked.

### Examples

**Good:** Keep a GitHub Actions workflow directory at the workspace root.

```text
.github/workflows/checks.yml
```

**Bad:** Publish a GitHub-hosted workspace without CI workflows.

```text
Cargo.toml
src/lib.rs
```

## `CI/no-clippy-check` {#cino-clippy-check}

- Severity: `warning`
- Scope: `language:yaml`
- Enforcer: `CI/no-clippy-check`
- See also: `CI/no-format-check`, `CI/no-test-step`

CI workflows must include a Clippy lint step. Clippy catches correctness, maintainability, and performance issues that still compile, so the lint gate must run before code can merge.

### Examples

**Good:** Run the Rust lint gate in CI.

```text
jobs:
  clippy:
    steps:
      - run: cargo clippy -- -D warnings
```

**Bad:** Let code merge without the Clippy lint step.

```text
jobs:
  test:
    steps:
      - run: cargo test --workspace
```

## `CI/no-deny-check` {#cino-deny-check}

- Severity: `warning`
- Scope: `language:yaml`
- Enforcer: `CI/no-deny-check`

CI workflows must include a cargo-deny dependency audit step. Without cargo-deny, license violations and vulnerable dependencies can enter before the gate has checked advisories, licenses, bans, and sources.

### Examples

**Good:** Run the dependency audit in CI.

```text
jobs:
  audit:
    steps:
      - run: cargo deny check
```

**Bad:** Omit the dependency audit from the workflow.

```text
jobs:
  test:
    steps:
      - run: cargo test --workspace
```

## `CI/no-format-check` {#cino-format-check}

- Severity: `warning`
- Scope: `language:yaml`
- Enforcer: `CI/no-format-check`
- See also: `CI/no-clippy-check`, `CI/no-test-step`

CI workflows must include a formatting check step. Formatting gates keep style drift from creating noisy diffs and merge conflicts over whitespace or formatter-owned layout.

### Examples

**Good:** Run the formatting gate in CI.

```text
jobs:
  fmt:
    steps:
      - run: cargo fmt --check
```

**Bad:** Let unformatted code merge without a CI check.

```text
jobs:
  test:
    steps:
      - run: cargo test --workspace
```

## `CI/no-op-command` {#cino-op-command}

- Severity: `error`
- Scope: `project:ci`
- Enforcer: `CI/no-op-command`

CI stages must run a real quality check instead of a pure no-op such as `true`, `:`, or `exit 0`. No-op commands create fake gates that always pass while bypassing the verification the stage was meant to enforce.

### Examples

**Good:** Run a real CI quality check.

```text
[[stages]]
name = "test"
cmd = "cargo test --workspace"
```

**Bad:** Configure a CI stage that always passes without doing work.

```text
[[stages]]
name = "test"
cmd = "true"
```

## `CI/no-test-step` {#cino-test-step}

- Severity: `warning`
- Scope: `language:yaml`
- Enforcer: `CI/no-test-step`
- See also: `CI/no-clippy-check`, `CI/no-format-check`

CI workflows must execute the test suite before code can merge. Tests that never run in CI provide no regression protection and let broken behavior reach the default branch unchecked.

### Examples

**Good:** Run the test suite in CI.

```text
jobs:
  test:
    steps:
      - run: cargo test --workspace
```

**Bad:** Let code merge without executing tests in CI.

```text
jobs:
  fmt:
    steps:
      - run: cargo fmt --check
```

