# Substrate Registry Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [ARCH/substrate-coherence-floor](#archsubstrate-coherence-floor)
- [ARCH/substrate-dead-dep](#archsubstrate-dead-dep)
- [ARCH/substrate-dual-mechanism](#archsubstrate-dual-mechanism)
- [ARCH/substrate-feature-conflict](#archsubstrate-feature-conflict)
- [ARCH/substrate-floating-ref](#archsubstrate-floating-ref)
- [ARCH/substrate-mirror-drift](#archsubstrate-mirror-drift)
- [ARCH/substrate-pin-behind](#archsubstrate-pin-behind)
- [ARCH/substrate-pin-drift](#archsubstrate-pin-drift)
- [ARCH/substrate-unregistered](#archsubstrate-unregistered)
- [ARCH/substrate-visibility-buildable](#archsubstrate-visibility-buildable)
- [SUBSTRATE/registration-required](#substrateregistration-required)

## `ARCH/substrate-coherence-floor` {#archsubstrate-coherence-floor}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-coherence-floor`

A consumer whose `rust-version` is below the substrate's `boundary_floors.msrv` cannot compile the substrate. Raise the consumer MSRV to at least the floor declared by the producer.

## `ARCH/substrate-dead-dep` {#archsubstrate-dead-dep}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-dead-dep`

A workspace-level `git =` dependency that is never referenced via `workspace = true` in any member crate is dead weight. Remove it from `[workspace.dependencies]` or add the missing `workspace = true` reference.

## `ARCH/substrate-dual-mechanism` {#archsubstrate-dual-mechanism}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-dual-mechanism`
- See also: `ARCH/substrate-pin-behind`

Each substrate registry entry must use exactly one mechanism. A `git-tag` entry that also carries `pinned_sha` is ambiguous and cannot be synced deterministically by the generator. Move the pin to a `[substrate.pin]` block.

## `ARCH/substrate-feature-conflict` {#archsubstrate-feature-conflict}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-feature-conflict`

Enabling both `native-tls` and `rustls`-family features on reqwest causes duplicate-provider link failures. The canonical fleet TLS provider is `rustls-no-provider`; use only one TLS backend.

## `ARCH/substrate-floating-ref` {#archsubstrate-floating-ref}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-floating-ref`
- See also: `ARCH/substrate-pin-drift`

Git dependencies must pin to a `rev =` (SHA) or `tag =`; floating `branch =` refs or bare git deps resolve to HEAD at fetch time and cannot be reproduced across machines.

## `ARCH/substrate-mirror-drift` {#archsubstrate-mirror-drift}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-mirror-drift`
- See also: `ARCH/substrate-pin-drift`

A `consumers_detail` entry with `mirror_of` must have its source consumer provide a `manifest_path`. Without a resolvable source manifest, the mirror cannot be verified or synced.

## `ARCH/substrate-pin-behind` {#archsubstrate-pin-behind}

- Severity: `info`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-pin-behind`
- See also: `ARCH/substrate-dual-mechanism`

A `registered-shared-crate` still using `mechanism = "git-sha-pin"` is a sign that the sha→tag promotion has not completed. Promote to `mechanism = "git-tag"` and add a `[substrate.pin]` block to enable deterministic sync.

## `ARCH/substrate-pin-drift` {#archsubstrate-pin-drift}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-pin-drift`
- See also: `ARCH/substrate-floating-ref`

Fenced substrate regions in consumer manifests must match the canonical pin recorded in `[substrate.pin]`. Drift indicates the manifest was hand-edited or the pin was bumped without running `kanon substrate sync --apply`.

## `ARCH/substrate-unregistered` {#archsubstrate-unregistered}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-unregistered`
- See also: `ARCH/shared-infra-unregistered-cross-repo-dep`

All cross-repo `git =` dependencies must have a corresponding `[[substrate]]` entry in `crates/basanos/standards/substrate.toml`. Unregistered git deps are invisible to fleet tooling and cannot be synced.

## `ARCH/substrate-visibility-buildable` {#archsubstrate-visibility-buildable}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/substrate-visibility-buildable`
- See also: `ARCH/substrate-visibility-mismatch`

A substrate with `visibility = "private"` consumed by a cross-org consumer violates the public-build invariant: the consumer's CI cannot clone the private producer. Either make the producer public or add an `operator-local` exception.

## `SUBSTRATE/registration-required` {#substrateregistration-required}

- Severity: `warning`
- Scope: `universal`
- See also: `ARCH/shared-infra-unregistered-cross-repo-dep`

Cargo path dependencies that leave the workspace are shared-infrastructure edges and must be registered in substrate.toml with their producer, consumers, mechanism, and ownership. Unregistered cross-repo path dependencies are invisible to the fleet tooling and cannot be audited, migrated, or replaced systematically.

### Examples

**Good:** Register a cross-repo path dependency in substrate.toml before using it.

```text
# substrate.toml
[[substrate]]
id = "logismos"
mechanism = "operator-local-path"
producer = "forkwright/logismos"
consumers = ["forkwright/kanon"]
```

**Bad:** Add a cross-repo path dependency to Cargo.toml with no substrate registry entry.

```text
# Cargo.toml (no substrate.toml entry)
[dependencies]
logismos = { path = "../logismos/crates/logismos" }
```

