# Locate Feature Implementation Across Codebase Layers

Find where a feature is implemented by tracing references across UI, stores, and backend API layers.

## When to Use
When you need to understand the full implementation of a feature that spans frontend components, state management, and backend API endpoints, or when you need to locate a specific feature's code across a multi-layered architecture.

## Steps
1. Search for the feature name in the primary source directory using case-insensitive file name patterns (try multiple case variations if initial search fails)
2. Use grep to find references in UI component files (*.svelte) to understand how the feature is exposed
3. Search in state management/store files (*.svelte.ts or *.ts) to find data fetching logic and store definitions
4. Grep for API function names or endpoint patterns in the frontend codebase to identify the API contract
5. Search the backend infrastructure directory for the actual API endpoint implementation (search in Python route files or equivalent)
6. Read the backend route handler to understand the complete implementation

## Tools Used
- find: Locate files by name pattern in specific directories
- grep: Search for text patterns in files, with case-insensitive matching and file type filtering
- read: View the content of identified files to understand implementation details
