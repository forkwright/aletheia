# Planning

> Standards for project planning, phase management, and decision tracking.

---

## File formats

### PLAN.md (per phase)

```markdown
# Phase NN: {Name}

## Goal
{What must be TRUE when this phase completes}

## Success criteria
{Observable outcomes, not tasks}

## Falsification

For each success criterion, what observation would prove it wrong?

| Criterion | Falsifier |
|-----------|-----------|
| {Criterion text} | {Observation that would falsify it} |
| ... | ... |

## Scope
### In scope
- {Specific deliverable}

### Out of scope
- {Thing explicitly excluded}

## Requirements
- {REQ-01}: {Specific, testable requirement}

## Decisions
| Decision | Choice | Rationale |
|----------|--------|-----------|

## Open questions
- {Question}: {context}

## Dependencies
- {Dependency}: {status}
```

Rules:
- Every success criterion must have a corresponding falsifier.
- Falsifiers must be observable ("benchmark shows X" not "it feels slow").
- If a criterion cannot be falsified, rewrite it or document it as aspirational.
