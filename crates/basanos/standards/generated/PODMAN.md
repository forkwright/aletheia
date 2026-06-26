# Podman Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PODMAN/rootless-required](#podmanrootless-required)

## `PODMAN/rootless-required` {#podmanrootless-required}

- Severity: `error`
- Scope: `project:ops`

All fleet containers must run rootless: the container user must be a non-root UID, and the container must not be granted --privileged or SYS_ADMIN capabilities. Running as root inside a container reduces namespace isolation and widens the attack surface if the container runtime is compromised.

### Examples

**Good:** Run the container as a named non-root user with an explicit UID mapping.

```text
podman run --user=1000:1000 --userns=keep-id ghcr.io/forkwright/kanon:latest
```

**Bad:** Run the container as root with no user namespace override.

```text
podman run --privileged ghcr.io/forkwright/kanon:latest
```

