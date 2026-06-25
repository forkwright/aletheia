# Crate Shape Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CRATE/shape-declaration-required](#crateshape-declaration-required)

## `CRATE/shape-declaration-required` {#crateshape-declaration-required}

- Severity: `warning`
- Scope: `language:toml`
- See also: `ARCHITECTURE/crate-shape-mismatch`

Every crate in the fleet must have an entry in CRATE-SHAPE.toml declaring its intended shape before composition budgets apply. An undeclared crate falls back to default budgets, which may not match the crate's actual architectural intent. Declaration is also the trigger for the shape-mismatch lint.

### Examples

**Good:** Declare the crate's shape in CRATE-SHAPE.toml before adding composition rules.

```text
# CRATE-SHAPE.toml
[crate.basanos]
shape = "engine"
repo = "forkwright/kanon"
notes = "lint engine and standards library"
```

**Bad:** Apply crate-shape composition budgets to a crate not listed in CRATE-SHAPE.toml.

```text
# CRATE-SHAPE.toml has no entry for crate `mylib`
# basanos applies default shape budgets; shape intent is undeclared
```

