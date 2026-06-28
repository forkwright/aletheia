# Gnomon Naming Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [GNOMON/essential-nature-required](#gnomonessential-nature-required)

## `GNOMON/essential-nature-required` {#gnomonessential-nature-required}

- Severity: `warning`
- Scope: `universal`
- See also: `COHERENCE/dimensional-coherence`, `NAMING/reveals-intent`

Module directories, agent identities, subsystems, and major features follow the gnomon naming convention. Names must identify the essential nature of the thing, not its implementation. The four-altitude test validates the name: L1 literal, L2 structural, L3 philosophical, L4 reflexive. A name that passes only L1 is a label; a name that passes all four is a gnomon.

### Examples

**Good:** Name a module after its essential nature, validated by all four altitude tests.

```text
// mnemosyne: goddess of memory — the module surfaces latent knowledge
// L1: stores and retrieves; L2: clean edges; L3: sovereignty; L4: exhibits memory by making knowledge retrievable
```

**Bad:** Name a module after its implementation detail rather than its essential nature.

```text
// VectorSearchEngine — tells callers the implementation, not the nature
// fails L3 (no philosophical grounding) and L4 (a search engine that finds nothing is still named "VectorSearchEngine")
```

