# Sysctl Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [SYSCTL/numeric-prefix](#sysctlnumeric-prefix)

## `SYSCTL/numeric-prefix` {#sysctlnumeric-prefix}

- Severity: `warning`
- Scope: `project:linux-sysctl`
- Enforcer: `SYSCTL/numeric-prefix`

Files in `/etc/sysctl.d/` must use the `NN-name.conf` naming convention. The numeric prefix makes load order explicit so later drop-ins can intentionally override earlier kernel parameter settings.

### Examples

**Good:** Name sysctl.d drop-ins with an explicit numeric load order.

```text
/etc/sysctl.d/50-security-hardening.conf
```

**Bad:** Use a sysctl.d filename whose load order is undefined.

```text
/etc/sysctl.d/security-hardening.conf
```

