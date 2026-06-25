# Topology Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [RUST/option-bool-pair](#rustoption-bool-pair)
- [RUST/primitive-for-domain-id](#rustprimitive-for-domain-id)
- [TOPOLOGY/shallow-module](#topologyshallow-module)

## `RUST/option-bool-pair` {#rustoption-bool-pair}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/option-bool-pair`
- See also: `TOPOLOGY/shallow-module`

Structs must not use multiple `Option<bool>` fields to encode related state. The combined tri-state values create unclear or invalid combinations; use a sum type that names the valid states explicitly.

### Examples

**Good:** Model named states with an enum.

```text
enum ReviewState {
    Approved,
    Rejected,
    Pending,
}
```

**Bad:** Encode related states with multiple optional booleans.

```text
struct Review {
    approved: Option<bool>,
    rejected: Option<bool>,
}
```

## `RUST/primitive-for-domain-id` {#rustprimitive-for-domain-id}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/primitive-for-domain-id`

Domain identifiers carried as primitive strings or integers can be mixed, swapped, or constructed without validation. Public Rust fields whose names indicate IDs, branches, SHAs, or other domain identifiers should use newtypes with constructor boundaries.

### Examples

**Good:** Wrap a domain identifier in a newtype with a constructor boundary.

```text
pub struct RunId(String);
impl RunId {
    pub fn parse(raw: &str) -> Result<Self, RunIdError> { /* validate */ }
}
```

**Bad:** Expose a domain identifier as a bare primitive field.

```text
pub struct RunRecord {
    pub run_id: String,
}
```

## `TOPOLOGY/shallow-module` {#topologyshallow-module}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `TOPOLOGY/shallow-module`
- See also: `ARCHITECTURE/thick-binary`

Public modules should expose a deep interface: a small surface that carries meaningful behavior. Wide modules full of thin delegation functions are harder to understand, test, and evolve.

### Examples

**Good:** Expose a cohesive operation instead of many thin delegates.

```text
pub fn render_standards_views(registry: &[Rule]) -> RenderedStandards
```

**Bad:** Publish many forwarding functions that leak the module internals.

```text
pub fn render_rust() { render_doc(StandardsDoc::Rust) }
```

