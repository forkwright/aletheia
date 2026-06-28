# TOML Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [TOML/cargo-section-order](#tomlcargo-section-order)
- [TOML/deep-inline-nesting](#tomldeep-inline-nesting)
- [TOML/missing-trailing-comma](#tomlmissing-trailing-comma)

## `TOML/cargo-section-order` {#tomlcargo-section-order}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `TOML/cargo-section-order`
- See also: `TOML/missing-trailing-comma`

Cargo.toml sections must follow the canonical order defined by the TOML standards. Consistent section ordering makes manifests predictable to scan and keeps dependency, lint, profile, and metadata blocks from drifting across crates.

### Examples

**Good:** Keep Cargo.toml sections in the canonical order.

```text
[package]
name = "basanos"

[dependencies]
regex = "1"
```

**Bad:** Place dependencies before the package section.

```text
[dependencies]
regex = "1"

[package]
name = "basanos"
```

## `TOML/deep-inline-nesting` {#tomldeep-inline-nesting}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `TOML/deep-inline-nesting`
- See also: `TOML/missing-trailing-comma`

TOML inline tables must not be nested three or more levels on a single line. Deep inline tables are hard to scan and should be expanded into section tables.

### Examples

**Good:** Expand nested structures into table sections.

```text
[tool.taplo.formatting]
reorder_keys = true
```

**Bad:** Hide three inline-table levels on one line.

```text
tool = { taplo = { formatting = { reorder_keys = true } } }
```

## `TOML/missing-trailing-comma` {#tomlmissing-trailing-comma}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `TOML/missing-trailing-comma`

Multi-line TOML arrays must keep trailing commas on value lines. Comma-terminated arrays produce cleaner diffs when appending entries and avoid formatter churn around closing brackets.

### Examples

**Good:** Keep multi-line TOML arrays comma-terminated.

```text
members = [
    "crates/basanos",
]
```

**Bad:** Leave the final multi-line array element without a trailing comma.

```text
members = [
    "crates/basanos"
]
```

