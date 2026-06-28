# Protocol Buffers Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PROTOBUF/reserve-removed-fields](#protobufreserve-removed-fields)

## `PROTOBUF/reserve-removed-fields` {#protobufreserve-removed-fields}

- Severity: `error`
- Scope: `language:protobuf`

Removed protobuf field numbers and names must be added to a reserved statement. Reusing a field number from a removed field causes silent data corruption: old serialized messages have the old type in that slot, and new deserializers interpret the bytes as the new type with no error. The reserved statement prevents accidental reuse.

### Examples

**Good:** Mark deprecated fields with the [deprecated=true] option and a comment.

```text
// Deprecated: use display_name instead.
string name = 1 [deprecated=true];
string display_name = 5;
```

**Bad:** Remove a field number from the schema without reserving it, allowing future reuse.

```text
// field 1 was removed — number is now available
string display_name = 5;  // breaks old clients silently
```

