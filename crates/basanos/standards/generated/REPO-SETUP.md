# Repository Setup Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [REPO/required-files](#reporequired-files)

## `REPO/required-files` {#reporequired-files}

- Severity: `warning`
- Scope: `universal`
- See also: `FLEET/tier-compliance-required`

Every forkwright project repository must include README.md, LICENSE, CLAUDE.md, and deny.toml at the root. These files are not optional: README is the entry point, LICENSE declares terms, CLAUDE.md gives agents the context they need to contribute correctly, and deny.toml enforces dependency policy. A fresh clone must succeed without these.

### Examples

**Good:** Include all required root files: README, LICENSE, CLAUDE.md, deny.toml.

```text
# repo root
README.md
LICENSE
CLAUDE.md
deny.toml
Cargo.toml
```

**Bad:** Ship a new repo without deny.toml or CLAUDE.md.

```text
# repo root
README.md
Cargo.toml
```

