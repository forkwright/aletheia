# Change Discipline Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CHANGE/deletion-without-why](#changedeletion-without-why)
- [CHANGE/refactor-and-behavior-mixed](#changerefactor-and-behavior-mixed)

## `CHANGE/deletion-without-why` {#changedeletion-without-why}

- Severity: `warning`
- Scope: `universal`
- See also: `STANDARDS/removal-no-issue`

Deletion of load-bearing code or configuration must carry a WHY in the commit body or a removal marker citing a tracking issue. Code that disappears without explanation becomes an invisible gap: reviewers cannot tell whether the deletion was intentional or accidental, and the fix for a regression requires archaeology.

### Examples

**Good:** Record the decision that justifies deleting load-bearing code.

```text
// REMOVED(#438): legacy XML parser — JSON-only API since v0.8, no callers remain
fn parse_xml_response(...)
```

**Bad:** Delete a load-bearing function with no commit body explaining why.

```text
commit: chore: remove old code
-fn parse_xml_response(...)
```

## `CHANGE/refactor-and-behavior-mixed` {#changerefactor-and-behavior-mixed}

- Severity: `warning`
- Scope: `universal`
- See also: `CHANGE/deletion-without-why`

Structural refactors and behavior changes must land as separate commits. A diff that blends broad movement (renames, file splits, module reorganization) with behavior edits (error handling, logic changes, API mutations) cannot be reviewed for correctness — the signal is buried in noise. Structural changes first; behavior changes after.

### Examples

**Good:** Land the structural rename as one commit and the behavior change as a second commit.

```text
commit 1: refactor(session): rename SessionCache to SessionStore
commit 2: fix(session): return 404 when session not found instead of panicking
```

**Bad:** Mix a rename of 40 files with a behavior change to the error path in one commit.

```text
commit: refactor(session): rename + fix error path
  (40 files renamed, session.rs error handling changed)
```

