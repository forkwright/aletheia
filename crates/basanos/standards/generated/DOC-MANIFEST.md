# Documentation Manifest Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DOCS/stale-local-link](#docsstale-local-link)

## `DOCS/stale-local-link` {#docsstale-local-link}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `DOCS/stale-local-link`

Markdown documentation must not contain repo-local links that resolve nowhere. Broken links turn docs into stale promises and hide renamed, moved, or deleted files from review.

### Examples

**Good:** Point a local Markdown link at a file that exists in the repo.

```text
[Architecture](ARCHITECTURE.md)
```

**Bad:** Leave a local Markdown link pointing at a missing file.

```text
[Architecture](docs/missing-architecture.md)
```

