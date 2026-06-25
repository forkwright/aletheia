# Meta-Artifact Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [META/frontmatter-required](#metafrontmatter-required)

## `META/frontmatter-required` {#metafrontmatter-required}

- Severity: `warning`
- Scope: `universal`
- See also: `CONTEXT/preamble-required`

Fleet meta-artifacts (planning docs, ADRs, standards, vision docs, runbooks) must open with a frontmatter block declaring `kind`, `status`, and `applies_to`. The frontmatter is the machine-readable declaration that the scanner uses to build the artifact catalog; without it the document is invisible to discovery and drift-detection tooling.

### Examples

**Good:** Open a planning document with the required frontmatter block.

```text
---
kind: plan
canonical: true
status: accepted
applies_to: [kanon]
---

# Phase 24 Plan
```

**Bad:** Write a planning document with no frontmatter.

```text
# Phase 24 Plan

This phase covers...
```

