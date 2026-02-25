# Multi-Phase Feature Development with Git Workflow
Implements a structured pattern for developing large features across multiple sequential phases, each merged independently with tests and type-checking before proceeding.

## When to Use
When implementing a complex feature or specification that naturally decomposes into 2-4 sequential phases, where each phase builds on the previous one, adds new capabilities, and requires intermediate validation before merging to main.

## Steps
1. Merge the previous phase's PR to main and pull latest changes
2. Create a new feature branch for the current phase
3. Read/grep related source files to understand existing architecture and integration points
4. Write new module(s) implementing the phase's core functionality
5. Write comprehensive unit tests for new modules
6. Run full test suite for the module to verify tests pass
7. Run TypeScript type-checking to catch compilation errors
8. Edit integration points (main orchestration file, public API exports) to wire in new functionality
9. Fix any type errors or unused variable warnings from TypeScript
10. Stage all changes and create a descriptive commit message referencing the phase number
11. Create a pull request with detailed description of what this phase adds
12. Merge PR with squash commit and descriptive body message
13. Return to main, pull latest, and repeat for next phase
14. After all phases complete, delete local and remote feature branches and verify full test suite passes

## Tools Used
- exec: Running git commands, npm test/tsc, branch creation, merges, and PR operations
- read: Understanding existing module structure and architecture
- grep: Finding integration points and references to existing components
- write: Creating new module files and test files
- edit: Modifying orchestration and export files to integrate new functionality
- note: Documenting completion of the entire multi-phase effort
