# QA Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [QA/audit-produces-structured-issues](#qaaudit-produces-structured-issues)
- [QA/verify-dont-trust](#qaverify-dont-trust)

## `QA/audit-produces-structured-issues` {#qaaudit-produces-structured-issues}

- Severity: `warning`
- Scope: `universal`
- See also: `QA/verify-dont-trust`

Every audit finding must become a structured issue with: what is wrong, where it is (file and line), which standard it violates, and how to fix it. Informal findings left in prose, comments, or chat that skip issue creation are invisible debt — they cannot be tracked, assigned, or verified closed.

### Examples

**Good:** File a structured issue with location, violated standard, and fix.

```text
Title: RUST/unwrap in session.rs:42 — bare unwrap on Option
Body: session.rs:42 calls .unwrap() with no invariant documented. Violates RUST.md § Error handling. Fix: replace with .expect("invariant: session exists after auth").
```

**Bad:** Leave an audit finding as a prose note with no tracked issue.

```text
// audit note: unwraps in session.rs — not filed as an issue, invisible to tracking
```

## `QA/verify-dont-trust` {#qaverify-dont-trust}

- Severity: `warning`
- Scope: `universal`
- See also: `VERIFICATION/self-grade-prohibited`

Auditors must verify claims against the code, not trust documentation or PR descriptions. If a doc says the system does X, check the code. If an issue is closed, verify the fix exists at the referenced location. If the linter reports clean, check whether it is running all rules.

### Examples

**Good:** Check that the fix is present in the code before closing the issue.

```text
Verified: session.rs:42 now uses .expect("..."). Issue closed.
```

**Bad:** Close an issue because the PR description says it was fixed.

```text
Closed: PR #123 says this is fixed.
```

