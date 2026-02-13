# Work Git Conventions

## Core Rules

1. **Never push** unless Cody explicitly says "push" or "push it"
2. **Use `safe-git`** (aliased as `git` in work repos) — blocks push, identity changes, force ops
3. **Commit messages must read as human-written** — terse, lowercase, no AI patterns
4. **Commit per-repo as we go** — small, logical units
5. **Never create AI-sounding branch names**

## Commit Message Style

**DO:**
```
fix null check in patient query
update roi constants
clean up unused imports
add date filter to sms report
handle edge case for empty results
rename cols for clarity
```

**DON'T:**
```
Implemented null check handling for patient query endpoint
Added comprehensive date filtering functionality to SMS report
Refactored and cleaned up unused import statements for better maintainability
```

**Rules:**
- Lowercase start (no capital unless proper noun)
- No period at end
- No "implement", "functionality", "comprehensive", "enhance", "robust"
- No bullet lists in commit body unless genuinely complex
- Past tense or imperative, keep under ~50 chars
- If a body is needed, keep it factual and brief

## Workflow (GitHub as Bus)

```
Cody (laptop) → push → GitHub → pull → Chiron (worker-node)
Chiron works → commit locally → Cody says push → push → GitHub → Cody pulls
```

## Safety Checks Before Any Git Operation

- [ ] Am I in the right repo?
- [ ] Does git config user.name/user.email match Cody's work identity?
- [ ] Is the commit message human-readable and terse?
- [ ] Am I on the right branch?
- [ ] Have I been told to push? (If not, commit only)
