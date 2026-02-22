# Architectural Discovery and Codebase Mapping
Systematically explore and document a multi-layered software architecture to understand component relationships and identify implementation patterns.

## When to Use
When you need to understand the structure of an unfamiliar codebase, identify how components interact across layers (API → Service → Repository → Entity), find similar implementations to use as templates, or discover bugs/inconsistencies in cross-cutting concerns.

## Steps
1. Start with a core domain class (e.g., MusicFileScanner) to understand the primary pattern
2. Examine the corresponding controller/API layer to see how it's exposed
3. Trace the service/business logic layer that implements the core functionality
4. Identify and inspect intermediate interfaces and decision-making components
5. Check enum/configuration files that define supported types and extensions
6. Review similar entity implementations (Movie, TV, Book) to find established patterns
7. Examine repository interfaces and implementations for the data access layer
8. Query actual data structure and folder conventions on disk
9. Map which repositories use shared base classes (identify shared bugs/improvements)
10. Synthesize findings into a concrete improvement plan targeting the identified pattern

## Tools Used
- exec: Execute grep, cat, find, and ls commands to explore file structure and search for class definitions, interfaces, and implementations
- plan_propose: Consolidate discoveries into a structured plan addressing the identified issues across all affected components
