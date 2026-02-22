# Codebase Feature Architecture Discovery

Systematically map feature implementation patterns across a codebase by locating specs, entities, repositories, services, controllers, and tests.

## When to Use
When you need to understand how a feature is implemented across multiple architectural layers, plan feature development, or identify existing patterns to follow for new features in a structured codebase.

## Steps
1. Find and read relevant specification documents to understand feature goals and status
2. Search for existing entity/model files related to the feature
3. Locate database migrations to understand data schema changes
4. Find repository and service layer implementations
5. Identify API controller implementations
6. Locate unit tests and test patterns
7. Map the directory structure to understand architectural organization
8. Catalog all related files across core, API, and test projects
9. Extract interface definitions and base classes to understand contracts
10. Propose a development plan based on discovered patterns

## Tools Used
- exec: Execute find commands to locate files by name/path patterns, grep to search file contents, and cat to read file contents
- plan_propose: Synthesize discovered patterns into a structured development plan
