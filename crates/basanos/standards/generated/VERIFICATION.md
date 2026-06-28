# Verification Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [VERIFICATION/claim-requires-fixture](#verificationclaim-requires-fixture)
- [VERIFICATION/self-grade-prohibited](#verificationself-grade-prohibited)

## `VERIFICATION/claim-requires-fixture` {#verificationclaim-requires-fixture}

- Severity: `warning`
- Scope: `universal`
- See also: `VERIFICATION/self-grade-prohibited`

A defect fix cannot be verified without a failing fixture. Before the fix can be merged, there must exist a test, script, or reproduction that fails on the unfixed code and passes on the fixed code. A claim without a failing fixture cannot be passed — there is no way to detect a regression.

### Examples

**Good:** Attach a failing fixture that proves the defect before marking a fix complete.

```text
# Before fix: tests/regression/issue_438.rs fails
# After fix: tests/regression/issue_438.rs passes
# Fixture committed at tests/regression/issue_438.rs
```

**Bad:** Close a bug with no test that would fail if the bug were reintroduced.

```text
Fixed by removing the off-by-one in parser.rs:42. No regression test added.
```

## `VERIFICATION/self-grade-prohibited` {#verificationself-grade-prohibited}

- Severity: `error`
- Scope: `universal`
- See also: `VERIFICATION/claim-requires-fixture`

A worker cannot self-grade its own claim. The agent or operator that made the fix must not also be the one who marks it verified. Verification requires reproduction: a different party must reproduce the original defect, confirm the fix addresses it, and only then close the issue. A steward rubber-stamping a claim without reproduction is not verification.

### Examples

**Good:** Have a different agent reproduce the bug before marking the fix verified.

```text
Worker: submitted fix for #438.
Verifier: reproduced original bug on main, applied fix, confirmed bug absent. Issue verified closed.
```

**Bad:** Close an issue by asking the same agent that fixed it to confirm it works.

```text
Worker: I fixed #438. The test now passes. Marking as verified.
```

