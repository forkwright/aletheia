
## Domain knowledge

This section is populated per-agent with domain-specific facts that the
agent needs to operate correctly. Unlike SOUL.md (identity) or CONTEXT.md
(situational), domain knowledge is stable reference material: architecture
decisions, terminology, data model constraints, and integration contracts.

### What belongs here

- **Terminology.** Names used in this domain and what they mean. Resolve
  ambiguities between similar terms.
- **Invariants.** Facts that are always true and that code must not violate.
  Example: "session.turns is always sorted ascending by timestamp."
- **Contracts.** APIs, event schemas, or data formats this agent depends on
  or produces. What consumers expect.
- **Architecture decisions.** Why key structures are the way they are. Enough
  context that an agent cold-starting can make locally consistent decisions.
- **Known limitations.** What this domain intentionally does not handle.
  Prevents agents from implementing workarounds for intended constraints.

### What does not belong here

- Operational procedures (put in SOUL.md operations section)
- Personal context about the operator (put in USER.md)
- Task-specific notes (put in daily memory files)
- General standards (those live in `standards/` and apply everywhere)

### Template

```markdown
## [Domain name] — domain knowledge

### Terminology
| Term | Meaning |
|------|---------|
| [term] | [definition] |

### Invariants
- [invariant 1]
- [invariant 2]

### Contracts
| Interface | Direction | Schema / format |
|-----------|-----------|-----------------|
| [name] | produces / consumes | [description or link] |

### Architecture decisions
- **[Decision]:** [rationale]

### Known limitations
- [limitation]
```
