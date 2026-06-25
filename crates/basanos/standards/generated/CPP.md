# C++ Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CPP/prefer-smart-pointers](#cppprefer-smart-pointers)

## `CPP/prefer-smart-pointers` {#cppprefer-smart-pointers}

- Severity: `warning`
- Scope: `language:cpp`

Fleet C++ code must use RAII smart pointers (std::unique_ptr, std::shared_ptr) for heap allocations rather than raw owning pointers. Raw new/delete pairs are error-prone under exceptions and early returns. smart pointers make ownership explicit and tie resource lifetime to scope.

### Examples

**Good:** Use std::unique_ptr for exclusive ownership and std::shared_ptr only when shared.

```text
auto engine = std::make_unique<LintEngine>();
// shared only when multiple owners are needed:
auto shared = std::make_shared<Registry>();
```

**Bad:** Use raw owning pointers and call delete manually.

```text
LintEngine* engine = new LintEngine();
// ... eventually:
delete engine; // forgotten in error paths
```

