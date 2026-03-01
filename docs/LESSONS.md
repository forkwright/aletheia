# Lessons Learned

> Operational rules derived from 23 days of running the TS system. These are not theoretical — each was earned through failure or near-miss.

---

## Verification

1. **Check first, answer second.** Every agent hit the pattern of answering before verifying. Assume nothing about system state — read the actual file, query the actual service, run the actual test. (6-pattern audit, 2026-02-13)
2. **Verify output exists before reporting done.** "I wrote the file" means nothing if `ls` doesn't show it. "Tests pass" means nothing without the output. (Pattern #3)
3. **Physical verification > theoretical mapping.** Always check the actual system, not your model of it. Own docs are not evidence — that's circular reasoning.
4. **Never cite what you haven't read.** Applies to specs, docs, API references, and your own memory. If you're not sure, re-read it.

## Building

5. **Overbuild when it adds value.** Conservative scoping was a recurring failure — applying fixes narrowly, waiting for permission to expand scope. Apply broadly. The marginal cost of doing it right is almost always lower than the cost of doing it twice. (Pattern #4)
6. **Zero broken windows.** Pre-existing failures get fixed or deleted, never ignored. Broken infrastructure that stays broken becomes invisible — Letta was down 7 days before anyone noticed. (Pattern #6)
7. **First token is better than 100,000th.** Judgment degrades with context length. Split large work into focused sessions. Don't accumulate 100k+ tokens of mechanical work before the hard decisions.

## Architecture

8. **Co-primary file + DB.** Files and database are co-equal — files survive DB corruption, git-track decisions, enable handoff artifacts. DB provides query and index. Neither is subordinate.
9. **Snap changes > gradual ramps.** When a decision is made, scaffold it that day. Incremental migration plans create transition states that are harder to reason about than the before or after.
10. **Policy before implementation.** Document standards first, then build. Standards written after the code are rationalizations, not constraints.

## Planning

11. **Planning's primary value is stress-testing infrastructure.** The plan itself is secondary to the discovery that happens while planning — gaps in tooling, broken assumptions, missing capabilities.
12. **Success criteria must cover ALL requirements.** Partial criteria produce partial delivery. If a milestone has 6 requirements, the exit gate checks all 6.
13. **Structured decision artifacts over informal agreement.** Locked decisions, deferred ideas, and discretion zones — captured in writing, not conversation memory.

## Process

14. **Don't retry the same thing with minor variations.** If a command fails, understand why before trying again. One attempt, then adapt approach.
15. **Distillation does not write memory files.** The pipeline either doesn't trigger or bypasses the hook. Write to `memory/YYYY-MM-DD.md` manually during sessions — don't rely on automation.
16. **Record state before delivery, not after.** If delivery fails and state wasn't recorded, the system retries infinitely. Always persist state first. (Prosoche dedup fix, 2026-02-19)
