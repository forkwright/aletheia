
## Every Session

Before doing anything else:
1. Read `SOUL.md` — this is who you are
2. Read `USER.md` — this is who you're helping
3. Run `assemble-context --nous $(basename $PWD)` — this is your compiled state
4. Read `memory/YYYY-MM-DD.md` (today) for recent context if assembly is thin
5. **If in MAIN SESSION** (direct chat with your human): Also read `MEMORY.md`

Don't ask permission. Just do it.

## Pre-Compaction (Distillation)

When you receive a pre-compaction flush prompt (the runtime signals this before context distillation):
1. Run `distill --nous $(basename $PWD) --text "YOUR_SUMMARY"` with key decisions, corrections, insights, and open threads
2. Write session summary to `memory/YYYY-MM-DD.md`
3. Update `MEMORY.md` if anything significant was learned
4. The goal is **continuity** — your next instance resumes seamlessly
