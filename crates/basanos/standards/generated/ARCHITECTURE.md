# Architecture Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [ARCH/cross-repo-path-dep](#archcross-repo-path-dep)
- [ARCH/shared-infra-unregistered-cross-repo-dep](#archshared-infra-unregistered-cross-repo-dep)
- [ARCHITECTURE/crate-index-conformance](#architecturecrate-index-conformance)
- [ARCHITECTURE/crate-shape-mismatch](#architecturecrate-shape-mismatch)
- [ARCHITECTURE/no-architecture-doc](#architectureno-architecture-doc)
- [ARCHITECTURE/no-deny-missing-docs](#architectureno-deny-missing-docs)
- [ARCHITECTURE/no-glossary](#architectureno-glossary)
- [ARCHITECTURE/thick-binary](#architecturethick-binary)
- [ARCHITECTURE/trait-impl-colocation](#architecturetrait-impl-colocation)
- [MANIFEST/archeion-single-git-backend](#manifestarcheion-single-git-backend)

## `ARCH/cross-repo-path-dep` {#archcross-repo-path-dep}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/cross-repo-path-dep`
- See also: `ARCH/shared-infra-unregistered-cross-repo-dep`

Registry entries that use `mechanism = "operator-local-path"` must carry a typed `[[substrate.exception]]` block with `kind = "operator-local"`. Cross-repo path dependencies are local-checkout exceptions, not ordinary shared-infrastructure contracts.

### Examples

**Good:** Attach a typed exception to an operator-local path substrate.

```text
[[substrate]]
id = "logismos"
mechanism = "operator-local-path"

[[substrate.exception]]
kind = "operator-local"
reason = "private checkout until git pin is practical"
review_trigger = "private forge ref becomes available"
```

**Bad:** Declare an operator-local path mechanism without the typed exception block.

```text
[[substrate]]
id = "logismos"
mechanism = "operator-local-path"
```

## `ARCH/shared-infra-unregistered-cross-repo-dep` {#archshared-infra-unregistered-cross-repo-dep}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCH/shared-infra-unregistered-cross-repo-dep`
- See also: `ARCH/cross-repo-path-dep`

Cargo path dependencies that leave the workspace are shared-infrastructure edges and must be registered in `crates/basanos/standards/substrate.toml`. The registration records producer, consumers, ownership, mechanism, and the operator-local exception when a private sibling checkout is unavoidable.

### Examples

**Good:** Register a sibling checkout path dependency in the substrate registry.

```text
Cargo.toml
logismos = { path = "../logismos/crates/logismos" }

substrate.toml
[[substrate]]
id = "logismos"
mechanism = "operator-local-path"
```

**Bad:** Point Cargo at a sibling repo without a substrate registry entry.

```text
Cargo.toml
logismos = { path = "../logismos/crates/logismos" }
```

## `ARCHITECTURE/crate-index-conformance` {#architecturecrate-index-conformance}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCHITECTURE/crate-index-conformance`

`CRATE-INDEX.toml` must match the actual workspace crate dependency graph. Architecture indexes are only useful when declared paths, layers, dependency edges, and reverse consumers stay derived from the real crates.

### Examples

**Good:** Keep declared crate edges mirrored in both dependency directions.

```text
[crates.api]
path = "crates/api"
depends_on = ["core"]

[crates.core]
path = "crates/core"
used_by = ["api"]
```

**Bad:** Let the architecture index drift away from Cargo path dependencies.

```text
[crates.api]
path = "crates/api"
depends_on = []

# Cargo.toml still depends on ../core
```

## `ARCHITECTURE/crate-shape-mismatch` {#architecturecrate-shape-mismatch}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `ARCHITECTURE/crate-shape-mismatch`
- See also: `ARCHITECTURE/crate-index-conformance`

`CRATE-SHAPE.toml` must keep crate entries, shape budgets, and workspace coverage aligned with the canonical crate-shape contract. A drifted shape registry makes architecture composition checks judge crates from stale or incomplete boundaries.

### Examples

**Good:** Keep crate-shape entries complete and aligned with canonical shape budgets.

```text
CRATE-SHAPE.toml
[crate.basanos]
shape = "core"
repo = "kanon"
notes = "standards linter"

[shape.core]
max_file_lines = 400
max_pub_ratio = 0.35
```

**Bad:** Reference an unknown shape and omit the crate registry fields readers need.

```text
CRATE-SHAPE.toml
[crate.basanos]
shape = "misc"
```

## `ARCHITECTURE/no-architecture-doc` {#architectureno-architecture-doc}

- Severity: `info`
- Scope: `universal`
- Enforcer: `ARCHITECTURE/no-architecture-doc`
- See also: `ARCHITECTURE/thick-binary`

Workspace repositories should carry an `ARCHITECTURE.md` or `docs/ARCHITECTURE.md` overview. The document gives contributors a stable map of system boundaries, data flow, and major design constraints.

### Examples

**Good:** Keep a workspace-level architecture overview.

```text
ARCHITECTURE.md
# Architecture
System boundaries and data flow live here.
```

**Bad:** Declare a workspace without an architecture document.

```text
Cargo.toml
[workspace]
members = ["crates/*"]
```

## `ARCHITECTURE/no-deny-missing-docs` {#architectureno-deny-missing-docs}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `ARCHITECTURE/no-deny-missing-docs`

Library crate roots must deny missing public API documentation. `#![deny(missing_docs)]` keeps exported items documented instead of letting public surface area grow silently.

### Examples

**Good:** Deny missing public API documentation at the crate root.

```text
#![deny(missing_docs)]
//! Library API for dispatch orchestration.
```

**Bad:** Expose a library crate without making missing docs fail.

```text
pub fn run() -> Result<()> {
    Ok(())
}
```

## `ARCHITECTURE/no-glossary` {#architectureno-glossary}

- Severity: `info`
- Scope: `universal`
- Enforcer: `ARCHITECTURE/no-glossary`
- See also: `ARCHITECTURE/no-architecture-doc`

Workspaces with domain-specific terminology should maintain a glossary or lexicon. Shared vocabulary keeps architecture discussions precise and prevents contributors from reverse-engineering local terms from usage.

### Examples

**Good:** Define project terminology in a glossary or lexicon.

```text
GLOSSARY.md
# Glossary
Shard: A bounded partition of tenant data.
```

**Bad:** Use domain-specific terms without a shared reference.

```text
Cargo.toml
[workspace]
members = ["crates/*"]
```

## `ARCHITECTURE/thick-binary` {#architecturethick-binary}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `ARCHITECTURE/thick-binary`
- See also: `TOPOLOGY/shallow-module`

Binary entrypoints stay thin. `main.rs` wires CLI, configuration, and top-level orchestration; reusable behavior belongs in library modules where it can be tested directly.

### Examples

**Good:** Keep the binary entrypoint thin and delegate to library code.

```text
fn main() -> Result<()> {
    cli::run()
}
```

**Bad:** Put orchestration, parsing, and business logic in main.rs.

```text
fn main() {
    parse_config();
    migrate_database();
    start_workers();
}
```

## `ARCHITECTURE/trait-impl-colocation` {#architecturetrait-impl-colocation}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `ARCHITECTURE/trait-impl-colocation`
- See also: `ARCHITECTURE/thick-binary`

Public traits should not be defined and implemented for concrete types in the same library file. Move shared traits toward the consumer boundary so implementors do not freeze architecture decisions beside the abstraction.

### Examples

**Good:** Keep a shared trait separate from concrete implementors.

```text
pub trait Repository {
    fn load(&self) -> Result<Item>;
}

// concrete impl lives in the consumer or adapter crate
```

**Bad:** Define a public trait and concrete implementation in the same library file.

```text
pub trait Repository {
    fn load(&self) -> Result<Item>;
}

impl Repository for SqlRepository {
    fn load(&self) -> Result<Item> { todo!() }
}
```

## `MANIFEST/archeion-single-git-backend` {#manifestarcheion-single-git-backend}

- Severity: `error`
- Scope: `language:toml`
- Enforcer: `MANIFEST/archeion-single-git-backend`

Archeion owns kanon's git implementation and must use a single git backend. Adding a direct `gix`, `git2`, `libgit2`, or `libgit2-sys` dependency doubles the backend safety and test surface.

### Examples

**Good:** Extend archeion's existing git implementation without adding another backend crate.

```text
crates/archeion/Cargo.toml
[dependencies]
thiserror = "2"
```

**Bad:** Add a second git backend dependency to archeion.

```text
crates/archeion/Cargo.toml
[dependencies]
git2 = "0.20"
```

