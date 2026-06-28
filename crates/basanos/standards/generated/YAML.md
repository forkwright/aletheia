# YAML Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [YAML/no-bare-boolean-strings](#yamlno-bare-boolean-strings)

## `YAML/no-bare-boolean-strings` {#yamlno-bare-boolean-strings}

- Severity: `warning`
- Scope: `language:yaml`

YAML values that are intended as strings but look like booleans (YES, NO, ON, OFF, true, false) must be quoted. YAML 1.1 (used by PyYAML and many CI systems) silently coerces these to boolean true/false, which causes unexpected behavior in pipelines that expect string values.

### Examples

**Good:** Quote boolean-like strings to avoid YAML 1.1 implicit coercion.

```text
enabled: true        # actual boolean
label: "true"        # string, not coerced
country_code: "NO"   # Norway, not false
```

**Bad:** Use unquoted YES/NO/ON/OFF which YAML 1.1 silently coerces to booleans.

```text
country: NO   # parsed as false in YAML 1.1
feature: YES  # parsed as true
```

