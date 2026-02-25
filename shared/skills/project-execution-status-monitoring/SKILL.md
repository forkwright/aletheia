# Project Execution Status Monitoring
Monitor the execution state and progress of a multi-phase project plan.

## When to Use
When you need to track the current status of an executing project, understand which phase is active, identify completed phases, and assess overall project state during development or deployment workflows.

## Steps
1. Execute a git log command to establish context about recent changes and project history
2. Query the project execution status using the plan_execute action with "status" to retrieve current project state
3. Parse the response to identify: project state (executing/completed/failed), active wave/phase, and list of all phases with their names and IDs
4. Repeat status checks if monitoring continuous execution or waiting for phase transitions

## Tools Used
- exec: for retrieving git history and project context
- plan_execute: for querying real-time project execution status and phase information