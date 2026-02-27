# Specification-Driven Codebase Navigation and Integration Analysis
Systematically locate, examine, and cross-reference a feature specification with its implementation across a distributed codebase to identify gaps and integration points.

## When to Use
When you need to:
- Understand the current state of implementation for a feature described in a spec document
- Identify missing or incomplete implementations (e.g., files marked "NOT YET CREATED")
- Map specification requirements to actual code across multiple modules
- Discover integration points between different subsystems (types, providers, handlers)
- Reference external implementations or patterns for guidance on completing a feature

## Steps
1. Locate the specification document using filename/number patterns and repository search
2. Review recent git history to understand implementation phase and context
3. Read the full specification to understand requirements and design
4. Explore the directory structure of the primary implementation module
5. Check for existence of key implementation files; note any missing components
6. Examine existing implementation files to understand patterns and current state
7. Search for type definitions and interfaces used by the feature across the codebase
8. Grep for specific function/method names and streaming/event dispatch patterns
9. Locate and examine reference implementations (external repos or existing patterns)
10. Verify dependency versions in package.json for required libraries
11. Check installed node_modules for API availability and type definitions
12. Trace how the feature integrates into the main application entry points and message handlers
13. Review configuration schema definitions for the feature's configuration options
14. Map gaps between specification requirements and current implementation state

## Tools Used
- exec: file system searches (find, grep, ls), git history examination, dependency inspection
- read: specification documents and implementation overview files
- Pattern: Combine find + grep to locate specifications, then cascade grep searches through related modules
- Reference: Compare with external implementations to understand best practices and missing patterns
