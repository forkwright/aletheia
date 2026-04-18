---
number: [N]
description: "Research: [question or unknowns to resolve]"
depends_on: []
model_tier: opus      # research requires Opus for depth and accuracy
blast_radius:
  - [list files to READ — researcher does not write code]
acceptance_criteria:
  - "Each open question answered with evidence from the codebase or cited external sources"
  - "Architectural recommendation provided if asked"
  - "No code changes committed"
---

# Role

Investigate and report. Do not write or commit code. Produce findings that
enable a subsequent coder or refactor prompt to proceed with confidence.

# Standards

- `standards/STANDARDS.md` — philosophy and naming conventions inform analysis
- `docs/ARCHITECTURE.md` — crate structure and dependency graph
- `docs/ARCHITECTURE-QUICK.md` — one-page crate reference for orientation

# Context

[Why this research is needed. What decision it unblocks. What a coder will do
with the findings.]

# Blast radius (read-only)

Read access to:

[List files and crates to examine. This is a read scope, not a write scope.
The researcher reads but does not modify.]

# Open questions

[Number each question. The output must address every question.]

1. [Question 1]
2. [Question 2]
3. [Question 3]

# Output format

```
## Findings

### Q1: [Question text]

[Answer. Cite specific files, function names, line numbers where relevant.
If the answer is uncertain, say so and explain what would resolve it.]

### Q2: [Question text]

[Answer.]

[Repeat for each question]

---

## Recommendation

[If the research was architectural: state the recommended approach with
rationale. If purely investigative: state what the next prompt should do.]

---

## Open issues

[Anything discovered that was not part of the original scope but warrants
a follow-up issue. One bullet per issue, include enough context to file it.]
```

# Task

Investigate the open questions above. Use the codebase as the primary source.
Cite external sources only when the codebase is silent on a topic. Produce
findings in the output format. Do not commit anything.
