# Context Preamble Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [CONTEXT/conflicting-rules-without-resolution](#contextconflicting-rules-without-resolution)
- [CONTEXT/preamble-required](#contextpreamble-required)

## `CONTEXT/conflicting-rules-without-resolution` {#contextconflicting-rules-without-resolution}

- Severity: `warning`
- Scope: `universal`
- Enforcer: `CONTEXT/conflicting-rules-without-resolution`
- See also: `CONTEXT/preamble-required`

Child context preambles must not declare merge policies that conflict with a parent unless one side uses explicit resolution. Align policies or declare an explicit merge resolution so agents can determine which instructions win.

### Examples

**Good:** Use explicit merge resolution when a child context needs a different policy.

```text
<!--
scope: crate
defers_to: ../CLAUDE.md
tightens: local crate rules
merge: explicit
merge-resolution: child rules override parent test guidance
-->
# Agent Context
```

**Bad:** Declare a merge policy that can conflict with the parent without explaining resolution.

```text
<!--
scope: crate
defers_to: ../CLAUDE.md
tightens: local crate rules
merge: last
-->
# Agent Context
```

## `CONTEXT/preamble-required` {#contextpreamble-required}

- Severity: `warning`
- Scope: `universal`
- Enforcer: `CONTEXT/preamble-required`

Agent-context files must start with a machine-readable preamble that declares scope, defers_to, and tightens. The preamble makes hierarchy and override behavior explicit before an agent consumes the file.

### Examples

**Good:** Start an agent-context file with the required preamble keys.

```text
<!--
scope: repo
defers_to: ../CLAUDE.md
tightens: local conventions
-->
# Agent Context
```

**Bad:** Publish an agent-context file without machine-readable scope metadata.

```text
# Agent Context
Follow the local repository conventions.
```

