# Manifest Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [MANIFEST/description-honesty](#manifestdescription-honesty)
- [MANIFEST/lockfile-no-localhost-git](#manifestlockfile-no-localhost-git)
- [MANIFEST/maturity-description-mismatch](#manifestmaturity-description-mismatch)
- [MANIFEST/missing-maturity-metadata](#manifestmissing-maturity-metadata)

## `MANIFEST/description-honesty` {#manifestdescription-honesty}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/description-honesty`

Cargo package descriptions must describe the crate's actual purpose. Placeholder or scaffold text in package metadata is user-facing in registries and generated documentation, so it must be replaced before the crate is treated as publishable.

### Examples

**Good:** Describe the crate's concrete role in user-facing Cargo metadata.

```text
description = "Lint Cargo manifest package metadata for standards drift"
```

**Bad:** Publish placeholder text as the package description.

```text
description = "placeholder crate"
```

## `MANIFEST/lockfile-no-localhost-git` {#manifestlockfile-no-localhost-git}

- Severity: `error`
- Scope: `language:lock`
- Enforcer: `MANIFEST/lockfile-no-localhost-git`
- See also: `SECURITY/hardcoded-loopback-url`

Tracked Cargo.lock files must resolve git dependencies from the upstream repository URL declared in Cargo.toml, not from a loopback host such as `127.0.0.1` or `localhost`. A lockfile produced against a local forge cannot build on machines that do not run that service, and it hides the real revision that is being compiled.

### Examples

**Good:** Resolve git dependencies from the upstream repository declared in Cargo.toml.

```text
Cargo.lock
source = "git+https://github.com/forkwright/theatron.git?tag=v1.3.0#2f73ff19"
```

**Bad:** Pin a git dependency to a local forge loopback address.

```text
Cargo.lock
source = "git+http://127.0.0.1:7878/forkwright/theatron.git?rev=3c02dc4#3c02dc42"
```

## `MANIFEST/maturity-description-mismatch` {#manifestmaturity-description-mismatch}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/maturity-description-mismatch`
- See also: `MANIFEST/description-honesty`

Scaffold Cargo packages must not use production-tone package descriptions. Descriptions are user-facing metadata, so scaffold crates must omit `package.description` or prefix it with `(scaffold)` until the crate leaves scaffold maturity.

### Examples

**Good:** Mark scaffold package descriptions visibly as scaffold status.

```text
description = "(scaffold) storage adapter under active design"
```

**Bad:** Describe a scaffold package as though it were production-ready.

```text
description = "Reliable storage adapter for production workloads"
```

## `MANIFEST/missing-maturity-metadata` {#manifestmissing-maturity-metadata}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/missing-maturity-metadata`
- See also: `MANIFEST/maturity-description-mismatch`

Cargo package manifests must declare `package.metadata.kanon` maturity, since, and exit-criteria fields. Typed crate maturity metadata lets docs, audits, prompts, and gates derive scaffold-vs-production status from one Cargo source.

### Examples

**Good:** Declare crate maturity metadata in the package manifest.

```text
[package.metadata.kanon]
maturity = "alpha"
since = "2026-05-26"
exit-criteria = "public API is stable"
```

**Bad:** Publish a package manifest without Kanon maturity metadata.

```text
[package]
name = "kanon-core"
```

