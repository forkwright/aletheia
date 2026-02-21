# Project Status & Specification Assessment
Quickly gather repository state, identify active specs, and locate priority tasks to understand project context and workload.

## When to Use
When you need to rapidly assess the current state of a project, understand which specifications are in progress vs. draft status, identify assigned tasks, and prioritize next steps based on project documentation and attention logs.

## Steps
1. Navigate to project root and retrieve recent git history and PR status to understand recent activity
2. List key specification documents and read their headers to identify status, authors, and phases
3. Cross-reference multiple related specs to understand dependencies and completion status
4. Check priority/attention logs (e.g., PROSOCHE.md) to identify high-priority items and blockers
5. Synthesize findings into a picture of what's in progress, what's blocked, and what needs attention

## Tools Used
- exec: Execute git commands for history and PR status, read specification files with cat and head, grep for specific status information, check attention/priority logs
