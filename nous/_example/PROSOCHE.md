# Prosoche

This file is written by the prosoche daemon (adaptive attention system). Do not edit manually.

When active, it contains dynamically computed focus directives based on:
- Calendar events and deadlines approaching
- Unfinished tasks from recent sessions
- Signals from other nous (pending requests, escalations)
- Time-of-day and operator activity patterns

Example format when populated:

```
focus: thesis-review
urgency: high
reason: Deadline in 2 days, last session left 3 open comments unresolved
context: Operator mentioned wanting final review before submission

pending:
  - from: chiron | "Schedule follow-up for Thursday"
  - from: operator | "Look into the citation format issue"
```
