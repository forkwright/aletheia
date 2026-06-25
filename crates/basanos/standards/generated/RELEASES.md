# Releases Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [RELEASES/semver-required](#releasessemver-required)

## `RELEASES/semver-required` {#releasessemver-required}

- Severity: `warning`
- Scope: `universal`
- See also: `SUPPLY-CHAIN/build-attestation-required`

All shipped software must follow Semantic Versioning. Breaking changes bump major, additive non-breaking changes bump minor, bug fixes bump patch. Pre-1.0 crates may treat any breaking change as a minor bump, but must document this in the crate's CHANGELOG. Consumers pin pre-1.0 dependencies to exact versions.

### Examples

**Good:** Bump major on breaking changes, minor on additive changes, patch on fixes.

```text
# breaking API change: 0.8.1 → 1.0.0
# new non-breaking feature: 1.0.0 → 1.1.0
# bug fix: 1.1.0 → 1.1.1
```

**Bad:** Ship a breaking API change as a patch bump.

```text
# removed a public function: 0.8.1 → 0.8.2
```

