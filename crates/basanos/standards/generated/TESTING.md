# Testing Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [TESTING/fragile-manifest-dir-path-math](#testingfragile-manifest-dir-path-math)
- [TESTING/ignore-no-issue](#testingignore-no-issue)
- [TESTING/no-benchmarks](#testingno-benchmarks)
- [TESTING/no-fuzz](#testingno-fuzz)
- [TESTING/no-nextest-config](#testingno-nextest-config)
- [TESTING/no-tests](#testingno-tests)
- [TESTING/skip-missing-fixture](#testingskip-missing-fixture)
- [TESTING/sleep-in-test](#testingsleep-in-test)
- [TESTING/tautological-test](#testingtautological-test)
- [TESTING/test-naming](#testingtest-naming)
- [TESTING/workspace-relative-fixture-path](#testingworkspace-relative-fixture-path)

## `TESTING/fragile-manifest-dir-path-math` {#testingfragile-manifest-dir-path-math}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/fragile-manifest-dir-path-math`
- See also: `TESTING/workspace-relative-fixture-path`

Tests must not climb from `CARGO_MANIFEST_DIR` with parent-directory path math to reach fixtures. Use a workspace_root() helper or crate-local fixture paths so tests keep working when crate layout changes.

### Examples

**Good:** Resolve crate-local fixtures without climbing through the workspace.

```text
PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/input.json")
```

**Bad:** Climb from CARGO_MANIFEST_DIR to a workspace-relative fixture path.

```text
PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .parent().expect("workspace")
    .parent().expect("checkout")
    .join("crates/pragma/testdata/input.json")
```

## `TESTING/ignore-no-issue` {#testingignore-no-issue}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/ignore-no-issue`
- See also: `TESTING/no-tests`

Ignored Rust tests must carry a tracking issue reference. A bare ignored test hides broken or flaky coverage indefinitely; the issue reference gives it an owner and a path back to execution.

### Examples

**Good:** Link an ignored test to the issue that tracks re-enabling it.

```text
#[ignore = "flaky: port conflict - see #456"]
fn retries_on_timeout() {}
```

**Bad:** Disable a test without any tracking reference.

```text
#[ignore]
fn retries_on_timeout() {}
```

## `TESTING/no-benchmarks` {#testingno-benchmarks}

- Severity: `info`
- Scope: `language:rust`
- Enforcer: `TESTING/no-benchmarks`
- See also: `TESTING/no-tests`

Rust workspaces should include a `benches/` directory with benchmark targets. Benchmarks catch performance regressions before they reach production and give optimization work an executable baseline.

### Examples

**Good:** Keep benchmark targets next to the workspace.

```text
benches/throughput.rs
```

**Bad:** Ship a workspace with no benchmark directory.

```text
[workspace]
members = ["crates/basanos"]
```

## `TESTING/no-fuzz` {#testingno-fuzz}

- Severity: `info`
- Scope: `language:rust`
- Enforcer: `TESTING/no-fuzz`
- See also: `TESTING/no-tests`

Rust workspaces should include a `fuzz/` directory with cargo-fuzz targets. Fuzz testing exercises edge cases and crash behavior that unit tests often miss.

### Examples

**Good:** Keep fuzz targets next to the workspace.

```text
fuzz/Cargo.toml
```

**Bad:** Ship a workspace with no fuzz target directory.

```text
[workspace]
members = ["crates/basanos"]
```

## `TESTING/no-nextest-config` {#testingno-nextest-config}

- Severity: `info`
- Scope: `language:rust`
- Enforcer: `TESTING/no-nextest-config`
- See also: `TESTING/no-tests`

Rust workspaces should include `.config/nextest.toml` so test execution has shared retry, timeout, and reporting policy. A checked-in nextest config keeps local and CI test behavior aligned.

### Examples

**Good:** Configure nextest for workspace test execution.

```text
.config/nextest.toml
```

**Bad:** Ship a Rust workspace without a nextest configuration.

```text
[workspace]
members = ["crates/basanos"]
```

## `TESTING/no-tests` {#testingno-tests}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/no-tests`

Library crates must have at least one inline test module or crate-level tests directory. A library entry point with no tests has unknown regression behavior and no executable contract for future changes.

### Examples

**Good:** Keep a library crate covered by inline or integration tests.

```text
#[cfg(test)]
mod tests {
    #[test]
    fn covers_public_behavior() {}
}
```

**Bad:** Ship a library entry point with no test module or tests directory.

```text
pub fn calculate_total(items: &[Item]) -> u64 { items.iter().map(Item::cost).sum() }
```

## `TESTING/skip-missing-fixture` {#testingskip-missing-fixture}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/skip-missing-fixture`
- See also: `TESTING/no-tests`

Tests must fail loudly when checked-in fixtures are missing. Silently returning early or skipping hides broken coverage and lets missing fixture files go undetected.

### Examples

**Good:** Fail loudly when a checked-in fixture is absent.

```text
assert!(fixture.exists(), "fixture must be checked in");
```

**Bad:** Return early when a missing fixture should fail the test.

```text
if !fixture.exists() { return; }
```

## `TESTING/sleep-in-test` {#testingsleep-in-test}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/sleep-in-test`
- See also: `TESTING/no-tests`

Tests must not depend on wall-clock sleeps for synchronization or timing. Use deterministic time control, condition variables, or explicit readiness signals so test duration and failure behavior are stable.

### Examples

**Good:** Use deterministic time control in tests.

```text
tokio::time::pause();
advance(Duration::from_secs(1)).await;
```

**Bad:** Sleep in test code and depend on wall-clock timing.

```text
std::thread::sleep(Duration::from_secs(1));
```

## `TESTING/tautological-test` {#testingtautological-test}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/tautological-test`
- See also: `TESTING/no-tests`

Tests must assert observable behavior instead of tautologies or empty verification. Assertions that compare a value to itself, check constants, or omit verification create false confidence while proving nothing about the code under test.

### Examples

**Good:** Assert observable behavior with distinct actual and expected values.

```text
assert_eq!(parse_status("ok"), Status::Ready);
```

**Bad:** Compare a value to itself so the assertion cannot fail.

```text
assert_eq!(status, status);
```

## `TESTING/test-naming` {#testingtest-naming}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TESTING/test-naming`
- See also: `TESTING/no-tests`

Test function names should describe the behavior under verification instead of carrying a generic `test_` prefix. Behavior-driven names make failures and coverage easier to scan.

### Examples

**Good:** Name the test after the behavior it verifies.

```text
fn parses_valid_input() {}
```

**Bad:** Prefix a test with a generic test_ marker.

```text
fn test_parse() {}
```

## `TESTING/workspace-relative-fixture-path` {#testingworkspace-relative-fixture-path}

- Severity: `info`
- Scope: `language:rust`
- Enforcer: `TESTING/workspace-relative-fixture-path`
- See also: `TESTING/fragile-manifest-dir-path-math`

Tests should reference crate-local fixtures relative to the current crate instead of reaching through workspace-root paths. Workspace-relative crate fixture paths couple tests to the checkout layout and break when crates move.

### Examples

**Good:** Resolve a crate-local fixture relative to the current crate.

```text
PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/input.json")
```

**Bad:** Reach back through the workspace to the current crate's testdata.

```text
workspace_root.join("crates/pragma/testdata/input.json")
```

