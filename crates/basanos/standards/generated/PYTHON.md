# Python Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PYTHON/type-annotations-required](#pythontype-annotations-required)

## `PYTHON/type-annotations-required` {#pythontype-annotations-required}

- Severity: `warning`
- Scope: `language:python`

All public Python functions and methods in fleet code must carry PEP 484 type annotations on parameters and return types. Unannotated signatures force callers to read the implementation to understand the contract and disable mypy's ability to catch type mismatches statically.

### Examples

**Good:** Annotate all public function signatures with PEP 484 type hints.

```text
def parse_rule(text: str) -> Rule:
    ...

def rules_for_doc(doc: StandardsDoc) -> list[Rule]:
    ...
```

**Bad:** Ship a public function with no type annotations, forcing callers to read the body.

```text
def parse_rule(text):
    ...
```

