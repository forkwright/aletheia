# Derivation Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DERIVATION/byte-stable-derived-artifacts](#derivationbyte-stable-derived-artifacts)
- [DERIVATION/committed-derived-artifacts](#derivationcommitted-derived-artifacts)
- [DERIVATION/content-derived-provenance](#derivationcontent-derived-provenance)
- [DERIVATION/regenerate-and-compare-freshness](#derivationregenerate-and-compare-freshness)

## `DERIVATION/byte-stable-derived-artifacts` {#derivationbyte-stable-derived-artifacts}

- Severity: `warning`
- Scope: `universal`
- See also: `DERIVATION/content-derived-provenance`, `DERIVATION/regenerate-and-compare-freshness`

Derived artifacts must be byte-stable: identical inputs and generator must produce identical bytes. Avoid embedded timestamps, random identifiers, and unordered collection iteration so regeneration does not create noisy diffs or break content-addressed caches.

### Examples

**Good:** Sort keys and pin the formatter so generated output is stable.

```text
# generate-bindings.sh
bindgen --no-layout-tests --sort-semantically wrapper.h > src/bindings.rs
```

**Bad:** Emit a timestamp into a generated file that is checked into version control.

```text
// Generated on 2026-06-22T12:00:00Z
```

## `DERIVATION/committed-derived-artifacts` {#derivationcommitted-derived-artifacts}

- Severity: `warning`
- Scope: `universal`
- See also: `DERIVATION/regenerate-and-compare-freshness`, `DERIVATION/content-derived-provenance`

Derived artifacts required to build, test, or operate the system must be committed to version control. Consumers must not need to run the generator themselves to obtain a working checkout.

### Examples

**Good:** Commit the generated file so a fresh clone builds without running the generator.

```text
src/bindings.rs  # checked in, with header pointing to wrapper.h + bindgen invocation
```

**Bad:** Exclude a required generated file from version control.

```text
src/bindings.rs  # listed in .gitignore; build fails on fresh clone
```

## `DERIVATION/content-derived-provenance` {#derivationcontent-derived-provenance}

- Severity: `warning`
- Scope: `universal`
- See also: `DERIVATION/byte-stable-derived-artifacts`, `DERIVATION/regenerate-and-compare-freshness`

Every derived artifact must carry provenance that names its source inputs and generator. Provenance must be reconstructible from the artifact and the repository, not from out-of-band knowledge.

### Examples

**Good:** Name the source inputs and generator in the generated file header.

```text
// Generated from deny.toml by `kanon audit derive-ignores --apply` (kanon 0.1.0)
```

**Bad:** Claim generation without saying what was generated from what.

```text
// Generated automatically.
```

## `DERIVATION/regenerate-and-compare-freshness` {#derivationregenerate-and-compare-freshness}

- Severity: `warning`
- Scope: `universal`
- See also: `DERIVATION/content-derived-provenance`, `DERIVATION/committed-derived-artifacts`

Every derived artifact must have a documented, deterministic regeneration command. The committed artifact must match the command's output; if it does not, it is stale.

### Examples

**Good:** Provide a checked-in script and a gate step that fails on diff.

```text
scripts/generate-schema.sh && git diff --exit-code schema.json
```

**Bad:** Update a derived file by hand and leave no regeneration command.

```text
// hand-edited to match the new API shape
```

