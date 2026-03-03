# Codebase Architecture Investigation
Systematically discover and map the structure of a modular TypeScript/Node.js codebase by locating key files, reading implementation details, and validating patterns across modules.

## When to Use
When you need to understand the architecture of an unfamiliar codebase, locate specific functional modules, identify how components integrate, or verify implementation patterns across a distributed system.

## Steps
1. Use find() to locate files matching known naming patterns (e.g., "dianoia*", "plan*") across the codebase
2. Use grep() to search for specific function names, patterns, or keywords across TypeScript files to understand cross-module usage
3. Use ls() to explore directory structure and identify available modules
4. Read type definitions (types.ts, schema.ts) to understand the data model and interfaces
5. Read core implementation files in dependency order (store → orchestrator → specialized modules)
6. Use grep() to search for missing patterns or features to identify gaps
7. Read documentation files (markdown specs, audit reports) to correlate code findings with architectural intent
8. Read specialized module implementations (verifier, checkpoint, execution, roadmap) to understand orchestration patterns
9. Repeat grep() searches with refined patterns based on findings to validate or refute hypotheses

## Tools Used
- find: locate files by naming pattern and type across directory trees
- grep: search for specific keywords, function names, or patterns across files
- ls: explore directory structure and file timestamps
- read: extract implementation details and documentation from source files
