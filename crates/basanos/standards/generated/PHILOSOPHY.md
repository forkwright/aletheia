# Philosophy Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PHILOSOPHY/fail-fast-fail-loud](#philosophyfail-fast-fail-loud)
- [PHILOSOPHY/parse-dont-validate](#philosophyparse-dont-validate)

## `PHILOSOPHY/fail-fast-fail-loud` {#philosophyfail-fast-fail-loud}

- Severity: `warning`
- Scope: `universal`
- See also: `STANDARDS/aggregate-status-without-detail`, `PHILOSOPHY/parse-dont-validate`

Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values, silent degradation, and swallowed errors are bugs — they defer the failure to a context where it is harder to diagnose and fix. Surface errors at the point of origin with full context, and panic when the program cannot possibly proceed correctly.

### Examples

**Good:** Panic on an invariant violation with a message describing the broken invariant.

```text
let session = sessions.get(id).expect("session must exist after authentication");
```

**Bad:** Return a silent fallback when an invariant is violated.

```text
let session = sessions.get(id).unwrap_or_default();
```

## `PHILOSOPHY/parse-dont-validate` {#philosophyparse-dont-validate}

- Severity: `warning`
- Scope: `universal`
- See also: `TOPOLOGY/shallow-module`

Invalid data cannot exist past the point of construction. Newtypes, validation constructors, and type-level guarantees enforce invariants at the boundary: HTTP handlers, config loading, deserialization, CLI argument parsing. Once a value is constructed, its validity is a compile-time or construction-time guarantee, not a runtime check scattered through business logic.

### Examples

**Good:** Use a newtype constructor that rejects invalid values at construction time.

```text
struct Email(String);
impl Email {
    fn parse(raw: &str) -> Result<Self, EmailError> {
        validate_email_format(raw)?;
        Ok(Self(raw.to_owned()))
    }
}
```

**Bad:** Accept a raw string and validate it deep inside business logic.

```text
fn send_confirmation(email: String) {
    if !is_valid_email(&email) { return; } // too late
    ...
}
```

