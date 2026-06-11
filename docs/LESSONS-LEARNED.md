# Lessons Learned

> Cross-dispatch pattern extraction. Each entry records what worked, what
> failed, and where the fix was codified. This is the institutional memory
> layer between ephemeral agent observations and durable standards.

**How entries are added:** After a dispatch run completes, triage classifies
observations from QA results and worker logs. Lessons that reflect structural
patterns (not one-off bugs) land here. See `standards/AGENTIC_PIPELINE.md §
Observations: ephemeral to tracked` for the full triage flow.

---

## Format

Each entry follows this structure:

```markdown
### YYYY-MM-DD — <short title>

**Context:** One sentence describing the dispatch, task type, or subsystem.

**What worked:** Concrete observations about what produced good outcomes.

**What failed:** Concrete observations about what produced poor outcomes.

**Structural fix:** Where the lesson was codified (file path, standard section,
issue number, or "none — one-off").
```

Entries are prepended (newest first). Do not reorder existing entries.

---

## Entries

### 2026-04-17 - standards pattern absorption (Wave 7)

**Context:** Bulk standards absorption from an external reference
project). No Rust changes; standards and workflow artifacts only.

**What worked:** Deriving the agentic pipeline standard from existing code
(`crates/energeia`) instead of specifying it independently. The implementation
already embodied the right structure; the standard made it explicit and
navigable for agents starting cold.

**What failed:** The external reference repo was not present at any expected
local checkout path.
All absorption had to be driven from the issue body description alone.

**Structural fix:** `standards/AGENTIC_PIPELINE.md` added as canonical pipeline
description. `workflow/prompts/templates/` populated with 5 role templates.
`shared/hooks/_templates/` extended with git-guard hook.
`docs/LESSONS-LEARNED.md` (this file) created as structured capture format.
`standards/STANDARDS.md` standards index updated to reference new files.
Closes #3451.
