# Prompt Templates

Role-scoped prompt skeletons for dispatch tasks. Each template is a starting
point, not a fill-in-the-blank form. Adapt to the specific task.

## Templates

| File | Role | When to use |
|------|------|-------------|
| `coder.md` | Feature implementation, bug fixes | Standard development work |
| `reviewer.md` | QA sub-agent, code review | After a coder agent produces a PR |
| `researcher.md` | Investigation, architecture analysis | Before implementation, for complex unknowns |
| `refactor.md` | Structural improvements, cross-crate moves | Planned refactors with known scope |
| `infra.md` | CI, tooling, deployment, standards | Non-product work that touches build or ops |

## How to use

1. Copy the relevant template.
2. Fill in the `[BRACKETED]` fields — these are required.
3. Fill in the optional fields (marked with `# optional`) if you have the
   information. Leave them out otherwise; the worker will discover them.
4. Place the filled prompt in `workflow/prompts/<project>/` with a numeric
   prefix (`01-`, `02-`, ...) that reflects the DAG execution order.

## YAML frontmatter

Each dispatch prompt requires YAML frontmatter:

```yaml
---
number: 1
description: "Short title for dashboards and QA reports"
depends_on: []          # prompt numbers this prompt depends on
model_tier: sonnet      # sonnet | opus | haiku
blast_radius:           # list of files/directories in scope
  - crates/my-crate/src/
acceptance_criteria:    # must be machine-verifiable
  - "cargo test -p my-crate passes"
  - "cargo clippy --workspace -- -D warnings clean"
---
```

Frontmatter is parsed by `crates/energeia/src/prompt.rs`.

## Relationship to PROMPTING.md

`standards/PROMPTING.md` covers API-level prompt construction: XML tags,
voice, caching, structured output. These templates apply those principles
to specific roles. The two documents are additive.
