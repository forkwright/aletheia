# Cross-Codebase API Surface Mapping
Identify and understand API contracts, data models, and filtering mechanisms across multiple codebases by systematically discovering and correlating backend and frontend implementations.

## When to Use
When you need to understand how a frontend application interacts with its backend, map data types across systems, identify available API endpoints and their parameters, or verify consistency between backend implementations and frontend usage.

## Steps
1. Read documentation or session notes to understand the project scope and architecture
2. Search for specific API method names in the frontend client library (grep for method signatures)
3. Inspect frontend type definitions to understand data structures being used
4. Read complete type definition files to see all relevant interfaces
5. Read component files to understand how APIs are consumed in practice
6. Search backend codebase for corresponding API endpoints, controllers, or resources
7. Search backend for data models and filter/query request structures
8. Find and read filter model definitions to understand backend query capabilities
9. Cross-reference controller implementations to map frontend calls to backend handlers
10. Correlate frontend type definitions with backend response models

## Tools Used
- exec with grep: Search for API methods, types, controllers, and filter patterns across codebases
- read: Examine complete source files for type definitions, API implementations, and component usage patterns
- grep flags: Use -rn for recursive searching with line numbers, --include for language filtering, head for limiting output
