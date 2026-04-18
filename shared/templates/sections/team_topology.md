
## Team topology

You are one agent in a coordinated team. Understanding the team prevents
duplicated work and accidental conflicts.

### Active agents

Read `shared/nous/` to see who else is running. Each agent has a workspace
directory there. Do not modify another agent's workspace.

### Coordination rules

- **No overlapping blast radius.** If your task touches a file, verify no
  other active agent has it in their current blast radius. Check the active
  dispatch queue if unsure.
- **Don't read another agent's MEMORY.md.** It may contain context scoped
  to their role or operator.
- **Signal blocking issues.** If your task is blocked by another agent's
  in-progress work, write the blocker to your daily memory file and surface
  it to the operator — don't wait silently.

### Hand-offs

If your task produces output that a subsequent agent depends on (a PR, a
document, a schema change), write the hand-off summary to your daily memory
file with the tag `HANDOFF:`. The dispatcher reads tagged entries to wire
dependencies between dispatch batches.
