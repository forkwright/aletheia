# Datalog Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DATALOG/raw-mutation](#datalograw-mutation)

## `DATALOG/raw-mutation` {#datalograw-mutation}

- Severity: `warning`
- Scope: `language:datalog`
- Enforcer: `DATALOG/raw-mutation`

Datalog queries must not use raw mutation operators. Route data changes through the transaction API so mutation boundaries remain explicit and storage invariants stay enforceable.

### Examples

**Good:** Route Datalog writes through the transaction API.

```text
tx.insert(entity, attr, value)?;
```

**Bad:** Mutate a Datalog relation directly from a query.

```text
let query = "?[x <- 'value']";
```

