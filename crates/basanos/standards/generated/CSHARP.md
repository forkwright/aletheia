# C# Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CSHARP/nullable-enabled](#csharpnullable-enabled)

## `CSHARP/nullable-enabled` {#csharpnullable-enabled}

- Severity: `warning`
- Scope: `language:csharp`

All fleet C# projects must enable nullable reference types (<Nullable>enable</Nullable>). With nullable enabled the compiler tracks null flow and flags potential NullReferenceException sites at compile time. Disabling it forfeits static null-safety and forces runtime discovery of nullability bugs.

### Examples

**Good:** Enable nullable reference types in the project file and annotate accordingly.

```text
<!-- .csproj -->
<Nullable>enable</Nullable>
// C#
string? maybeNull = GetValue();
if (maybeNull is not null) Use(maybeNull);
```

**Bad:** Disable nullable reference types and use null-check patterns inconsistently.

```text
<!-- .csproj -->
<Nullable>disable</Nullable>
// compiler gives no guidance on null flow
```

