# Kotlin Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [KOTLIN/structured-concurrency](#kotlinstructured-concurrency)

## `KOTLIN/structured-concurrency` {#kotlinstructured-concurrency}

- Severity: `warning`
- Scope: `language:kotlin`

Coroutines in fleet Kotlin code must launch from a structured scope tied to the component lifecycle (viewModelScope, lifecycleScope, or a CoroutineScope with a SupervisorJob scoped to the service). GlobalScope launches escape cancellation and leak resources when the owning component is destroyed.

### Examples

**Good:** Launch coroutines from a structured scope tied to component lifecycle.

```text
class MyViewModel : ViewModel() {
    fun load() {
        viewModelScope.launch { /* ... */ }
    }
}
```

**Bad:** Use GlobalScope for coroutines in a lifecycle-aware component.

```text
GlobalScope.launch { /* lives forever, leaks resources if the component is destroyed */
    loadData()
}
```

