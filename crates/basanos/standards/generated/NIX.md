# Nix Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [NIX/flake-lock-committed](#nixflake-lock-committed)

## `NIX/flake-lock-committed` {#nixflake-lock-committed}

- Severity: `warning`
- Scope: `language:nix`

flake.lock must be committed alongside flake.nix. The lock file pins all flake inputs to exact revisions, making builds reproducible across machines and time. Adding flake.lock to .gitignore means every build resolves inputs from the network, and two builds of the same commit may produce different results.

### Examples

**Good:** Commit flake.lock alongside flake.nix so builds are reproducible.

```text
# committed to the repo root:
flake.nix
flake.lock   # pinned inputs
```

**Bad:** Add flake.lock to .gitignore so every build resolves inputs at build time.

```text
# .gitignore
flake.lock  # inputs float; "nix build" today may differ from yesterday
```

