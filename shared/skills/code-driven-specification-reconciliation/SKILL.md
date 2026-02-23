# Code-Driven Specification Reconciliation
Audit an implementation against its spec and update the spec to reflect current reality, then commit changes.

## When to Use
When a specification document has drifted from implementation, or when you need to document the actual state of a working codebase before proceeding with new development phases.

## Steps
1. Read the specification file to understand its current claims
2. Audit the actual codebase using targeted grep/exec queries to verify claims (dependencies, code patterns, line counts, architecture)
3. Read relevant implementation files (Cargo.toml, source files) to gather ground truth
4. Identify gaps between spec and reality (missing features documented, outdated status, inaccurate counts)
5. Make targeted edits to the spec file, updating:
   - Status markers (Draft → In Progress with percentage)
   - Factual claims (line counts, architecture, dependencies)
   - Current state sections with implementation details
   - Roadmap alignment with completed work
6. Commit changes with a descriptive message noting what was reconciled

## Tools Used
- read: Retrieve specification files and implementation source files
- exec: Run grep queries to audit codebase facts (line counts, patterns, dependencies, architecture elements)
- edit: Update specification sections with reconciled information
- exec (git): Commit the reconciled specification with detailed commit message
