# Performance Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PERFORMANCE/clone-in-loop](#performanceclone-in-loop)
- [PERFORMANCE/collect-then-iter](#performancecollect-then-iter)

## `PERFORMANCE/clone-in-loop` {#performanceclone-in-loop}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `PERFORMANCE/clone-in-loop`

Rust code should not clone values inside loops when borrowing, moving ownership, or restructuring can avoid repeated copies. Repeated clones in hot loops hide allocation and CPU cost behind an otherwise small expression.

### Examples

**Good:** Borrow values inside the loop instead of cloning each iteration.

```text
for item in items {
    process(item.as_ref());
}
```

**Bad:** Clone a value on every loop iteration.

```text
for item in items {
    process(item.clone());
}
```

## `PERFORMANCE/collect-then-iter` {#performancecollect-then-iter}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `PERFORMANCE/collect-then-iter`
- See also: `PERFORMANCE/clone-in-loop`

Rust code should not collect an iterator into a `Vec` only to immediately iterate it again. Keep the operations in one iterator chain so the code avoids an unnecessary allocation and preserves streaming behavior.

### Examples

**Good:** Keep the iterator chain lazy without an intermediate Vec.

```text
items.iter().map(normalize).for_each(send);
```

**Bad:** Collect into a Vec only to iterate the values immediately.

```text
items.iter().map(normalize).collect::<Vec<_>>().iter().for_each(send);
```

