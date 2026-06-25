# Systemd Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [SYSTEMD/restart-policy-required](#systemdrestart-policy-required)

## `SYSTEMD/restart-policy-required` {#systemdrestart-policy-required}

- Severity: `warning`
- Scope: `project:ops`

Every long-running fleet service unit must declare Restart=on-failure and a RestartSec delay. A unit without a restart policy leaves the service down after any crash, requiring manual intervention. RestartSec prevents tight restart loops that burn CPU while the underlying issue is still present.

### Examples

**Good:** Set Restart and RestartSec so the service recovers from transient failures.

```text
[Service]
Restart=on-failure
RestartSec=5s
```

**Bad:** Ship a unit file with no Restart directive so a crash leaves the service down.

```text
[Service]
ExecStart=/usr/bin/myapp
# no Restart directive
```

