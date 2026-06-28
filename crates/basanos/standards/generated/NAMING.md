# Naming Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [NAMING/no-fleet-collision](#namingno-fleet-collision)
- [NAMING/no-owner-prefix](#namingno-owner-prefix)
- [NAMING/reveals-intent](#namingreveals-intent)
- [NAMING/struct-name-vague-hub](#namingstruct-name-vague-hub)
- [VOCAB/hub-word-drift](#vocabhub-word-drift)

## `NAMING/no-fleet-collision` {#namingno-fleet-collision}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `NAMING/no-fleet-collision`
- See also: `NAMING/reveals-intent`

Fleet repository names are reserved nouns. Workspace-local crates and public API type names must not reuse a reserved repo noun outside the repo that owns it.

### Examples

**Good:** Use a repo-local noun that cannot collide with a fleet repository.

```text
package.name = "aggelmata"
```

**Bad:** Reuse a reserved fleet repository name in a different repo.

```text
package.name = "zetesis"
```

## `NAMING/no-owner-prefix` {#namingno-owner-prefix}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `NAMING/no-owner-prefix`
- See also: `NAMING/no-fleet-collision`, `NAMING/reveals-intent`

Crate names must not bolt a role onto a fleet repository or owner slug. The resulting parent-child shape makes ownership ambiguous and bypasses GNOMON's standalone naming review.

### Examples

**Good:** Use a standalone crate coinage that names what the crate is.

```text
package.name = "nous-classify"
```

**Bad:** Prefix a fleet repository name onto a crate role.

```text
package.name = "aletheia-classify"
```

## `NAMING/reveals-intent` {#namingreveals-intent}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `NAMING/reveals-intent`

Local names reveal the role of the value in the surrounding operation. Placeholder bindings such as `tmp`, `data`, or `val` are allowed only with a nearby rationale or a domain where the short name is conventional.

### Examples

**Good:** Name the role of the value.

```text
let selected_prompt = queue.next_ready();
```

**Bad:** Use a placeholder name that hides purpose.

```text
let tmp = queue.next_ready();
```

## `NAMING/struct-name-vague-hub` {#namingstruct-name-vague-hub}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `NAMING/struct-name-vague-hub`
- See also: `NAMING/reveals-intent`

Public Rust structs must not repeat the enclosing crate concern and then add a generic hub-word suffix. The crate already names the concern; the struct name must reveal the narrower responsibility the type owns.

### Examples

**Good:** Use a suffix that names the concrete role.

```text
pub struct BasanosRuleCatalog;
```

**Bad:** Repeat the crate concern and add a generic coordination noun.

```text
pub struct BasanosManager;
```

## `VOCAB/hub-word-drift` {#vocabhub-word-drift}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `VOCAB/hub-word-drift`
- See also: `NAMING/reveals-intent`

Hub-word prose must use the canonical term from the shared vocabulary registry. Forbidden synonyms fragment meanings across modules, while the registry records the accepted term and any explicitly distinct concepts.

### Examples

**Good:** Use the canonical hub word from the shared registry.

```text
We persist memory by writing to the mneme facade.
```

**Bad:** Use a forbidden synonym near the hub word.

```text
We persist memory by offloading to storage.
```

